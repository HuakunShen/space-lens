//! Desktop adapter for the remote device runtime.
//!
//! The WebView never talks to the remote host. It keeps calling the closed
//! `sl_*` commands; this module maps each allowed request onto the host's
//! frozen routes and refuses everything outside that allow-list before a
//! socket is opened. The provisioning pipeline itself lives in
//! `spacelens_remote::runtime` and is shared with the CLI and the MCP adapter.
//!
//! What is desktop-specific here:
//!
//! - the embedded manifest authorizes what shipped in the app bundle;
//! - the agent archive cache lives in the app cache directory;
//! - mutations are limited to starting a scan and trash-only cleanup of a
//!   reviewed plan (`cleanup/plan`, `cleanup/execute`); app-scoped cleanup,
//!   app scans and process termination are never mapped, even if the remote
//!   host would allow them;
//! - capabilities are masked to what the workbench can actually drive;
//! - SSE payloads are re-published through the window's [`EventSink`].

use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};
use spacelens_remote::manifest::AgentManifest;
use spacelens_remote::runtime;
use spacelens_remote::{RemoteError, RemotePolicy};
use tauri::Manager;

use crate::commands::{ReadRequest, SubmitRequest};
use crate::events::EventSink;

pub use spacelens_remote::runtime::RemoteHost;

/// Compiled into the app: an install is authorized by what shipped, never by
/// what a server says today. Regenerate with `scripts/agent-manifest.ts`.
const MANIFEST_JSON: &str = include_str!("../../../../crates/remote/agents/manifest.json");

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(600);

/// Full provisioning + connection with the app-owned inputs: the embedded
/// manifest and the app cache directory for agent archives.
pub fn connect(
  app: &tauri::AppHandle,
  target: spacelens_remote::SshTarget,
  progress: &mut dyn FnMut(spacelens_remote::InstallProgress),
) -> Result<Arc<RemoteHost>, RemoteError> {
  let manifest = AgentManifest::parse(MANIFEST_JSON)?;
  let cache_root = app
    .path()
    .app_cache_dir()
    .map_err(|error| RemoteError::HostUnavailable(format!("no app cache directory: {error}")))?
    .join("remote-agents");
  let mut source =
    |manifest: &AgentManifest,
     triple: &str,
     progress: &mut dyn FnMut(spacelens_remote::InstallProgress)| {
      let cache = cache_root.join(&manifest.agent.version).join(triple);
      runtime::download_agent(manifest, triple, Some(&cache), progress)
    };
  RemoteHost::connect_with(
    target,
    &manifest,
    // Trash-only cleanup over SSH: the host refuses permanent deletion by
    // construction, and this grants nothing else (no app cleanup, no process
    // termination — see `submit_route`). The user still confirms a plan in
    // the workbench before anything moves.
    RemotePolicy { allow_cleanup: true },
    &mut source,
    progress,
  )
}

/// Forwards remote SSE payloads into this session's sink. Repeated calls while
/// a reader is live are no-ops: `sl_connect` runs on every renderer load and
/// must not duplicate events.
pub fn start_events(remote: &Arc<RemoteHost>, sink: EventSink) {
  remote.start_events(Arc::new(move |payload| {
    sink.publish(payload.clone());
  }));
}

// ------------------------------------------------------------- HTTP reads

pub fn read(remote: &Arc<RemoteHost>, request: &ReadRequest) -> Result<Value, Value> {
  match request {
    ReadRequest::Health => remote.call(Method::GET, "/health", None, REQUEST_TIMEOUT),
    ReadRequest::Capabilities => Ok(remote_capabilities(remote.capabilities(), &remote.platform)),
    ReadRequest::Roots => remote.call(Method::GET, "/api/v1/roots", None, REQUEST_TIMEOUT),
    ReadRequest::Volumes => remote.call(Method::GET, "/api/v1/volumes", None, REQUEST_TIMEOUT),
    ReadRequest::FsList { path } => remote.call(
      Method::POST,
      "/api/v1/fs/list",
      Some(match path {
        Some(path) => json!({ "path": path }),
        None => json!({}),
      }),
      REQUEST_TIMEOUT,
    ),
    ReadRequest::ScanStatus { scan_id } => remote.call(
      Method::GET,
      &format!("/api/v1/scans/{scan_id}"),
      None,
      REQUEST_TIMEOUT,
    ),
    ReadRequest::ScanCancel { scan_id } => remote.call(
      Method::POST,
      &format!("/api/v1/scans/{scan_id}/cancel"),
      Some(json!({})),
      REQUEST_TIMEOUT,
    ),
    ReadRequest::ScanList => {
      let value = remote.call(Method::GET, "/api/v1/scans", None, REQUEST_TIMEOUT)?;
      Ok(value.get("scans").cloned().unwrap_or_else(|| json!([])))
    }
    ReadRequest::TreeSlice {
      scan_id,
      node_id,
      depth,
      max_children_per_node,
    } => remote.call(
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
    } => remote.call(
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
    } => remote.call(
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
    } => remote.call(
      Method::POST,
      "/api/v1/apps/audit",
      Some(json!({
        "minSize": min_size,
        "includeHeuristic": include_heuristic,
        "limit": limit,
      })),
      DISCOVERY_TIMEOUT,
    ),
    ReadRequest::AppIcon { app_path } => remote.call(
      Method::POST,
      "/api/v1/apps/icon",
      Some(json!({ "appPath": app_path })),
      DISCOVERY_TIMEOUT,
    ),
    ReadRequest::MemorySnapshot => remote.call(
      Method::GET,
      "/api/v1/memory/snapshot",
      None,
      REQUEST_TIMEOUT,
    ),
    ReadRequest::MemoryHistory { group_ids } => remote.call(
      Method::POST,
      "/api/v1/memory/history",
      Some(json!({ "groupIds": group_ids })),
      REQUEST_TIMEOUT,
    ),
    ReadRequest::MemoryCandidates { exclude } => remote.call(
      Method::POST,
      "/api/v1/memory/candidates",
      Some(json!({ "exclude": exclude })),
      REQUEST_TIMEOUT,
    ),
    ReadRequest::MemoryAudit => {
      remote.call(Method::GET, "/api/v1/memory/audit", None, REQUEST_TIMEOUT)
    }
  }
}

/// The remote mutation allow-list: a scan, and trash-only cleanup of a
/// reviewed plan. App-scoped cleanup, app scans and process termination stay
/// absent by construction — the host may serve them, this adapter does not
/// map them.
pub fn submit(remote: &Arc<RemoteHost>, request: &SubmitRequest) -> Result<Value, Value> {
  let Some(path) = submit_route(request) else {
    return Err(forbidden(
      "app cleanup and process termination are disabled for remote hosts",
    ));
  };
  // The wire shape the host parses, spelled here rather than derived: the
  // engine types are deserialize-only because they never leave the desktop.
  let body = match request {
    SubmitRequest::ScanStart { request } => serde_json::to_value(request).map_err(internal)?,
    SubmitRequest::CleanupPlan { request } => json!({
      "scanId": request.scan_id,
      "nodeIds": request.node_ids,
    }),
    SubmitRequest::CleanupExecute { request } => json!({
      "planId": request.plan_id,
      "confirm": request.confirm,
    }),
    _ => {
      return Err(forbidden(
        "app cleanup and process termination are disabled for remote hosts",
      ))
    }
  };
  remote.call(Method::POST, path, Some(body), DISCOVERY_TIMEOUT)
}

pub fn submit_route(request: &SubmitRequest) -> Option<&'static str> {
  match request {
    SubmitRequest::ScanStart { .. } => Some("/api/v1/scans"),
    SubmitRequest::CleanupPlan { .. } => Some("/api/v1/cleanup/plan"),
    SubmitRequest::CleanupExecute { .. } => Some("/api/v1/cleanup/execute"),
    SubmitRequest::AppCleanupPlan { .. }
    | SubmitRequest::AppCleanupExecute { .. }
    | SubmitRequest::AppScanStart { .. }
    | SubmitRequest::MemoryTerminatePlan { .. }
    | SubmitRequest::MemoryTerminateExecute { .. } => None,
  }
}

/// The desktop's own policy is stricter than the remote host's flags: app
/// cleanup, app scans, reveal and process termination stay hidden whatever
/// the host advertises. Plain trash-only cleanup passes through as the host
/// reports it, so a host served read-only still shows no cleanup here.
fn remote_capabilities(mut value: Value, platform: &str) -> Value {
  if let Some(cleanup) = value.get_mut("cleanup").and_then(Value::as_object_mut) {
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

fn forbidden(message: &str) -> Value {
  json!({ "problem": { "code": "Forbidden", "message": message, "retryable": false } })
}

fn internal(error: impl std::fmt::Display) -> Value {
  json!({ "problem": { "code": "InternalError", "message": error.to_string(), "retryable": false } })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn submit(json: Value) -> SubmitRequest {
    serde_json::from_value(json).expect("fixture must match the closed submit union")
  }

  #[test]
  fn scan_start_and_trash_only_cleanup_are_allowed_remotely() {
    let scan = submit(json!({
      "kind": "scanStart",
      "paths": ["/home/alice"],
    }));
    assert_eq!(submit_route(&scan), Some("/api/v1/scans"));

    let plan = submit(json!({
      "kind": "cleanupPlan",
      "scanId": "scan_12345678",
      "nodeIds": ["n1"],
    }));
    assert_eq!(submit_route(&plan), Some("/api/v1/cleanup/plan"));

    let execute = submit(json!({
      "kind": "cleanupExecute",
      "planId": "plan_12345678",
      "confirm": true,
    }));
    assert_eq!(submit_route(&execute), Some("/api/v1/cleanup/execute"));

    // Everything the desktop does not map stays absent: app-scoped cleanup,
    // app scans and process termination never reach a remote host.
    let denied = [
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
  fn remote_capabilities_keep_host_cleanup_and_mask_the_rest() {
    for platform in ["linux", "macos"] {
      let value = remote_capabilities(full_capabilities(), platform);
      // Trash-only cleanup is the host's answer: a host served with the flag
      // advertises it, and the workbench then offers plan/execute.
      assert_eq!(value["cleanup"]["plan"], true);
      assert_eq!(value["cleanup"]["execute"], true);
      assert_eq!(value["cleanup"]["mode"], "trash");
      // App-scoped cleanup has no remote mapping, so it stays hidden even
      // when the host advertises it.
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
    // A host that serves read-only keeps advertising no cleanup.
    let read_only = json!({
      "apiMajor": 1,
      "contractVersion": "1.1.0",
      "scan": { "start": true, "cancel": true, "maxConcurrent": 1 },
      "cleanup": { "plan": false, "execute": false, "mode": "none" },
      "host": { "folderPicker": false, "platform": "linux" },
      "icloud": "unavailable"
    });
    let masked = remote_capabilities(read_only, "linux");
    assert_eq!(masked["cleanup"]["plan"], false);
    assert_eq!(masked["cleanup"]["execute"], false);
    assert_eq!(masked["cleanup"]["mode"], "none");
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
}
