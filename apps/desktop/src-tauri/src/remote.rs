//! The remote device runtime: provision over SSH, start `spacelens serve` on
//! the remote loopback, reach it through an SSH local forward, and speak the
//! existing host HTTP API as a native client.
//!
//! The WebView never talks to the remote host. It keeps calling the closed
//! `sl_*` commands; [`RemoteHost`] maps each allowed request onto the host's
//! frozen routes and refuses every mutation before a socket is opened.
//!
//! Trust boundary:
//!
//! - the user's OpenSSH is the only transport; no password, key, or fingerprint
//!   is handled here;
//! - the agent and host versions are pinned by the embedded manifest;
//! - the host is started without `--allow-cleanup` and
//!   `--allow-process-control`, and this module denies those requests again;
//! - the remote server binds loopback and is reached only through
//!   `127.0.0.1:<local>` on this machine.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{json, Value};
use spacelens_remote::agent::{AgentInfo, AGENT_PROTOCOL};
use spacelens_remote::install::{AgentLayout, HostLayout, InstallProgress, InstallStage};
use spacelens_remote::manifest::AgentManifest;
use spacelens_remote::probe::{parse_probe, resolve_target, validate_absolute_path, RemoteProbe};
use spacelens_remote::server::{
  host_install_script, kill_script, parse_readiness_line, serve_script, ssh_exec, ssh_probe,
  ssh_transfer, ssh_tunnel, ReadinessLine, ServeReadiness, SshCommand,
};
use spacelens_remote::target::SshTarget;
use spacelens_remote::RemoteError;
use tauri::Manager;

use crate::commands::{ReadRequest, SubmitRequest};
use crate::events::EventSink;

/// Compiled into the app: an install is authorized by what shipped, never by
/// what a server says today. Regenerate with `scripts/agent-manifest.ts`.
const MANIFEST_JSON: &str = include_str!("../agents/manifest.json");

const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
const AGENT_TRANSFER_TIMEOUT: Duration = Duration::from_secs(180);
const HOST_INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
const SERVE_START_TIMEOUT: Duration = Duration::from_secs(300);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(600);

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// A connected remote host. One per `deviceId`; the frontend reaches it
/// through a device-scoped session.
pub struct RemoteHost {
  pub device_id: String,
  pub label: String,
  pub generation: u64,
  pub platform: String,
  pub service_instance_id: String,
  pub target: SshTarget,
  pid_file: String,
  base_url: String,
  token: Mutex<String>,
  client: reqwest::blocking::Client,
  stream_client: reqwest::blocking::Client,
  capabilities: Value,
  tunnel: Mutex<Option<Child>>,
  server: Mutex<Option<Child>>,
  sse_stop: Arc<AtomicBool>,
  sse_running: Arc<AtomicBool>,
  /// The last remote event sequence the sink accepted, so a renderer reload
  /// resumes instead of replaying the host's whole event ring.
  sse_last: Arc<AtomicU64>,
  sse: Mutex<Option<thread::JoinHandle<()>>>,
  shutdown: AtomicBool,
}

/// Produces the agent binary for one release target. The app downloads and
/// verifies the pinned archive; tests hand in a fixture and exercise the same
/// provisioning path without an app handle or the network.
pub type AgentSource<'a> = &'a mut dyn FnMut(
  &AgentManifest,
  &str,
  &mut dyn FnMut(InstallProgress),
) -> Result<Vec<u8>, RemoteError>;

impl RemoteHost {
  /// Full provisioning + connection. Long-running and blocking; call it from a
  /// blocking thread, never the UI thread.
  pub fn connect(
    app: &tauri::AppHandle,
    target: SshTarget,
    progress: &mut dyn FnMut(InstallProgress),
  ) -> Result<Arc<Self>, RemoteError> {
    let mut source =
      |manifest: &AgentManifest, target: &str, progress: &mut dyn FnMut(InstallProgress)| {
        agent_bytes(app, manifest, target, progress)
      };
    Self::connect_with(target, &mut source, progress)
  }

  /// The provisioning pipeline with its one app-owned input injected. Public
  /// only so the integration test can run it without a Tauri window; callers
  /// in the app use [`RemoteHost::connect`].
  #[doc(hidden)]
  pub fn connect_with(
    target: SshTarget,
    source: AgentSource<'_>,
    progress: &mut dyn FnMut(InstallProgress),
  ) -> Result<Arc<Self>, RemoteError> {
    target.validate()?;
    let manifest = AgentManifest::parse(MANIFEST_JSON)?;
    install_crypto_provider();

    // 1. Probe: fixed script, no user value interpolated.
    progress(InstallProgress::new(
      InstallStage::Probe,
      format!("Checking {target} over SSH"),
    ));
    let probe_output = run_ssh(&target, &ssh_probe(&target)?, None, PROBE_TIMEOUT)?;
    if !probe_output.success {
      return Err(RemoteError::Probe(format!(
        "ssh probe failed: {}",
        tail(&probe_output.stderr)
      )));
    }
    let probe = parse_probe(&String::from_utf8_lossy(&probe_output.stdout))
      .map_err(|error| RemoteError::Probe(error.to_string()))?;
    if !probe.shell_supported() {
      return Err(RemoteError::UnsupportedShell(format!(
        "remote login shell {:?} is not POSIX-compatible",
        probe.shell
      )));
    }
    if !probe.writable {
      return Err(RemoteError::stage(
        InstallStage::AgentInstall,
        format!("{} is not writable by this user", probe.home),
      ));
    }
    validate_absolute_path(&probe.home, "home")?;
    if probe.platform() != "linux" {
      // The Linux storage beta ships first; macOS is a later gate. The agent
      // is built for macOS, so only the policy here says no.
      return Err(RemoteError::UnsupportedTarget(format!(
        "remote {} hosts are not supported in this version",
        probe.platform()
      )));
    }
    if !probe.supports_node(manifest.host.min_node) {
      return Err(RemoteError::UnsupportedTarget(format!(
        "remote needs Node.js >= {} and npm; found node {:?} npm {:?}",
        manifest.host.min_node, probe.node_version, probe.npm
      )));
    }

    // 2. Resolve the release target.
    progress(InstallProgress::new(
      InstallStage::Resolve,
      "Resolving the remote architecture",
    ));
    let resolved = resolve_target(&probe.os, &probe.arch)?;

    // 3. Install the Rust agent (protected scan, memory stdio, audit).
    let agent = AgentLayout::new(&probe.home, &manifest.agent.version, &staging_suffix())?;
    ensure_agent(
      source,
      &target,
      &manifest,
      resolved.triple,
      &agent,
      progress,
    )?;

    // 4. Install or verify the pinned host package.
    let host = HostLayout::new(&probe.home, &manifest.host.version)?;
    ensure_host(&target, &probe, &host, progress)?;

    // 5. Start `spacelens serve --machine`; readiness on stdout, ticket on stderr.
    progress(InstallProgress::new(
      InstallStage::ServeStart,
      "Starting the remote host",
    ));
    // A previous app run can leave a server behind (the pid file outlives the
    // tunnel). Stop it first so provisioning does not stack orphaned hosts.
    if let Ok(command) = kill_script(&host.pid_file) {
      if let Ok(command) = ssh_exec(&target, &command) {
        let _ = run_ssh(&target, &command, None, Duration::from_secs(10));
      }
    }
    let node = probe
      .node
      .as_deref()
      .ok_or_else(|| RemoteError::UnsupportedTarget("node was not found".into()))?;
    let (mut server, readiness) = start_serve(&target, &host, &agent.binary, node, &probe.home)?;

    // 6. Tunnel to the remote loopback.
    progress(InstallProgress::new(
      InstallStage::Tunnel,
      "Opening a loopback tunnel",
    ));
    let (mut tunnel, local_port) = match start_tunnel(&target, readiness.port) {
      Ok(tunnel) => tunnel,
      Err(error) => {
        // `Child` drop does not kill: an unwound serve child would keep the
        // remote host alive until the next connect reaped its pid file.
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };
    let base_url = format!("http://127.0.0.1:{local_port}");

    // 7. Exchange the single-use ticket. No Origin header is sent, which is
    // exactly what a `--machine` ticket is bound to.
    progress(InstallProgress::new(
      InstallStage::Pair,
      "Pairing with the remote host",
    ));
    let client = match http_client(REQUEST_TIMEOUT) {
      Ok(client) => client,
      Err(error) => {
        let _ = tunnel.kill();
        let _ = tunnel.wait();
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };
    let token = match pair(&client, &base_url, &readiness.ticket) {
      Ok(token) => token,
      Err(error) => {
        let _ = tunnel.kill();
        let _ = tunnel.wait();
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };

    let capabilities = match fetch_capabilities(&client, &base_url, &token) {
      Ok(capabilities) => capabilities,
      Err(error) => {
        let _ = tunnel.kill();
        let _ = tunnel.wait();
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };

    // The SSE client has no total timeout: the event stream is long-lived.
    let stream_client = match http_client(Duration::from_secs(0)) {
      Ok(client) => client,
      Err(error) => {
        let _ = tunnel.kill();
        let _ = tunnel.wait();
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };

    progress(InstallProgress::new(
      InstallStage::Ready,
      format!("{} is ready", target.label()),
    ));
    Ok(Arc::new(Self {
      device_id: target.device_id(),
      label: target.label(),
      generation: NEXT_GENERATION.fetch_add(1, Ordering::SeqCst),
      platform: probe.platform().to_string(),
      service_instance_id: readiness.service_instance_id.clone(),
      target,
      pid_file: host.pid_file.clone(),
      base_url,
      token: Mutex::new(token),
      client,
      stream_client,
      capabilities,
      tunnel: Mutex::new(Some(tunnel)),
      server: Mutex::new(Some(server)),
      sse_stop: Arc::new(AtomicBool::new(false)),
      sse_running: Arc::new(AtomicBool::new(false)),
      sse_last: Arc::new(AtomicU64::new(0)),
      sse: Mutex::new(None),
      shutdown: AtomicBool::new(false),
    }))
  }

  /// Starts forwarding remote SSE payloads into this session's sink. Repeated
  /// calls while a reader is live are no-ops: `sl_connect` runs on every
  /// renderer load and must not duplicate events.
  pub fn start_events(&self, sink: EventSink) {
    if self.sse_running.swap(true, Ordering::SeqCst) {
      return;
    }
    // A reader that exited without being stopped (the tunnel dropped) is
    // already winding down; join it so only one reader owns the sink.
    if let Some(handle) = self.sse.lock().unwrap().take() {
      let _ = handle.join();
    }
    self.sse_stop.store(false, Ordering::SeqCst);
    let stop_flag = self.sse_stop.clone();
    let running = self.sse_running.clone();
    let last = self.sse_last.clone();
    let base_url = self.base_url.clone();
    let token = self.token.lock().unwrap().clone();
    let client = self.stream_client.clone();
    let handle = thread::spawn(move || {
      let mut since = last.load(Ordering::SeqCst);
      while !stop_flag.load(Ordering::SeqCst) {
        let response = client
          .get(format!("{base_url}/api/v1/events"))
          .query(&[("since", since.to_string())])
          .bearer_auth(&token)
          .send();
        let Ok(response) = response else {
          sleep_unless(&stop_flag, Duration::from_secs(2));
          continue;
        };
        if !response.status().is_success() {
          sleep_unless(&stop_flag, Duration::from_secs(2));
          continue;
        }
        let reader = BufReader::new(response);
        let mut data = String::new();
        for line in reader.lines() {
          if stop_flag.load(Ordering::SeqCst) {
            running.store(false, Ordering::SeqCst);
            return;
          }
          let Ok(line) = line else { break };
          if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim());
          } else if line.is_empty() && !data.is_empty() {
            if let Ok(envelope) = serde_json::from_str::<Value>(&data) {
              if let Some(sequence) = envelope.get("sequence").and_then(Value::as_u64) {
                if sequence > since {
                  since = sequence;
                  last.store(sequence, Ordering::SeqCst);
                  if let Some(payload) = envelope.get("payload") {
                    sink.publish(payload.clone());
                  }
                }
              }
            }
            data.clear();
          }
        }
        sleep_unless(&stop_flag, Duration::from_secs(1));
      }
      running.store(false, Ordering::SeqCst);
    });
    self.sse.lock().unwrap().replace(handle);
  }

  fn signal_events_stop(&self) {
    self.sse_stop.store(true, Ordering::SeqCst);
    self.sse_running.store(false, Ordering::SeqCst);
  }

  /// Signals the reader and waits for it. Only safe after the tunnel is gone:
  /// the reader may be blocked on a response body, and killing the tunnel is
  /// what unblocks it.
  fn join_events(&self) {
    if let Some(handle) = self.sse.lock().unwrap().take() {
      let _ = handle.join();
    }
  }

  pub fn stop_events(&self) {
    self.signal_events_stop();
    self.join_events();
  }

  /// Stops the tunnel, the SSH server child, and the remote server process.
  /// Best effort: a host that is already gone is not an error.
  pub fn stop(&self) {
    if self.shutdown.swap(true, Ordering::SeqCst) {
      return;
    }
    self.signal_events_stop();
    // Closing the tunnel unblocks the SSE reader.
    self.stop_local_children();
    self.join_events();
    let command = kill_script(&self.pid_file).ok();
    if let Some(command) = command {
      if let Ok(command) = ssh_exec(&self.target, &command) {
        let _ = run_ssh(&self.target, &command, None, Duration::from_secs(10));
      }
    }
  }

  fn stop_local_children(&self) {
    if let Some(mut child) = self.tunnel.lock().unwrap().take() {
      let _ = child.kill();
      let _ = child.wait();
    }
    if let Some(mut child) = self.server.lock().unwrap().take() {
      let _ = child.kill();
      let _ = child.wait();
    }
  }

  // ------------------------------------------------------------- HTTP reads

  pub fn read(&self, request: &ReadRequest) -> Result<Value, Value> {
    match request {
      ReadRequest::Health => self.call(Method::GET, "/health", None, REQUEST_TIMEOUT),
      ReadRequest::Capabilities => Ok(remote_capabilities(
        self.capabilities.clone(),
        &self.platform,
      )),
      ReadRequest::Roots => self.call(Method::GET, "/api/v1/roots", None, REQUEST_TIMEOUT),
      ReadRequest::Volumes => self.call(Method::GET, "/api/v1/volumes", None, REQUEST_TIMEOUT),
      ReadRequest::ScanStatus { scan_id } => self.call(
        Method::GET,
        &format!("/api/v1/scans/{scan_id}"),
        None,
        REQUEST_TIMEOUT,
      ),
      ReadRequest::ScanCancel { scan_id } => self.call(
        Method::POST,
        &format!("/api/v1/scans/{scan_id}/cancel"),
        Some(json!({})),
        REQUEST_TIMEOUT,
      ),
      ReadRequest::ScanList => {
        let value = self.call(Method::GET, "/api/v1/scans", None, REQUEST_TIMEOUT)?;
        Ok(value.get("scans").cloned().unwrap_or_else(|| json!([])))
      }
      ReadRequest::TreeSlice {
        scan_id,
        node_id,
        depth,
        max_children_per_node,
      } => self.call(
        Method::POST,
        "/api/v1/tree/slice",
        Some(json!({
          "scanId": scan_id,
          "nodeId": node_id,
          "depth": depth,
          "maxChildrenPerNode": max_children_per_node,
        })),
        REQUEST_TIMEOUT,
      ),
      ReadRequest::TreeChildren {
        scan_id,
        node_id,
        offset,
        limit,
        sort,
      } => self.call(
        Method::POST,
        "/api/v1/tree/children",
        Some(json!({
          "scanId": scan_id,
          "nodeId": node_id,
          "offset": offset,
          "limit": limit,
          "sort": sort,
        })),
        REQUEST_TIMEOUT,
      ),
      ReadRequest::Discovery {
        scan_id,
        kind,
        min_size,
        offset,
        limit,
      } => self.call(
        Method::POST,
        "/api/v1/discovery",
        Some(json!({
          "scanId": scan_id,
          "kind": kind,
          "minSize": min_size,
          "offset": offset,
          "limit": limit,
        })),
        DISCOVERY_TIMEOUT,
      ),
      ReadRequest::AppAudit {
        min_size,
        include_heuristic,
        limit,
      } => self.call(
        Method::POST,
        "/api/v1/apps/audit",
        Some(json!({
          "minSize": min_size,
          "includeHeuristic": include_heuristic,
          "limit": limit,
        })),
        DISCOVERY_TIMEOUT,
      ),
      ReadRequest::AppIcon { app_path } => self.call(
        Method::POST,
        "/api/v1/apps/icon",
        Some(json!({ "appPath": app_path })),
        DISCOVERY_TIMEOUT,
      ),
      ReadRequest::MemorySnapshot => self.call(
        Method::GET,
        "/api/v1/memory/snapshot",
        None,
        REQUEST_TIMEOUT,
      ),
      ReadRequest::MemoryHistory { group_ids } => self.call(
        Method::POST,
        "/api/v1/memory/history",
        Some(json!({ "groupIds": group_ids })),
        REQUEST_TIMEOUT,
      ),
      ReadRequest::MemoryCandidates { exclude } => self.call(
        Method::POST,
        "/api/v1/memory/candidates",
        Some(json!({ "exclude": exclude })),
        REQUEST_TIMEOUT,
      ),
      ReadRequest::MemoryAudit => {
        self.call(Method::GET, "/api/v1/memory/audit", None, REQUEST_TIMEOUT)
      }
    }
  }

  /// Remote v1 is read-only. The host is started without the mutation flags,
  /// and this match denies again before any socket is opened.
  pub fn submit(&self, request: &SubmitRequest) -> Result<Value, Value> {
    let Some(path) = submit_route(request) else {
      return Err(forbidden(
        "cleanup and process termination are disabled for remote hosts",
      ));
    };
    match request {
      SubmitRequest::ScanStart { request } => self.call(
        Method::POST,
        path,
        Some(serde_json::to_value(request).map_err(internal)?),
        DISCOVERY_TIMEOUT,
      ),
      _ => Err(forbidden(
        "cleanup and process termination are disabled for remote hosts",
      )),
    }
  }

  fn call(
    &self,
    method: Method,
    path: &str,
    body: Option<Value>,
    timeout: Duration,
  ) -> Result<Value, Value> {
    let token = self.token.lock().unwrap().clone();
    let mut request = self
      .client
      .request(method, format!("{}{}", self.base_url, path))
      .timeout(timeout)
      .bearer_auth(token);
    if let Some(body) = body {
      request = request.json(&body);
    }
    let response = request.send().map_err(|error| {
      json!({"problem": {
        "code": if error.is_timeout() { "Timeout" } else { "HostUnavailable" },
        "message": error.to_string(),
        "retryable": true
      }})
    })?;
    let status = response.status();
    let value: Value = response.json().unwrap_or(Value::Null);
    if status.is_success() {
      Ok(value)
    } else if let Some(problem) = value.get("problem") {
      Err(json!({ "problem": problem }))
    } else {
      Err(json!({"problem": {
        "code": code_for_status(status.as_u16()),
        "message": format!("remote host answered HTTP {}", status.as_u16()),
        "retryable": status.is_server_error()
      }}))
    }
  }
}

/// The complete remote mutation allow-list. One arm: starting a read-only
/// scan. Everything else is absent by construction, not by a default.
pub fn submit_route(request: &SubmitRequest) -> Option<&'static str> {
  match request {
    SubmitRequest::ScanStart { .. } => Some("/api/v1/scans"),
    SubmitRequest::CleanupPlan { .. }
    | SubmitRequest::CleanupExecute { .. }
    | SubmitRequest::AppCleanupPlan { .. }
    | SubmitRequest::AppCleanupExecute { .. }
    | SubmitRequest::AppScanStart { .. }
    | SubmitRequest::MemoryTerminatePlan { .. }
    | SubmitRequest::MemoryTerminateExecute { .. } => None,
  }
}

impl Drop for RemoteHost {
  fn drop(&mut self) {
    // Never open a network connection just because a value was dropped, and
    // never block on a reader whose socket is still open.
    self.signal_events_stop();
    self.stop_local_children();
  }
}

// ------------------------------------------------------------- provisioning

fn ensure_agent(
  source: AgentSource<'_>,
  target: &SshTarget,
  manifest: &AgentManifest,
  triple: &str,
  layout: &AgentLayout,
  progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), RemoteError> {
  if agent_healthy(target, &layout.binary)? {
    return Ok(());
  }
  let bytes = source(manifest, triple, progress)?;
  progress(InstallProgress::new(
    InstallStage::AgentInstall,
    format!("Installing the agent under {}", layout.version_dir),
  ));

  let staging = &layout.staging_dir;
  let mkdir = format!(
    "mkdir -p {} && chmod 700 {}",
    quote(staging)?,
    quote(staging)?
  );
  let created = run_ssh(
    target,
    &ssh_exec(target, &script(&mkdir)?)?,
    None,
    PROBE_TIMEOUT,
  )?;
  if !created.success {
    return Err(RemoteError::stage(
      InstallStage::AgentInstall,
      format!("cannot create staging directory: {}", tail(&created.stderr)),
    ));
  }

  let result = (|| -> Result<(), RemoteError> {
    let remote_binary = format!("{staging}/spacelens");
    let transfer = run_ssh(
      target,
      &ssh_transfer(target, &remote_binary)?,
      Some(bytes.clone()),
      AGENT_TRANSFER_TIMEOUT,
    )?;
    if !transfer.success {
      return Err(RemoteError::stage(
        InstallStage::AgentInstall,
        format!("transfer failed: {}", tail(&transfer.stderr)),
      ));
    }
    let wc = run_ssh(
      target,
      &ssh_exec(
        target,
        &script(&format!("wc -c < {}", quote(&remote_binary)?))?,
      )?,
      None,
      PROBE_TIMEOUT,
    )?;
    let transferred: u64 = String::from_utf8_lossy(&wc.stdout)
      .trim()
      .parse()
      .unwrap_or(0);
    if transferred != bytes.len() as u64 {
      return Err(RemoteError::stage(
        InstallStage::Verify,
        format!(
          "transfer wrote {transferred} bytes, expected {}",
          bytes.len()
        ),
      ));
    }

    let smoke_script = format!(
      "chmod 755 {binary} && exec {binary} agent-info --json",
      binary = quote(&remote_binary)?
    );
    let smoke = run_ssh(
      target,
      &ssh_exec(target, &script(&smoke_script)?)?,
      None,
      PROBE_TIMEOUT,
    )?;
    if !smoke.success {
      return Err(RemoteError::stage(
        InstallStage::AgentInstall,
        format!("agent smoke test failed: {}", tail(&smoke.stderr)),
      ));
    }
    let info = AgentInfo::parse(&String::from_utf8_lossy(&smoke.stdout))?;
    info.require(spacelens_remote::agent::REQUIRED_FEATURES)?;

    let activate = format!(
      "mkdir -p {version_dir} && mv {staged} {final_binary} && rmdir {staging}",
      version_dir = quote(&layout.version_dir)?,
      staged = quote(&remote_binary)?,
      final_binary = quote(&layout.binary)?,
      staging = quote(staging)?,
    );
    let activated = run_ssh(
      target,
      &ssh_exec(target, &script(&activate)?)?,
      None,
      PROBE_TIMEOUT,
    )?;
    if !activated.success {
      return Err(RemoteError::stage(
        InstallStage::AgentInstall,
        format!("cannot activate the agent: {}", tail(&activated.stderr)),
      ));
    }
    Ok(())
  })();

  if result.is_err() {
    let _ = run_ssh(
      target,
      &ssh_exec(target, &script(&format!("rm -rf {}", quote(staging)?))?)?,
      None,
      PROBE_TIMEOUT,
    );
  }
  result
}

fn ensure_host(
  target: &SshTarget,
  probe: &RemoteProbe,
  host: &HostLayout,
  progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), RemoteError> {
  if host_entry_present(target, &host.entry)? {
    return Ok(());
  }
  let npm = probe
    .npm
    .as_deref()
    .ok_or_else(|| RemoteError::UnsupportedTarget("npm was not found".into()))?;
  progress(InstallProgress::new(
    InstallStage::HostCheck,
    format!("Installing @space-lens/cli@{}", host.version),
  ));
  let command = host_install_script(host, npm)?;
  let output = run_ssh(
    target,
    &ssh_exec(target, &command)?,
    None,
    HOST_INSTALL_TIMEOUT,
  )?;
  if !output.success {
    return Err(RemoteError::HostUnavailable(format!(
      "npm install failed: {}",
      tail(&output.stderr)
    )));
  }
  if !host_entry_present(target, &host.entry)? {
    return Err(RemoteError::HostUnavailable(format!(
      "npm reported success but {} is missing",
      host.entry
    )));
  }
  Ok(())
}

fn agent_healthy(target: &SshTarget, binary: &str) -> Result<bool, RemoteError> {
  let command = format!("exec {} agent-info --json", quote(binary)?);
  let output = run_ssh(
    target,
    &ssh_exec(target, &script(&command)?)?,
    None,
    PROBE_TIMEOUT,
  )?;
  if !output.success {
    return Ok(false);
  }
  match AgentInfo::parse(&String::from_utf8_lossy(&output.stdout)) {
    Ok(info) => Ok(
      info.agent_protocol == AGENT_PROTOCOL
        && info
          .require(spacelens_remote::agent::REQUIRED_FEATURES)
          .is_ok(),
    ),
    Err(_) => Ok(false),
  }
}

fn host_entry_present(target: &SshTarget, entry: &str) -> Result<bool, RemoteError> {
  let command = format!("test -f {}", quote(entry)?);
  let output = run_ssh(
    target,
    &ssh_exec(target, &script(&command)?)?,
    None,
    PROBE_TIMEOUT,
  )?;
  Ok(output.success)
}

fn agent_bytes(
  app: &tauri::AppHandle,
  manifest: &AgentManifest,
  triple: &str,
  progress: &mut dyn FnMut(InstallProgress),
) -> Result<Vec<u8>, RemoteError> {
  let artifact = manifest.agent.targets.get(triple);
  if let Ok(path) = std::env::var("SPACELENS_REMOTE_AGENT") {
    let bytes = std::fs::read(&path)?;
    if let Some(artifact) = artifact {
      manifest.verify_artifact(artifact, &bytes)?;
    }
    return Ok(bytes);
  }
  let artifact = artifact.ok_or_else(|| {
    RemoteError::Manifest(format!(
      "the embedded manifest pins no agent for {triple}; set SPACELENS_REMOTE_AGENT to a local build"
    ))
  })?;

  let cache = app
    .path()
    .app_cache_dir()
    .map_err(|error| RemoteError::HostUnavailable(format!("no app cache directory: {error}")))?
    .join("remote-agents")
    .join(&manifest.agent.version)
    .join(triple);
  // The manifest pins the archive, so the cache holds the archive and the
  // binary is extracted again on every use. Never verify the extracted file
  // against the archive's digest.
  let cached = cache.join("agent.tar.gz");
  if let Ok(bytes) = std::fs::read(&cached) {
    if manifest.verify_artifact(artifact, &bytes).is_ok() {
      return Ok(spacelens_remote::archive::extract_agent(&bytes)?.bytes);
    }
    let _ = std::fs::remove_file(&cached);
  }

  progress(InstallProgress::new(
    InstallStage::Download,
    format!("Downloading the agent for {triple}"),
  ));
  let url = manifest.download_url(artifact);
  let client = http_client(Duration::from_secs(120))?;
  let response = client
    .get(&url)
    .send()
    .map_err(|error| RemoteError::HostUnavailable(format!("agent download failed: {error}")))?;
  if !response.status().is_success() {
    return Err(RemoteError::HostUnavailable(format!(
      "agent download answered HTTP {}",
      response.status().as_u16()
    )));
  }
  let bytes = response
    .bytes()
    .map_err(|error| RemoteError::HostUnavailable(format!("agent download failed: {error}")))?
    .to_vec();

  progress(InstallProgress::new(
    InstallStage::Verify,
    "Verifying the agent checksum",
  ));
  manifest.verify_artifact(artifact, &bytes)?;
  let extracted = spacelens_remote::archive::extract_agent(&bytes)?;
  std::fs::create_dir_all(&cache)?;
  std::fs::write(&cached, &bytes)?;
  Ok(extracted.bytes)
}

fn start_serve(
  target: &SshTarget,
  host: &HostLayout,
  agent_binary: &str,
  node: &str,
  home: &str,
) -> Result<(Child, ServeReadiness), RemoteError> {
  let command = serve_script(host, agent_binary, node, home)?;
  let ssh = ssh_exec(target, &command)?;
  let mut child = spawn(&ssh)?;
  let stdout = child
    .stdout
    .take()
    .ok_or_else(|| RemoteError::HostUnavailable("serve stdout was not piped".into()))?;
  let stderr = child
    .stderr
    .take()
    .ok_or_else(|| RemoteError::HostUnavailable("serve stderr was not piped".into()))?;

  let log = Arc::new(Mutex::new(ServeLog::default()));
  spawn_line_reader(stdout, log.clone(), true);
  spawn_line_reader(stderr, log.clone(), false);

  let deadline = Instant::now() + SERVE_START_TIMEOUT;
  loop {
    {
      let log = log.lock().unwrap();
      if let (Some(line), Some(ticket)) = (log.readiness.clone(), log.ticket.clone()) {
        return Ok((
          child,
          ServeReadiness {
            service_instance_id: line.service_instance_id,
            port: line.port,
            ticket,
          },
        ));
      }
    }
    if let Ok(Some(status)) = child.try_wait() {
      let log = log.lock().unwrap();
      return Err(RemoteError::HostUnavailable(format!(
        "serve exited before readiness ({}): {}",
        status,
        tail(log.stderr.as_bytes())
      )));
    }
    if Instant::now() >= deadline {
      let _ = child.kill();
      let _ = child.wait();
      let log = log.lock().unwrap();
      return Err(RemoteError::HostUnavailable(format!(
        "serve did not become ready within {}s: {}",
        SERVE_START_TIMEOUT.as_secs(),
        tail(log.stderr.as_bytes())
      )));
    }
    thread::sleep(Duration::from_millis(100));
  }
}

fn start_tunnel(target: &SshTarget, remote_port: u16) -> Result<(Child, u16), RemoteError> {
  let mut last_error = String::new();
  for _ in 0..2 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
      .map_err(|error| RemoteError::HostUnavailable(format!("no local port: {error}")))?;
    let local_port = listener
      .local_addr()
      .map_err(|error| RemoteError::HostUnavailable(format!("no local port: {error}")))?
      .port();
    drop(listener);

    let ssh = ssh_tunnel(target, local_port, remote_port)?;
    let mut child = spawn(&ssh)?;
    // Drain stderr so a failed forward can be reported and never blocks.
    if let Some(stderr) = child.stderr.take() {
      thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
          line.clear();
        }
      });
    }

    let client = http_client(Duration::from_secs(2))?;
    let deadline = Instant::now() + TUNNEL_TIMEOUT;
    loop {
      if client
        .get(format!("http://127.0.0.1:{local_port}/health"))
        .send()
        .is_ok_and(|response| response.status().is_success())
      {
        return Ok((child, local_port));
      }
      if let Ok(Some(status)) = child.try_wait() {
        last_error = format!("tunnel exited with {status}");
        break;
      }
      if Instant::now() >= deadline {
        let _ = child.kill();
        let _ = child.wait();
        last_error = "tunnel did not answer /health".to_string();
        break;
      }
      thread::sleep(Duration::from_millis(150));
    }
  }
  Err(RemoteError::stage_retryable(
    InstallStage::Tunnel,
    if last_error.is_empty() {
      "tunnel failed".to_string()
    } else {
      last_error
    },
    true,
  ))
}

fn pair(
  client: &reqwest::blocking::Client,
  base_url: &str,
  ticket: &str,
) -> Result<String, RemoteError> {
  let response = client
    .post(format!("{base_url}/api/v1/session/exchange"))
    .json(&json!({ "ticket": ticket }))
    .send()
    .map_err(|error| RemoteError::HostUnavailable(format!("pairing failed: {error}")))?;
  let status = response.status();
  let value: Value = response.json().unwrap_or(Value::Null);
  if !status.is_success() {
    return Err(RemoteError::HostUnavailable(format!(
      "pairing was refused (HTTP {}): {}",
      status.as_u16(),
      value
        .get("problem")
        .and_then(|problem| problem.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("no message")
    )));
  }
  value
    .get("token")
    .and_then(Value::as_str)
    .map(str::to_string)
    .ok_or_else(|| RemoteError::HostUnavailable("pairing returned no token".into()))
}

fn fetch_capabilities(
  client: &reqwest::blocking::Client,
  base_url: &str,
  token: &str,
) -> Result<Value, RemoteError> {
  let response = client
    .get(format!("{base_url}/api/v1/capabilities"))
    .bearer_auth(token)
    .send()
    .map_err(|error| RemoteError::HostUnavailable(format!("capabilities failed: {error}")))?;
  let status = response.status();
  let value: Value = response.json().unwrap_or(Value::Null);
  if !status.is_success() {
    return Err(RemoteError::HostUnavailable(format!(
      "capabilities answered HTTP {}",
      status.as_u16()
    )));
  }
  if value.get("apiMajor").and_then(Value::as_u64) != Some(1) {
    return Err(RemoteError::HostUnavailable(format!(
      "remote host speaks API {}, this app speaks API 1",
      value
        .get("apiMajor")
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".into())
    )));
  }
  Ok(value)
}

/// The desktop's own policy is stricter than the remote host's flags: even a
/// misreported capability cannot advertise cleanup, app scans, reveal, or
/// process termination for a remote device.
fn remote_capabilities(mut value: Value, platform: &str) -> Value {
  if let Some(cleanup) = value.get_mut("cleanup").and_then(Value::as_object_mut) {
    cleanup.insert("plan".into(), Value::Bool(false));
    cleanup.insert("execute".into(), Value::Bool(false));
    cleanup.insert("mode".into(), Value::String("none".into()));
    cleanup.insert("appPlan".into(), Value::Bool(false));
    cleanup.insert("appExecute".into(), Value::Bool(false));
  }
  if let Some(scan) = value.get_mut("scan").and_then(Value::as_object_mut) {
    scan.insert("appScan".into(), Value::Bool(false));
    if platform != "macos" {
      scan.insert("appAudit".into(), Value::Bool(false));
    }
  }
  if let Some(host) = value.get_mut("host").and_then(Value::as_object_mut) {
    host.insert("folderPicker".into(), Value::Bool(false));
    host.insert("pathReveal".into(), Value::Bool(false));
    host.insert("appLocationReveal".into(), Value::Bool(false));
  }
  if let Some(memory) = value.get_mut("memory").and_then(Value::as_object_mut) {
    memory.insert("terminate".into(), Value::Bool(false));
    memory.insert("reopen".into(), Value::Bool(false));
  }
  value
}

// ------------------------------------------------------------------- plumbing

#[derive(Default)]
struct ServeLog {
  readiness: Option<ReadinessLine>,
  stderr: String,
  ticket: Option<String>,
}

fn spawn_line_reader<R: Read + Send + 'static>(reader: R, log: Arc<Mutex<ServeLog>>, stdout: bool) {
  thread::spawn(move || {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
      line.clear();
      match reader.read_line(&mut line) {
        Ok(0) | Err(_) => break,
        Ok(_) => {}
      }
      let trimmed = line.trim_end_matches(['\r', '\n']);
      let mut log = log.lock().unwrap();
      if stdout {
        if log.readiness.is_none() {
          log.readiness = parse_readiness_line(trimmed);
        }
      } else {
        if log.ticket.is_none() {
          log.ticket = spacelens_remote::server::parse_ticket(trimmed);
        }
        // Keep a bounded tail for diagnostics; never grow with server uptime.
        if log.stderr.len() < 64 * 1024 {
          log.stderr.push_str(trimmed);
          log.stderr.push('\n');
        }
      }
    }
  });
}

struct Captured {
  success: bool,
  stdout: Vec<u8>,
  stderr: Vec<u8>,
}

fn run_ssh(
  target: &SshTarget,
  command: &SshCommand,
  stdin: Option<Vec<u8>>,
  timeout: Duration,
) -> Result<Captured, RemoteError> {
  let _ = target;
  run_captured(&command.program, &command.args, stdin, timeout)
}

fn run_captured(
  program: &str,
  args: &[String],
  stdin: Option<Vec<u8>>,
  timeout: Duration,
) -> Result<Captured, RemoteError> {
  let mut child = Command::new(program)
    .args(args)
    .stdin(if stdin.is_some() {
      Stdio::piped()
    } else {
      Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|error| RemoteError::HostUnavailable(format!("cannot run {program}: {error}")))?;

  let writer = stdin.map(|bytes| {
    let mut pipe = child.stdin.take().expect("stdin was piped");
    thread::spawn(move || {
      let _ = pipe.write_all(&bytes);
    })
  });
  let stdout = child.stdout.take().expect("stdout was piped");
  let stderr = child.stderr.take().expect("stderr was piped");
  let stdout = thread::spawn(move || read_all(stdout));
  let stderr = thread::spawn(move || read_all(stderr));

  let deadline = Instant::now() + timeout;
  let status = loop {
    if let Ok(Some(status)) = child.try_wait() {
      break status;
    }
    if Instant::now() >= deadline {
      let _ = child.kill();
      let _ = child.wait();
      if let Some(writer) = writer {
        let _ = writer.join();
      }
      let _ = stdout.join();
      let _ = stderr.join();
      return Err(RemoteError::HostUnavailable(format!(
        "{program} timed out after {}s",
        timeout.as_secs()
      )));
    }
    thread::sleep(Duration::from_millis(40));
  };
  if let Some(writer) = writer {
    let _ = writer.join();
  }
  Ok(Captured {
    success: status.success(),
    stdout: stdout.join().unwrap_or_default(),
    stderr: stderr.join().unwrap_or_default(),
  })
}

fn read_all(mut reader: impl Read) -> Vec<u8> {
  let mut out = Vec::new();
  let _ = reader.read_to_end(&mut out);
  out
}

fn spawn(command: &SshCommand) -> Result<Child, RemoteError> {
  Command::new(&command.program)
    .args(&command.args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|error| RemoteError::HostUnavailable(format!("cannot run ssh: {error}")))
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client, RemoteError> {
  let mut builder = reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(10));
  builder = if timeout.is_zero() {
    builder.timeout(None)
  } else {
    builder.timeout(timeout)
  };
  builder
    .build()
    .map_err(|error| RemoteError::HostUnavailable(format!("cannot build HTTP client: {error}")))
}

fn install_crypto_provider() {
  if rustls::crypto::CryptoProvider::get_default().is_none() {
    let _ = rustls::crypto::ring::default_provider().install_default();
  }
}

fn staging_suffix() -> String {
  format!(
    "{}-{}",
    std::process::id(),
    NEXT_GENERATION.fetch_add(1, Ordering::SeqCst)
  )
}

fn script(body: &str) -> Result<String, RemoteError> {
  spacelens_remote::quote::shell_script(body)
}

fn quote(value: &str) -> Result<String, RemoteError> {
  spacelens_remote::quote::quote_posix(value)
}

fn tail(bytes: &[u8]) -> String {
  let text = String::from_utf8_lossy(bytes);
  let trimmed = text.trim();
  if trimmed.len() <= 800 {
    trimmed.to_string()
  } else {
    format!("…{}", &trimmed[trimmed.len() - 800..])
  }
}

fn sleep_unless(stop: &AtomicBool, duration: Duration) {
  let deadline = Instant::now() + duration;
  while Instant::now() < deadline {
    if stop.load(Ordering::SeqCst) {
      return;
    }
    thread::sleep(Duration::from_millis(50));
  }
}

fn forbidden(message: &str) -> Value {
  json!({ "problem": { "code": "Forbidden", "message": message, "retryable": false } })
}

fn internal(error: impl std::fmt::Display) -> Value {
  json!({ "problem": { "code": "InternalError", "message": error.to_string(), "retryable": false } })
}

fn code_for_status(status: u16) -> &'static str {
  match status {
    401 => "Unauthenticated",
    403 => "Forbidden",
    404 => "NotFound",
    409 => "StalePlan",
    429 => "LimitExceeded",
    _ => "InternalError",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn submit(json: Value) -> SubmitRequest {
    serde_json::from_value(json).expect("fixture must match the closed submit union")
  }

  #[test]
  fn only_scan_start_is_allowed_remotely() {
    let allowed = submit(json!({
      "kind": "scanStart",
      "paths": ["/home/alice"],
    }));
    assert_eq!(submit_route(&allowed), Some("/api/v1/scans"));

    let denied = [
      json!({ "kind": "cleanupPlan", "scanId": "scan_12345678", "nodeIds": ["n1"] }),
      json!({ "kind": "cleanupExecute", "planId": "plan_12345678", "confirm": true }),
      json!({ "kind": "appCleanupPlan", "auditId": "audit_12345678", "appId": "app", "paths": ["/a"] }),
      json!({ "kind": "appCleanupExecute", "planId": "plan_12345678", "confirm": true }),
      json!({ "kind": "appScanStart", "auditId": "audit_12345678", "appId": "app" }),
      json!({ "kind": "memoryTerminatePlan", "targets": [], "mode": "graceful", "origin": "manual" }),
      json!({ "kind": "memoryTerminateExecute", "planId": "tplan_0123456789ABCDEF", "confirm": true }),
    ];
    for fixture in denied {
      let request = submit(fixture.clone());
      assert_eq!(submit_route(&request), None, "remote must refuse {fixture}");
    }
  }

  fn full_capabilities() -> Value {
    json!({
      "apiMajor": 1,
      "contractVersion": "1.1.0",
      "scan": { "start": true, "cancel": true, "discovery": true, "appAudit": true, "appScan": true, "maxConcurrent": 1 },
      "cleanup": { "plan": true, "execute": true, "mode": "trash", "appPlan": true, "appExecute": true },
      "host": { "folderPicker": true, "appLocationReveal": true, "pathReveal": true, "platform": "linux" },
      "icloud": "unavailable",
      "memory": { "available": true, "snapshot": true, "history": true, "candidates": true, "audit": true, "terminate": true, "reopen": true, "platform": "linux" }
    })
  }

  #[test]
  fn remote_capabilities_intersect_with_read_only_policy() {
    for platform in ["linux", "macos"] {
      let value = remote_capabilities(full_capabilities(), platform);
      assert_eq!(value["cleanup"]["plan"], false);
      assert_eq!(value["cleanup"]["execute"], false);
      assert_eq!(value["cleanup"]["mode"], "none");
      assert_eq!(value["cleanup"]["appPlan"], false);
      assert_eq!(value["cleanup"]["appExecute"], false);
      assert_eq!(value["scan"]["appScan"], false);
      assert_eq!(value["host"]["folderPicker"], false);
      assert_eq!(value["host"]["pathReveal"], false);
      assert_eq!(value["host"]["appLocationReveal"], false);
      assert_eq!(value["memory"]["terminate"], false);
      assert_eq!(value["memory"]["reopen"], false);
      // Reads that the remote does support stay as the host reported them.
      assert_eq!(value["scan"]["start"], true);
      assert_eq!(value["scan"]["discovery"], true);
    }
    // App audit depends on a macOS remote; the Linux beta hides it.
    assert_eq!(
      remote_capabilities(full_capabilities(), "linux")["scan"]["appAudit"],
      false
    );
    assert_eq!(
      remote_capabilities(full_capabilities(), "macos")["scan"]["appAudit"],
      true
    );
  }

  #[test]
  fn http_statuses_map_to_contract_problem_codes() {
    assert_eq!(code_for_status(401), "Unauthenticated");
    assert_eq!(code_for_status(403), "Forbidden");
    assert_eq!(code_for_status(409), "StalePlan");
    assert_eq!(code_for_status(500), "InternalError");
  }
}
