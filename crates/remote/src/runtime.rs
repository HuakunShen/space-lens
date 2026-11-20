//! The effectful remote runtime: provision over SSH, start `spacelens serve`
//! on the remote loopback, reach it through an SSH local forward, and speak
//! the existing host HTTP API as a native client.
//!
//! This module is the one place the crate spawns processes and opens sockets.
//! Shells (desktop, CLI, MCP) inject the two local inputs that differ per
//! host: the agent bytes ([`AgentSource`]) and the event forwarder
//! ([`EventForwarder`]). Everything else — the exact `ssh` argv, the POSIX
//! scripts, the readiness and pairing handshakes — is owned here once.
//!
//! Trust boundary:
//!
//! - the user's OpenSSH is the only transport; no password, key, or fingerprint
//!   is handled here;
//! - the agent and host versions are pinned by the caller-supplied
//!   [`AgentManifest`], and downloads are verified against its digests;
//! - the host is started without `--allow-process-control`, and cleanup only
//!   with an explicit [`RemotePolicy`];
//! - the remote server binds loopback and is reached only through
//!   `127.0.0.1:<local>` on this machine.
//!
//! Effects are injected where a test needs them: [`AgentSource`] supplies the
//! agent bytes, and the SSH executable comes from `PATH`, so the integration
//! test runs the same production path against a fake `ssh`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};

use crate::agent::{AgentInfo, AGENT_PROTOCOL};
use crate::archive;
use crate::install::{agent_binary_name, AgentLayout, HostLayout, InstallProgress, InstallStage};
use crate::manifest::AgentManifest;
use crate::probe::{parse_probe, resolve_target, RemoteProbe};
use crate::server::{
  activate_agent_script, agent_health_script, agent_smoke_script, byte_count_script,
  file_exists_script, host_install_script, host_install_script_pwsh, host_install_tarball_script,
  host_install_tarball_script_pwsh, kill_script, kill_script_pwsh, mkdir_script,
  parse_readiness_line, remove_file_script, remove_tree_script, serve_script, serve_script_pwsh,
  ssh_exec_shell, ssh_probe_shell, ssh_transfer_shell, ssh_tunnel, staging_dir_script,
  ReadinessLine, RemotePolicy, RemoteShell, ServeReadiness, SshCommand,
};
use crate::target::SshTarget;
use crate::RemoteError;

const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
const AGENT_TRANSFER_TIMEOUT: Duration = Duration::from_secs(180);
const HOST_INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
const SERVE_START_TIMEOUT: Duration = Duration::from_secs(300);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(600);

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Where forwarded remote events go. The desktop wraps its window sink; a
/// headless client logs, collects, or ignores them.
pub type EventForwarder = Arc<dyn Fn(&Value) + Send + Sync>;

/// Produces the agent binary for one release target. The caller downloads and
/// verifies the pinned archive ([`download_agent`] is one implementation);
/// tests hand in a fixture and exercise the same provisioning path without
/// the network.
pub type AgentSource<'a> = &'a mut dyn FnMut(
  &AgentManifest,
  &str,
  &mut dyn FnMut(InstallProgress),
) -> Result<Vec<u8>, RemoteError>;

/// A scan request in the host's wire shape ([contract `ScanStartRequest`]).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStart {
  pub paths: Vec<String>,
  pub ignore_hidden: bool,
  pub respect_gitignore: bool,
  pub ignored_mode: IgnoredMode,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub label: Option<String>,
  #[serde(skip_serializing_if = "std::ops::Not::not")]
  pub local_only: bool,
}

impl Default for ScanStart {
  fn default() -> Self {
    Self {
      paths: Vec::new(),
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: IgnoredMode::Summarize,
      label: None,
      local_only: false,
    }
  }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IgnoredMode {
  #[default]
  Summarize,
  Exclude,
}

/// A connected remote host. One per `deviceId`; hosts with a session concept
/// (the desktop workbench, a long-lived MCP process) keep it alive, one-shot
/// clients call [`RemoteHost::stop`] when their command finishes.
pub struct RemoteHost {
  pub device_id: String,
  pub label: String,
  pub generation: u64,
  pub platform: String,
  pub service_instance_id: String,
  pub target: SshTarget,
  pub policy: RemotePolicy,
  /// The authority the host answers for (`127.0.0.1:<remote port>`). The
  /// tunnel exposes it under a *local* port, so every request must carry the
  /// host's own authority in its `Host` header or be refused.
  authority: String,
  pid_file: String,
  /// The command channel this host speaks; the stop path needs it long after
  /// provisioning, so it is part of the connection, not a parameter.
  shell: RemoteShell,
  base_url: String,
  token: Mutex<String>,
  client: reqwest::blocking::Client,
  stream_client: reqwest::blocking::Client,
  capabilities: Value,
  tunnel: Mutex<Option<Child>>,
  server: Mutex<Option<Child>>,
  sse_stop: Arc<AtomicBool>,
  sse_running: Arc<AtomicBool>,
  /// The last remote event sequence the forwarder accepted, so a reconnect
  /// resumes instead of replaying the host's whole event ring.
  sse_last: Arc<AtomicU64>,
  sse: Mutex<Option<thread::JoinHandle<()>>>,
  shutdown: AtomicBool,
}

impl RemoteHost {
  /// Full provisioning + connection. Long-running and blocking; never call it
  /// from a UI thread. `manifest` pins both artifacts; `policy` is the
  /// client's half of the mutation contract; `source` supplies the agent
  /// bytes for the resolved target.
  pub fn connect_with(
    target: SshTarget,
    manifest: &AgentManifest,
    policy: RemotePolicy,
    source: AgentSource<'_>,
    progress: &mut dyn FnMut(InstallProgress),
  ) -> Result<Arc<Self>, RemoteError> {
    target.validate()?;
    install_crypto_provider();

    // 1. Probe: fixed script, no user value interpolated. The POSIX probe is
    // the first and only attempt for every POSIX host; a Windows host cannot
    // parse it at all (its default shell is cmd.exe), so the PowerShell probe
    // is the fallback that discovers it.
    progress(InstallProgress::new(
      InstallStage::Probe,
      format!("Checking {target} over SSH"),
    ));
    let probe = probe_remote(&target)?;
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
    let platform = probe.platform();
    if !matches!(platform, "linux" | "macos" | "windows") {
      return Err(RemoteError::UnsupportedTarget(format!(
        "remote {} hosts are not supported yet",
        platform
      )));
    }
    // One command channel per platform, chosen from the probe's own answer
    // and threaded through every command below.
    let shell = RemoteShell::for_platform(platform)?;
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
    let agent = AgentLayout::for_shell(
      &probe.home,
      &manifest.agent.version,
      &staging_suffix(),
      shell,
    )?;
    ensure_agent(
      source,
      &target,
      manifest,
      resolved.triple,
      &agent,
      shell,
      progress,
    )?;

    // 4. Install or verify the pinned host package.
    let host = HostLayout::for_shell(&probe.home, &manifest.host.version, shell)?;
    ensure_host(&target, &probe, &host, shell, progress)?;

    // 5. Start `spacelens serve --machine`; readiness on stdout, ticket on stderr.
    progress(InstallProgress::new(
      InstallStage::ServeStart,
      "Starting the remote host",
    ));
    // A previous client run can leave a server behind (the pid file outlives
    // the tunnel). Stop it first so provisioning does not stack orphaned hosts.
    if let Ok(body) = kill_body(&host.pid_file, shell) {
      if let Ok(command) = ssh_exec_shell(&target, &body, shell) {
        let _ = run_ssh(&target, &command, None, Duration::from_secs(10));
      }
    }
    let node = probe
      .node
      .as_deref()
      .ok_or_else(|| RemoteError::UnsupportedTarget("node was not found".into()))?;
    let (mut server, readiness) = start_serve(
      &target,
      &host,
      &agent.binary,
      node,
      &probe.home,
      &policy,
      shell,
    )?;

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
    // exactly what a `--machine` ticket is bound to. Every request also
    // carries the host's own authority: the tunnel remaps the port, but the
    // host's Host-header gate does not.
    let authority = format!("127.0.0.1:{}", readiness.port);
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
    let token = match pair(&client, &base_url, &readiness.ticket, &authority) {
      Ok(token) => token,
      Err(error) => {
        let _ = tunnel.kill();
        let _ = tunnel.wait();
        let _ = server.kill();
        let _ = server.wait();
        return Err(error);
      }
    };

    let capabilities = match fetch_capabilities(&client, &base_url, &token, &authority) {
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
      platform: platform.to_string(),
      service_instance_id: readiness.service_instance_id.clone(),
      target,
      policy,
      authority,
      pid_file: host.pid_file.clone(),
      shell,
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

  /// The capabilities the host reported at pair time.
  pub fn capabilities(&self) -> Value {
    self.capabilities.clone()
  }

  /// Starts forwarding remote SSE payloads into `forwarder`. Repeated calls
  /// while a reader is live are no-ops: a host with a renderer must not
  /// duplicate events on every reload.
  pub fn start_events(&self, forwarder: EventForwarder) {
    if self.sse_running.swap(true, Ordering::SeqCst) {
      return;
    }
    // A reader that exited without being stopped (the tunnel dropped) is
    // already winding down; join it so only one reader owns the forwarder.
    if let Some(handle) = self.sse.lock().unwrap().take() {
      let _ = handle.join();
    }
    self.sse_stop.store(false, Ordering::SeqCst);
    let stop_flag = self.sse_stop.clone();
    let running = self.sse_running.clone();
    let last = self.sse_last.clone();
    let base_url = self.base_url.clone();
    let authority = self.authority.clone();
    let token = self.token.lock().unwrap().clone();
    let client = self.stream_client.clone();
    let handle = thread::spawn(move || {
      let mut since = last.load(Ordering::SeqCst);
      while !stop_flag.load(Ordering::SeqCst) {
        let response = client
          .get(format!("{base_url}/api/v1/events"))
          .header("host", authority.clone())
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
                    forwarder(payload);
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
    if let Ok(body) = kill_body(&self.pid_file, self.shell) {
      if let Ok(command) = ssh_exec_shell(&self.target, &body, self.shell) {
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

  pub fn health(&self) -> Result<Value, Value> {
    self.call(Method::GET, "/health", None, REQUEST_TIMEOUT)
  }

  pub fn roots(&self) -> Result<Value, Value> {
    self.call(Method::GET, "/api/v1/roots", None, REQUEST_TIMEOUT)
  }

  pub fn scan_start(&self, request: &ScanStart) -> Result<Value, Value> {
    let body = serde_json::to_value(request).map_err(internal)?;
    self.call(Method::POST, "/api/v1/scans", Some(body), DISCOVERY_TIMEOUT)
  }

  pub fn scan_status(&self, scan_id: &str) -> Result<Value, Value> {
    self.call(
      Method::GET,
      &format!("/api/v1/scans/{scan_id}"),
      None,
      REQUEST_TIMEOUT,
    )
  }

  pub fn scan_cancel(&self, scan_id: &str) -> Result<Value, Value> {
    self.call(
      Method::POST,
      &format!("/api/v1/scans/{scan_id}/cancel"),
      Some(json!({})),
      REQUEST_TIMEOUT,
    )
  }

  pub fn scan_list(&self) -> Result<Value, Value> {
    let value = self.call(Method::GET, "/api/v1/scans", None, REQUEST_TIMEOUT)?;
    Ok(value.get("scans").cloned().unwrap_or_else(|| json!([])))
  }

  pub fn tree_slice(
    &self,
    scan_id: &str,
    node_id: &str,
    depth: u32,
    max_children_per_node: usize,
  ) -> Result<Value, Value> {
    self.call(
      Method::POST,
      "/api/v1/tree/slice",
      Some(json!({
        "scanId": scan_id,
        "nodeId": node_id,
        "depth": depth,
        "maxChildrenPerNode": max_children_per_node,
      })),
      REQUEST_TIMEOUT,
    )
  }

  pub fn tree_children(
    &self,
    scan_id: &str,
    node_id: &str,
    offset: usize,
    limit: usize,
    sort: &str,
  ) -> Result<Value, Value> {
    self.call(
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
    )
  }

  pub fn discovery(
    &self,
    scan_id: &str,
    kind: &str,
    min_size: u64,
    offset: usize,
    limit: usize,
  ) -> Result<Value, Value> {
    self.call(
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
    )
  }

  pub fn app_audit(
    &self,
    min_size: u64,
    include_heuristic: bool,
    limit: usize,
  ) -> Result<Value, Value> {
    self.call(
      Method::POST,
      "/api/v1/apps/audit",
      Some(json!({
        "minSize": min_size,
        "includeHeuristic": include_heuristic,
        "limit": limit,
      })),
      DISCOVERY_TIMEOUT,
    )
  }

  pub fn memory_snapshot(&self) -> Result<Value, Value> {
    self.call(
      Method::GET,
      "/api/v1/memory/snapshot",
      None,
      REQUEST_TIMEOUT,
    )
  }

  // ------------------------------------------------------------ HTTP writes

  /// Plans a trash-only cleanup. Denied locally — before any socket — unless
  /// the session's policy grants cleanup.
  pub fn cleanup_plan(&self, scan_id: &str, node_ids: &[String]) -> Result<Value, Value> {
    if let Some(problem) = denied_cleanup(&self.policy) {
      return Err(problem);
    }
    self.call(
      Method::POST,
      "/api/v1/cleanup/plan",
      Some(json!({
        "scanId": scan_id,
        "nodeIds": node_ids,
      })),
      REQUEST_TIMEOUT,
    )
  }

  /// Executes a plan by id, with the contract's explicit confirm.
  pub fn cleanup_execute(&self, plan_id: &str) -> Result<Value, Value> {
    if let Some(problem) = denied_cleanup(&self.policy) {
      return Err(problem);
    }
    self.call(
      Method::POST,
      "/api/v1/cleanup/execute",
      Some(json!({ "planId": plan_id, "confirm": true })),
      DISCOVERY_TIMEOUT,
    )
  }

  /// One authenticated host request. Public so shells can map their own
  /// request unions onto the frozen routes without going through the typed
  /// helpers above.
  pub fn call(
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
      .header("host", self.authority.clone())
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

impl Drop for RemoteHost {
  fn drop(&mut self) {
    // Never open a network connection just because a value was dropped, and
    // never block on a reader whose socket is still open.
    self.signal_events_stop();
    self.stop_local_children();
  }
}

// ------------------------------------------------------------- provisioning

/// Runs the fixed probe script and returns the parsed answer. This is the
/// diagnostic surface for CLI shells; [`RemoteHost::connect_with`] performs
/// the same probe as its first stage. Windows hosts are discovered here too:
/// their cmd.exe cannot parse the POSIX script, so the PowerShell probe
/// answers instead.
pub fn probe(target: &SshTarget) -> Result<RemoteProbe, RemoteError> {
  target.validate()?;
  probe_remote(target)
}

/// Probes a target over the platform's command channel, in order.
///
/// The POSIX probe is the first and only attempt for every POSIX host — its
/// argv and failure text are exactly what they have always been. Only when
/// that probe cannot run does the PowerShell probe answer (a Windows host's
/// cmd.exe rejects the POSIX script outright), and only when both fail is
/// the original error reported, so an unreachable POSIX host keeps its own
/// diagnostics.
fn probe_remote(target: &SshTarget) -> Result<RemoteProbe, RemoteError> {
  let posix = try_probe(target, RemoteShell::Posix);
  if posix.is_ok() {
    return posix;
  }
  match try_probe(target, RemoteShell::WindowsPwsh) {
    Ok(probe) => Ok(probe),
    Err(_) => posix,
  }
}

fn try_probe(target: &SshTarget, shell: RemoteShell) -> Result<RemoteProbe, RemoteError> {
  let output = run_ssh(
    target,
    &ssh_probe_shell(target, shell)?,
    None,
    PROBE_TIMEOUT,
  )?;
  if !output.success {
    return Err(RemoteError::Probe(format!(
      "ssh probe failed: {}",
      tail(&output.stderr)
    )));
  }
  parse_probe(&String::from_utf8_lossy(&output.stdout))
    .map_err(|error| RemoteError::Probe(error.to_string()))
}

/// Picks the stop script for one platform's pid file.
fn kill_body(pid_file: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  match shell {
    RemoteShell::Posix => kill_script(pid_file),
    RemoteShell::WindowsPwsh => kill_script_pwsh(pid_file),
  }
}

fn ensure_agent(
  source: AgentSource<'_>,
  target: &SshTarget,
  manifest: &AgentManifest,
  triple: &str,
  layout: &AgentLayout,
  shell: RemoteShell,
  progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), RemoteError> {
  if agent_healthy(target, &layout.binary, shell)? {
    return Ok(());
  }
  let bytes = source(manifest, triple, progress)?;
  progress(InstallProgress::new(
    InstallStage::AgentInstall,
    format!("Installing the agent under {}", layout.version_dir),
  ));

  let staging = &layout.staging_dir;
  let created = run_ssh(
    target,
    &ssh_exec_shell(target, &staging_dir_script(staging, shell)?, shell)?,
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
    let remote_binary = format!("{staging}/{}", agent_binary_name(shell));
    let transfer = run_ssh(
      target,
      &ssh_transfer_shell(target, &remote_binary, shell)?,
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
      &ssh_exec_shell(target, &byte_count_script(&remote_binary, shell)?, shell)?,
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

    let smoke = run_ssh(
      target,
      &ssh_exec_shell(target, &agent_smoke_script(&remote_binary, shell)?, shell)?,
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
    info.require(crate::agent::REQUIRED_FEATURES)?;

    let activated = run_ssh(
      target,
      &ssh_exec_shell(
        target,
        &activate_agent_script(
          &layout.version_dir,
          &remote_binary,
          &layout.binary,
          staging,
          shell,
        )?,
        shell,
      )?,
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
      &ssh_exec_shell(target, &remove_tree_script(staging, shell)?, shell)?,
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
  shell: RemoteShell,
  progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), RemoteError> {
  if host_entry_present(target, &host.entry, shell)? {
    return Ok(());
  }
  let npm = probe
    .npm
    .as_deref()
    .ok_or_else(|| RemoteError::UnsupportedTarget("npm was not found".into()))?;
  // npm's shebang resolves `node` from PATH, which a non-interactive SSH
  // session does not have when node lives in nvm or a user-local tree.
  let node_dir = probe
    .node
    .as_deref()
    .and_then(|node| Path::new(node).parent())
    .map(|dir| dir.to_string_lossy().into_owned())
    .ok_or_else(|| RemoteError::UnsupportedTarget("node was not found".into()))?;
  shell.validate_path(&node_dir, "node directory")?;

  // Development and acceptance testing can point the client at a local
  // `npm pack` tarball, so a remote can be exercised before the pinned
  // package version is published. The tarball travels over the same SSH
  // session; nothing here trusts a file the remote already had.
  if let Ok(tarball) = std::env::var("SPACELENS_REMOTE_HOST_TARBALL") {
    if !tarball.is_empty() {
      progress(InstallProgress::new(
        InstallStage::HostCheck,
        format!(
          "Installing the host package from {}",
          Path::new(&tarball).display()
        ),
      ));
      let bytes = std::fs::read(&tarball)?;
      let remote_tarball = format!(
        "{}/{}/.staging-{}-host.tgz",
        host.home,
        crate::install::HOST_DIR,
        staging_suffix()
      );
      let created = run_ssh(
        target,
        &ssh_exec_shell(
          target,
          &mkdir_script(&parent_of(&remote_tarball, shell)?, shell)?,
          shell,
        )?,
        None,
        PROBE_TIMEOUT,
      )?;
      if !created.success {
        return Err(RemoteError::HostUnavailable(format!(
          "cannot create host staging directory: {}",
          tail(&created.stderr)
        )));
      }
      let transfer = run_ssh(
        target,
        &ssh_transfer_shell(target, &remote_tarball, shell)?,
        Some(bytes),
        AGENT_TRANSFER_TIMEOUT,
      )?;
      if !transfer.success {
        return Err(RemoteError::HostUnavailable(format!(
          "host tarball transfer failed: {}",
          tail(&transfer.stderr)
        )));
      }
      let body = match shell {
        RemoteShell::Posix => host_install_tarball_script(host, npm, &remote_tarball, &node_dir)?,
        RemoteShell::WindowsPwsh => {
          host_install_tarball_script_pwsh(host, npm, &remote_tarball, &node_dir)?
        }
      };
      let output = run_ssh(
        target,
        &ssh_exec_shell(target, &body, shell)?,
        None,
        HOST_INSTALL_TIMEOUT,
      )?;
      let _ = run_ssh(
        target,
        &ssh_exec_shell(target, &remove_file_script(&remote_tarball, shell)?, shell)?,
        None,
        PROBE_TIMEOUT,
      );
      if !output.success {
        return Err(RemoteError::HostUnavailable(format!(
          "npm install failed: {}",
          tail(&output.stderr)
        )));
      }
      if !host_entry_present(target, &host.entry, shell)? {
        return Err(RemoteError::HostUnavailable(format!(
          "npm reported success but {} is missing",
          host.entry
        )));
      }
      return Ok(());
    }
  }

  progress(InstallProgress::new(
    InstallStage::HostCheck,
    format!("Installing @space-lens/cli@{}", host.version),
  ));
  let body = match shell {
    RemoteShell::Posix => host_install_script(host, npm, &node_dir)?,
    RemoteShell::WindowsPwsh => host_install_script_pwsh(host, npm, &node_dir)?,
  };
  let output = run_ssh(
    target,
    &ssh_exec_shell(target, &body, shell)?,
    None,
    HOST_INSTALL_TIMEOUT,
  )?;
  if !output.success {
    return Err(RemoteError::HostUnavailable(format!(
      "npm install failed: {}",
      tail(&output.stderr)
    )));
  }
  if !host_entry_present(target, &host.entry, shell)? {
    return Err(RemoteError::HostUnavailable(format!(
      "npm reported success but {} is missing",
      host.entry
    )));
  }
  Ok(())
}

fn agent_healthy(
  target: &SshTarget,
  binary: &str,
  shell: RemoteShell,
) -> Result<bool, RemoteError> {
  let output = run_ssh(
    target,
    &ssh_exec_shell(target, &agent_health_script(binary, shell)?, shell)?,
    None,
    PROBE_TIMEOUT,
  )?;
  if !output.success {
    return Ok(false);
  }
  match AgentInfo::parse(&String::from_utf8_lossy(&output.stdout)) {
    Ok(info) => Ok(
      info.agent_protocol == AGENT_PROTOCOL
        && info.require(crate::agent::REQUIRED_FEATURES).is_ok(),
    ),
    Err(_) => Ok(false),
  }
}

fn host_entry_present(
  target: &SshTarget,
  entry: &str,
  shell: RemoteShell,
) -> Result<bool, RemoteError> {
  let output = run_ssh(
    target,
    &ssh_exec_shell(target, &file_exists_script(entry, shell)?, shell)?,
    None,
    PROBE_TIMEOUT,
  )?;
  Ok(output.success)
}

/// Downloads (or reuses a cached) pinned agent archive and returns the binary.
///
/// `cache_dir` is where the verified archive is kept between runs; `None`
/// downloads every time. `SPACELENS_REMOTE_AGENT=<path>` bypasses the network
/// and hands provisioning a local build, which is how the pinned manifest is
/// exercised before a release exists.
pub fn download_agent(
  manifest: &AgentManifest,
  triple: &str,
  cache_dir: Option<&Path>,
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
      "the pinned manifest has no agent for {triple}; set SPACELENS_REMOTE_AGENT to a local build"
    ))
  })?;

  // The manifest pins the archive, so the cache holds the archive and the
  // binary is extracted again on every use. Never verify the extracted file
  // against the archive's digest. The cache name's extension follows the
  // artifact, because extraction dispatches on it.
  let cached = cache_dir.map(|dir| dir.join(cache_archive_name(artifact)));
  if let Some(cached) = cached {
    if let Ok(bytes) = std::fs::read(&cached) {
      if manifest.verify_artifact(artifact, &bytes).is_ok() {
        return Ok(archive::extract_agent(&artifact.archive, &bytes)?.bytes);
      }
      let _ = std::fs::remove_file(&cached);
    }
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
  let extracted = archive::extract_agent(&artifact.archive, &bytes)?;
  if let Some(cache_dir) = cache_dir {
    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(cache_dir.join(cache_archive_name(artifact)), &bytes)?;
  }
  Ok(extracted.bytes)
}

/// The cache file name for one artifact: the same bytes, under the
/// extension that names their format.
fn cache_archive_name(artifact: &crate::manifest::AgentArtifact) -> String {
  if artifact.archive.ends_with(".zip") {
    "agent.zip".into()
  } else {
    "agent.tar.gz".into()
  }
}

fn start_serve(
  target: &SshTarget,
  host: &HostLayout,
  agent_binary: &str,
  node: &str,
  home: &str,
  policy: &RemotePolicy,
  shell: RemoteShell,
) -> Result<(Child, ServeReadiness), RemoteError> {
  let body = match shell {
    RemoteShell::Posix => serve_script(host, agent_binary, node, home, policy)?,
    RemoteShell::WindowsPwsh => serve_script_pwsh(host, agent_binary, node, home, policy)?,
  };
  let ssh = ssh_exec_shell(target, &body, shell)?;
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
        // The host's authority gate names its own port; the local forward
        // carries a different one, so say who the request is for.
        .header("host", format!("127.0.0.1:{remote_port}"))
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
  authority: &str,
) -> Result<String, RemoteError> {
  let response = client
    .post(format!("{base_url}/api/v1/session/exchange"))
    .header("host", authority)
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
  authority: &str,
) -> Result<Value, RemoteError> {
  let response = client
    .get(format!("{base_url}/api/v1/capabilities"))
    .header("host", authority)
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
      "remote host speaks API {}, this client speaks API 1",
      value
        .get("apiMajor")
        .map(Value::to_string)
        .unwrap_or_else(|| "unknown".into())
    )));
  }
  Ok(value)
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
          log.ticket = crate::server::parse_ticket(trimmed);
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

fn parent_of(path: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  let parent = Path::new(path)
    .parent()
    .map(|value| value.to_string_lossy().into_owned())
    .ok_or_else(|| {
      RemoteError::InvalidPath(format!("host staging directory has no parent: {path:?}"))
    })?;
  shell.validate_path(&parent, "host staging directory")?;
  Ok(parent)
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

/// The local half of the cleanup policy: refuse before a socket is opened.
fn denied_cleanup(policy: &RemotePolicy) -> Option<Value> {
  if policy.allow_cleanup {
    None
  } else {
    Some(forbidden(
      "cleanup is disabled for this remote session; reconnect with cleanup allowed",
    ))
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

  #[test]
  fn cleanup_is_refused_locally_without_the_policy() {
    let denied = denied_cleanup(&RemotePolicy::read_only()).expect("must deny");
    assert_eq!(denied["problem"]["code"], "Forbidden");
    assert_eq!(denied["problem"]["retryable"], false);

    assert!(
      denied_cleanup(&RemotePolicy {
        allow_cleanup: true
      })
      .is_none(),
      "an allowed policy must not deny"
    );
  }

  #[test]
  fn scan_start_serializes_to_the_contract_shape() {
    let request = ScanStart {
      paths: vec!["/home/alice".into()],
      ..ScanStart::default()
    };
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["paths"][0], "/home/alice");
    assert_eq!(value["respectGitignore"], true);
    assert_eq!(value["ignoredMode"], "summarize");
    assert!(value.get("label").is_none());
    assert!(value.get("localOnly").is_none());

    let explicit = ScanStart {
      label: Some("Home".into()),
      local_only: true,
      ignored_mode: IgnoredMode::Exclude,
      ..ScanStart::default()
    };
    let value = serde_json::to_value(&explicit).unwrap();
    assert_eq!(value["label"], "Home");
    assert_eq!(value["localOnly"], true);
    assert_eq!(value["ignoredMode"], "exclude");
  }

  #[test]
  fn http_statuses_map_to_contract_problem_codes() {
    assert_eq!(code_for_status(401), "Unauthenticated");
    assert_eq!(code_for_status(403), "Forbidden");
    assert_eq!(code_for_status(409), "StalePlan");
    assert_eq!(code_for_status(500), "InternalError");
  }

  #[test]
  fn the_agent_cache_keeps_the_artifacts_extension() {
    let zip = crate::manifest::AgentArtifact {
      archive: "spacelens-x86_64-pc-windows-msvc-v0.4.0.zip".into(),
      sha256: "0".repeat(64),
      size: 4,
    };
    let tar_gz = crate::manifest::AgentArtifact {
      archive: "spacelens-x86_64-unknown-linux-musl-v0.4.0.tar.gz".into(),
      ..zip.clone()
    };
    assert_eq!(cache_archive_name(&zip), "agent.zip");
    assert_eq!(cache_archive_name(&tar_gz), "agent.tar.gz");
  }
}
