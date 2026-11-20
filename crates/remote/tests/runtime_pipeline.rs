//! End-to-end provisioning without a remote machine.
//!
//! `cargo test` builds this binary twice over: once as the test runner and,
//! through a generated `ssh` wrapper, once per SSH role OpenSSH would play.
//! A fake `ssh` executes the fixed probe and exec scripts locally after
//! rewriting the virtual home, forwards `-L` tunnels, and the fake `node`
//! starts a small host fixture. The production `RemoteHost` code is unchanged:
//! the same scripts, quoting, transfer, readiness, tunnel, pairing, HTTP
//! adapter, policy gate, and SSE reader run in the test.
//!
//! The fake tunnel and host both listen on loopback, exactly like the real
//! remote forward; nothing here reaches the network.

#[cfg(unix)]
mod unix {
  use std::fs::{self, OpenOptions};
  use std::io::{BufRead, BufReader, Read, Write};
  use std::net::{TcpListener, TcpStream};
  use std::path::{Path, PathBuf};
  use std::process::{Command, Stdio};
  use std::sync::mpsc;
  use std::thread;
  use std::time::{Duration, SystemTime, UNIX_EPOCH};

  use spacelens_remote::install::{HostLayout, InstallStage};
  use spacelens_remote::manifest::AgentManifest;
  use spacelens_remote::runtime::{RemoteHost, ScanStart};
  use spacelens_remote::target::SshTarget;
  use spacelens_remote::RemotePolicy;

  const MANIFEST: &str = include_str!("../agents/manifest.json");
  const VIRTUAL_HOME: &str = "/home/lens";
  const TOKEN: &str = "fixture-token";
  const INSTANCE: &str = "inst_remote_fixture";

  // ------------------------------------------------------------ entrypoints

  pub fn main() {
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(String::as_str) {
      Some("--fake-ssh") => fake_ssh(&argv[2..]),
      Some("--fixture-host") => fixture_host(&argv[2..]),
      _ => run_test(),
    }
  }

  // --------------------------------------------------------------- the test

  fn run_test() {
    let fixture = Fixture::new();
    std::env::set_var("PATH", format!("{}:{}", fixture.bin.display(), path_var()));
    std::env::set_var("FAKE_REMOTE_HOME", &fixture.home);
    std::env::set_var("FAKE_NODE_BIN", fixture.bin.join("node"));
    std::env::set_var("FAKE_NPM_BIN", fixture.bin.join("npm"));
    std::env::set_var("FAKE_SSH_LOG", &fixture.log);
    std::env::set_var("FAKE_SERVE_LOG", fixture.root.join("serve.log"));

    let manifest = AgentManifest::parse(MANIFEST).unwrap();
    let host = HostLayout::new(VIRTUAL_HOME, &manifest.host.version).unwrap();
    let local_entry = fixture.local(&host.entry);
    fs::create_dir_all(local_entry.parent().unwrap()).unwrap();
    fs::write(&local_entry, b"// fixture host entry\n").unwrap();

    let agent = fixture.write_agent();
    let mut stages = Vec::new();
    let mut progress = |stage: spacelens_remote::InstallProgress| {
      stages.push(stage.stage);
    };
    let mut source =
      |_manifest: &AgentManifest,
       _triple: &str,
       _progress: &mut dyn FnMut(spacelens_remote::InstallProgress)| {
        Ok(fs::read(&agent).unwrap())
      };

    let target = SshTarget::Host {
      user: "lens".into(),
      host: "example.test".into(),
      port: None,
    };
    // The whole production pipeline, except for the one local input the
    // client owns: the SSH executable (PATH) and the agent bytes.
    let remote = RemoteHost::connect_with(
      target.clone(),
      &manifest,
      RemotePolicy::read_only(),
      &mut source,
      &mut progress,
    )
    .expect("provisioning failed");

    assert_eq!(remote.platform, "linux");
    assert_eq!(remote.service_instance_id, INSTANCE);
    assert!(remote.device_id.starts_with("remote:"));
    for stage in [
      InstallStage::Probe,
      InstallStage::Resolve,
      InstallStage::AgentInstall,
      InstallStage::ServeStart,
      InstallStage::Tunnel,
      InstallStage::Pair,
      InstallStage::Ready,
    ] {
      assert!(stages.contains(&stage), "missing progress stage {stage:?}");
    }

    let installed = fixture.local(&format!(
      "{}/.space-lens/agents/{}/spacelens",
      VIRTUAL_HOME, manifest.agent.version
    ));
    assert_eq!(fs::read(&installed).unwrap(), fs::read(&agent).unwrap());
    assert!(is_executable(&installed));

    let pid_file = fixture.local(&host.pid_file);
    assert!(pid_file.exists(), "serve script did not write its pid file");

    let health = remote.health().unwrap();
    assert_eq!(health["serviceInstanceId"], INSTANCE);

    let roots = remote.roots().unwrap();
    assert_eq!(roots["roots"][0]["path"], VIRTUAL_HOME);

    // The host over-reports: cleanup is enabled. A read-only session must
    // still refuse it locally, before any socket is opened.
    let capabilities = remote.capabilities();
    assert_eq!(capabilities["cleanup"]["plan"], true);
    assert_eq!(capabilities["cleanup"]["mode"], "trash");

    let scan = remote
      .scan_start(&ScanStart {
        paths: vec![VIRTUAL_HOME.into()],
        label: Some("Home".into()),
        ..ScanStart::default()
      })
      .expect("scan start");
    assert_eq!(scan["scanId"], "scan_remote_1");

    let log_before_denial = fixture.log_text();
    let denied = remote
      .cleanup_plan("scan_remote_1", &["node_1".into()])
      .expect_err("cleanup plan must be denied");
    assert_eq!(denied["problem"]["code"], "Forbidden");
    assert_eq!(fixture.log_text(), log_before_denial, "denial reached HTTP");
    assert!(
      !fixture.serve_text().contains("--allow-cleanup"),
      "read-only serve was granted cleanup: {}",
      fixture.serve_text()
    );

    // Events: the fixture's first SSE response ends after two frames, so the
    // reader must reconnect with `since=2`; only the third frame then arrives.
    let (event_tx, event_rx) = mpsc::channel();
    remote.start_events(std::sync::Arc::new(move |payload| {
      let _ = event_tx.send(payload.clone());
    }));
    let mut payloads = Vec::new();
    for _ in 0..3 {
      let envelope = event_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("timed out waiting for a forwarded event");
      payloads.push(envelope);
    }
    assert_eq!(payloads[0]["kind"], "scan.updated");
    assert_eq!(payloads[1]["kind"], "scan.completed");
    assert_eq!(payloads[2]["kind"], "scan.resumed");
    thread::sleep(Duration::from_millis(200));
    let log = fixture.log_text();
    assert!(
      log.contains("GET /api/v1/events?since=0"),
      "no initial SSE request:\n{log}"
    );
    assert!(
      log.contains("GET /api/v1/events?since=2"),
      "no resumed SSE request:\n{log}"
    );
    assert_eq!(
      event_rx.try_iter().count(),
      0,
      "replayed frames reached the forwarder again"
    );

    remote.stop();
    assert!(
      !pid_file.exists(),
      "stop did not remove the remote pid file"
    );

    // A second session with cleanup granted: the serve argv carries the flag,
    // and plan/execute reach the host.
    let cleanup_remote = RemoteHost::connect_with(
      target,
      &manifest,
      RemotePolicy {
        allow_cleanup: true,
      },
      &mut source,
      &mut |_stage: spacelens_remote::InstallProgress| {},
    )
    .expect("cleanup provisioning failed");
    assert!(
      serve_text_contains("--allow-cleanup").expect("serve log"),
      "serve was not granted --allow-cleanup:\n{}",
      fixture.serve_text()
    );
    let plan = cleanup_remote
      .cleanup_plan("scan_remote_1", &["node_1".into()])
      .expect("cleanup plan with the policy granted");
    assert_eq!(plan["planId"], "plan_remote_1");
    let outcome = cleanup_remote
      .cleanup_execute("plan_remote_1")
      .expect("cleanup execute with the policy granted");
    assert_eq!(outcome["trashed"][0]["nodeId"], "node_1");

    cleanup_remote.stop();
    assert!(
      !pid_file.exists(),
      "cleanup stop did not remove the remote pid file"
    );
    eprintln!(
      "remote runtime: probe → install → serve → tunnel → pair → read → policy → SSE resume: ok"
    );
  }

  // ------------------------------------------------------------- fake roles

  /// Runs one `ssh` invocation. The wrapper passes everything OpenSSH would
  /// have received, so the option/target/command split is the real one.
  fn fake_ssh(args: &[String]) {
    if args.iter().any(|arg| arg == "-N") {
      let index = args
        .iter()
        .position(|arg| arg == "-L")
        .expect("tunnel without -L");
      forward(&args[index + 1]);
      return;
    }
    let command = args.last().expect("ssh invocation without a command");
    if command.contains("SPACELENS-PROBE") {
      // Run the exact argv the client built. This proves the multi-line
      // script is one quoted word the remote login shell can parse; the
      // values are then replaced with the fixture's Linux answers.
      let check = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .env("HOME", home())
        .stdin(Stdio::null())
        .output()
        .expect("cannot run the probe script");
      assert!(
        check.status.success(),
        "probe script failed: {}",
        String::from_utf8_lossy(&check.stderr)
      );
      assert!(String::from_utf8_lossy(&check.stdout).contains("SPACELENS-PROBE"));
      print_probe();
      return;
    }
    // Everything else (mkdir, cat transfer, wc, chmod, mv, agent-info, the
    // serve script, kill) is a POSIX script. Run it locally after mapping the
    // virtual home onto the fixture home. `exec` keeps one process per SSH
    // session, so killing the session (as the real ssh child would be killed)
    // also ends the command it started.
    use std::os::unix::process::CommandExt;
    let rewritten = command.replace(VIRTUAL_HOME, &home());
    let error = Command::new("/bin/sh")
      .arg("-c")
      .arg(&rewritten)
      .stdin(Stdio::inherit())
      .stdout(Stdio::inherit())
      .stderr(Stdio::inherit())
      .exec();
    eprintln!("fake ssh cannot run the remote shell: {error}");
    std::process::exit(127);
  }

  fn print_probe() {
    let node = std::env::var("FAKE_NODE_BIN").unwrap();
    let npm = std::env::var("FAKE_NPM_BIN").unwrap();
    print!(
      "SPACELENS-PROBE\tos=Linux\n\
       SPACELENS-PROBE\tarch=x86_64\n\
       SPACELENS-PROBE\thome={VIRTUAL_HOME}\n\
       SPACELENS-PROBE\tshell=/bin/bash\n\
       SPACELENS-PROBE\twritable=yes\n\
       SPACELENS-PROBE\tnode={node}\n\
       SPACELENS-PROBE\tnode_version=v22.0.0\n\
       SPACELENS-PROBE\tnpm={npm}\n"
    );
    std::io::stdout().flush().unwrap();
  }

  /// The fake `node` named by the probe. It serves the host routes the remote
  /// runtime uses and prints the readiness/pairing lines `serve --machine`
  /// prints.
  fn fixture_host(args: &[String]) {
    // The probe runs `node --version` while checking the remote. Answer that
    // like Node; record the serve argv like a real launch; reserve the HTTP
    // fixture for a real serve invocation.
    if args.iter().any(|arg| arg == "--version") {
      println!("v22.0.0");
      return;
    }
    if let Ok(log) = std::env::var("FAKE_SERVE_LOG") {
      if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log) {
        let _ = file.write_all(format!("{}\n", args.join(" ")).as_bytes());
      }
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture host bind");
    let port = listener.local_addr().unwrap().port();
    let log = std::env::var("FAKE_SSH_LOG").unwrap();
    thread::spawn(move || {
      for stream in listener.incoming().flatten() {
        let log = log.clone();
        thread::spawn(move || handle(stream, &log));
      }
    });

    println!(
      "{{\"serviceInstanceId\":\"{INSTANCE}\",\"port\":{port},\"url\":\"http://127.0.0.1:{port}\",\"ticketSingleUse\":true}}"
    );
    std::io::stdout().flush().unwrap();
    eprintln!("pairing URL (single use): http://127.0.0.1:{port}/?pair={TOKEN}");
    std::io::stderr().flush().unwrap();
    loop {
      thread::park();
    }
  }

  fn forward(spec: &str) {
    // 127.0.0.1:<local>:127.0.0.1:<remote>
    let parts: Vec<&str> = spec.split(':').collect();
    assert_eq!(parts.len(), 4, "unexpected forward spec {spec}");
    let local: u16 = parts[1].parse().unwrap();
    let remote: u16 = parts[3].parse().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", local)).expect("tunnel bind");
    for client in listener.incoming().flatten() {
      let Ok(upstream) = TcpStream::connect(("127.0.0.1", remote)) else {
        continue;
      };
      thread::spawn(move || bridge(client, upstream));
    }
  }

  fn bridge(client: TcpStream, upstream: TcpStream) {
    let client_read = client.try_clone().unwrap();
    let upstream_read = upstream.try_clone().unwrap();
    thread::spawn(move || pump(upstream_read, client));
    pump(client_read, upstream);
  }

  fn pump(mut from: TcpStream, mut to: TcpStream) {
    let _ = std::io::copy(&mut from, &mut to);
    let _ = to.shutdown(std::net::Shutdown::Write);
  }

  // ------------------------------------------------------------ HTTP fixture

  fn handle(mut stream: TcpStream, log: &str) {
    let reader_stream = match stream.try_clone() {
      Ok(stream) => stream,
      Err(_) => return,
    };
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
      return;
    }
    let mut headers = Vec::new();
    loop {
      let mut line = String::new();
      if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
      }
      if line.trim().is_empty() {
        break;
      }
      headers.push(line.trim_end().to_string());
    }
    let header = |name: &str| {
      headers.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key
          .eq_ignore_ascii_case(name)
          .then(|| value.trim().to_string())
      })
    };
    let body = read_body(&mut reader, header("content-length"));

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    append_log(
      log,
      &format!(
        "{method} {path} auth={} origin={}",
        header("authorization").unwrap_or_default(),
        header("origin").unwrap_or_default()
      ),
    );

    let route = path.split('?').next().unwrap_or(path);
    if route == "/health" {
      respond_json(
        &mut stream,
        "200 OK",
        &format!(
          "{{\"status\":\"ok\",\"serviceInstanceId\":\"{INSTANCE}\",\"apiMajor\":1,\"contractVersion\":\"1.1.0\"}}"
        ),
      );
      return;
    }
    if route == "/api/v1/session/exchange" {
      if header("origin").is_some() {
        respond_json(
          &mut stream,
          "403 Forbidden",
          r#"{"problem":{"code":"Forbidden","message":"origin","retryable":false}}"#,
        );
        return;
      }
      assert_eq!(body, format!("{{\"ticket\":\"{TOKEN}\"}}"));
      respond_json(&mut stream, "200 OK", &format!("{{\"token\":\"{TOKEN}\"}}"));
      return;
    }
    if route.starts_with("/api/")
      && header("authorization").as_deref() != Some(&format!("Bearer {TOKEN}"))
    {
      respond_json(
        &mut stream,
        "401 Unauthorized",
        r#"{"problem":{"code":"Unauthenticated","message":"token","retryable":false}}"#,
      );
      return;
    }

    match (method, route) {
      ("GET", "/api/v1/capabilities") => respond_json(
        &mut stream,
        "200 OK",
        r#"{
          "apiMajor": 1,
          "contractVersion": "1.1.0",
          "scan": {"start": true, "cancel": true, "discovery": true, "appAudit": true, "appScan": true, "maxConcurrent": 2},
          "cleanup": {"plan": true, "execute": true, "mode": "trash", "appPlan": true, "appExecute": true},
          "host": {"folderPicker": true, "appLocationReveal": true, "pathReveal": true, "platform": "linux"},
          "icloud": "unavailable",
          "memory": {"sample": true, "terminate": true, "gracefulAppQuit": true, "reopen": true, "platform": "linux"}
        }"#,
      ),
      ("GET", "/api/v1/roots") => respond_json(
        &mut stream,
        "200 OK",
        &format!(
          "{{\"roots\":[{{\"id\":\"root_home\",\"path\":\"{VIRTUAL_HOME}\",\"label\":\"Home\"}}]}}"
        ),
      ),
      ("POST", "/api/v1/scans") => respond_json(
        &mut stream,
        "200 OK",
        r#"{"scanId":"scan_remote_1","rootIds":["root_home"],"createdAt":"2026-01-01T00:00:00Z","label":"Home"}"#,
      ),
      ("POST", "/api/v1/cleanup/plan") => respond_json(
        &mut stream,
        "200 OK",
        r#"{"planId":"plan_remote_1","entries":[{"path":"/home/lens/target","size":1024,"reason":"node_modules","fingerprint":{"size":1024,"mtimeMs":1.0}}],"totalSize":1024}"#,
      ),
      ("POST", "/api/v1/cleanup/execute") => respond_json(
        &mut stream,
        "200 OK",
        r#"{"trashed":[{"nodeId":"node_1","path":"/home/lens/target","size":1024}],"bytesFreed":1024,"failed":[]}"#,
      ),
      ("GET", "/api/v1/events") => {
        let since: u64 = query(path, "since")
          .and_then(|value| value.parse().ok())
          .unwrap_or(0);
        if since < 2 {
          stream
            .write_all(
              b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n\
                data: {\"sequence\":1,\"emittedAt\":\"2026-01-01T00:00:00Z\",\"payload\":{\"kind\":\"scan.updated\"}}\n\n\
                data: {\"sequence\":2,\"emittedAt\":\"2026-01-01T00:00:01Z\",\"payload\":{\"kind\":\"scan.completed\"}}\n\n",
            )
            .unwrap();
          let _ = stream.flush();
          // Dropping the connection is what makes the client resume.
        } else {
          stream
            .write_all(
              b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n\
                data: {\"sequence\":3,\"emittedAt\":\"2026-01-01T00:00:02Z\",\"payload\":{\"kind\":\"scan.resumed\"}}\n\n",
            )
            .unwrap();
          let _ = stream.flush();
          // Keep the stream open like a quiet host, so only an explicit stop
          // ends the reader.
          thread::sleep(Duration::from_secs(300));
        }
      }
      _ => respond_json(
        &mut stream,
        "404 Not Found",
        r#"{"problem":{"code":"NotFound","message":"route","retryable":false}}"#,
      ),
    }
  }

  fn read_body(reader: &mut BufReader<TcpStream>, length: Option<String>) -> String {
    let length: usize = length.and_then(|value| value.parse().ok()).unwrap_or(0);
    let mut body = vec![0; length];
    if length > 0 {
      reader.read_exact(&mut body).unwrap();
    }
    String::from_utf8(body).unwrap()
  }

  fn respond_json(stream: &mut TcpStream, status: &str, body: &str) {
    write!(
      stream,
      "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
      body.len()
    )
    .unwrap();
    let _ = stream.flush();
  }

  fn append_log(path: &str, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
      let _ = file.write_all(format!("{line}\n").as_bytes());
    }
  }

  fn query(path: &str, key: &str) -> Option<String> {
    path
      .split_once('?')?
      .1
      .split('&')
      .find_map(|pair| pair.split_once('=').filter(|(name, _)| *name == key))
      .map(|(_, value)| value.to_string())
  }

  // --------------------------------------------------------------- fixtures

  struct Fixture {
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    log: PathBuf,
  }

  impl Fixture {
    fn new() -> Self {
      let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
      let root = std::env::temp_dir().join(format!(
        "space-lens-remote-pipeline-{}-{stamp}",
        std::process::id()
      ));
      let home = root.join("home");
      let bin = root.join("bin");
      fs::create_dir_all(&home).unwrap();
      fs::create_dir_all(&bin).unwrap();
      let exe = std::env::current_exe().unwrap();
      write_wrapper(
        &bin.join("ssh"),
        &format!("exec '{}' --fake-ssh \"$@\"", exe.display()),
      );
      write_wrapper(
        &bin.join("node"),
        &format!("exec '{}' --fixture-host \"$@\"", exe.display()),
      );
      write_wrapper(&bin.join("npm"), "exit 1");
      Self {
        log: root.join("ssh.log"),
        root,
        home,
        bin,
      }
    }

    fn write_agent(&self) -> PathBuf {
      let path = self.root.join("agent.bin");
      fs::write(
        &path,
        "#!/bin/sh\n\
         if [ \"$1\" = agent-info ] && [ \"$2\" = --json ]; then\n\
           printf '%s\\n' '{\"agentProtocol\":1,\"version\":\"0.4.0\",\"platform\":\"linux\",\"arch\":\"x86_64\",\"features\":[\"scan.stream\",\"scan.progress\",\"memory.stdio\"]}'\n\
           exit 0\n\
         fi\n\
         exit 1\n",
      )
      .unwrap();
      set_executable(&path);
      path
    }

    /// Maps one virtual-home path to the fixture directory.
    fn local(&self, path: &str) -> PathBuf {
      PathBuf::from(path.replacen(VIRTUAL_HOME, self.home.to_str().unwrap(), 1))
    }

    fn log_text(&self) -> String {
      fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn serve_text(&self) -> String {
      fs::read_to_string(self.root.join("serve.log")).unwrap_or_default()
    }
  }

  impl Drop for Fixture {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.root);
    }
  }

  fn write_wrapper(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    set_executable(path);
  }

  fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
  }

  fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
  }

  /// The fixture's serve log accumulates every session's argv; the read-only
  /// assertion runs before the cleanup session starts, so containment is exact.
  fn serve_text_contains(needle: &str) -> Result<bool, String> {
    let log = std::env::var("FAKE_SERVE_LOG").map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(&log).map_err(|error| error.to_string())?;
    Ok(text.contains(needle))
  }

  fn home() -> String {
    std::env::var("FAKE_REMOTE_HOME").expect("FAKE_REMOTE_HOME")
  }

  fn path_var() -> String {
    std::env::var("PATH").unwrap_or_default()
  }
}

#[cfg(unix)]
fn main() {
  unix::main();
}

#[cfg(not(unix))]
fn main() {}
