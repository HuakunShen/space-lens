//! IPC surface. Closed command set, closed request unions with
//! `deny_unknown_fields`, session bound to the calling window label. The
//! WebView can name ids and intentions only — never a raw operation outside
//! the engine's vocabulary.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::app_cleanup::{
  AppCleanupExecuteRequest, AppCleanupPlanRequest, AppCleanupState, AuditSnapshot,
};
use crate::engine::{
  default_discovery_limit, run_discovery, AppScanStartRequest, ChildrenPageRequest,
  CleanupExecuteRequest, CleanupPlanRequest, DiscoveryData, DiscoveryInput, DiscoveryKind,
  DiscoveryPage, DiscoveryRequest, EngineProblem, EngineStore, LocalScanTask, PreparedDiscovery,
  ScanStartRequest, TreeSliceRequest,
};
use crate::events::{EventSink, ScopedEventFrame, SubscriptionAck};
use crate::memory::MemoryState;
use spacelens_memory::{TerminateExecuteRequest, TerminatePlanRequest};
use spacelens_storage_usage::{storage_breakdown, ScannedRoot};

pub const EVENT_NAME: &str = "spacelens://event";
pub const API_MAJOR: u32 = 1;
pub const CONTRACT_VERSION: &str = "1.0.0";
pub const SERVICE_INSTANCE_ID: &str = "inst_desktop";

/// One connected frontend session: its own engine store and event sink.
pub struct SessionBundle {
  window_label: String,
  pub engine: Mutex<EngineStore>,
  pub events: EventSink,
  // Only discovery requests wait here; ordinary reads never acquire it.
  discovery: Mutex<()>,
  // Only app-audit and app-icon requests wait here; ordinary reads never
  // acquire it.
  pub app_audit: Mutex<AppAuditState>,
  // Only app-cleanup plan/execute requests wait here; ordinary submits never
  // acquire it, and app cleanup never acquires the engine lock.
  pub app_cleanup: Mutex<AppCleanupState>,
}

/// Per-session app-audit state: the icon allow-list plus the icon cache plus
/// the current audit-generation authorization snapshot.
///
/// `known_paths` accumulates the canonical `.app` bundle paths of every audit
/// this session has run; icons are served only for those paths, so a raw IPC
/// caller cannot turn `appIcon` into an arbitrary-file read. `icons` caches
/// successful data URLs by canonical path; icon misses are never cached.
/// `audit` is the latest successful audit's snapshot (id, report, exact
/// per-app locations); cleanup and reveal authorize against it alone, and a
/// failed audit never replaces it.
#[derive(Default)]
pub struct AppAuditState {
  known_paths: std::collections::HashSet<PathBuf>,
  icons: std::collections::HashMap<PathBuf, String>,
  pub audit: Option<AuditSnapshot>,
}

pub struct AppState {
  pub sessions: Mutex<std::collections::HashMap<String, Arc<SessionBundle>>>,
  pub roots: Vec<PathBuf>,
  pub next_session: std::sync::atomic::AtomicU64,
}

impl AppState {
  pub fn new(roots: Vec<PathBuf>) -> Self {
    Self {
      sessions: Mutex::new(std::collections::HashMap::new()),
      roots,
      next_session: std::sync::atomic::AtomicU64::new(1),
    }
  }
}

// ------------------------------------------------------------------- connect

#[tauri::command]
pub fn sl_connect(window: Window, state: State<'_, AppState>) -> Result<Value, Value> {
  Ok(connect_window_session(window.label(), &state))
}

fn connect_window_session(window_label: &str, state: &AppState) -> Value {
  // Serialize lookup + creation so parallel adapter calls and renderer
  // refreshes cannot allocate a second engine for the same native window.
  let mut sessions = state.sessions.lock().unwrap();
  if let Some((session_id, _)) = sessions
    .iter()
    .find(|(_, bundle)| bundle.window_label == window_label)
  {
    return session_metadata(session_id);
  }
  let session_id = format!(
    "sess_{}",
    state
      .next_session
      .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
  );
  let engine = EngineStore::new(state.roots.clone());
  sessions.insert(
    session_id.clone(),
    Arc::new(SessionBundle {
      window_label: window_label.to_string(),
      engine: Mutex::new(engine),
      events: EventSink::new(),
      discovery: Mutex::new(()),
      app_audit: Mutex::new(AppAuditState::default()),
      app_cleanup: Mutex::new(AppCleanupState::default()),
    }),
  );
  session_metadata(&session_id)
}

fn session_metadata(session_id: &str) -> Value {
  json!({
      "sessionId": session_id,
      "serviceInstanceId": SERVICE_INSTANCE_ID,
      "cacheNamespace": format!("native/{SERVICE_INSTANCE_ID}/{session_id}"),
      "backendLabel": "Native host",
  })
}

/// Every declared capability must be truly reachable by the engine.
/// `scan.discovery` derives all three views from protected scan reports and
/// runs the legacy walk for legacy scans; `cleanup.plan`/`execute` are
/// trash-only and re-verified. `maxConcurrent: 1` mirrors the engine's
/// single worker slot — EngineStore rejects a second scan while one is
/// active or leased; the slot semantics are locked by
/// `tests/local_scan.rs::protected_prepare_is_nonblocking_and_cancellation_is_terminal`.
/// `scan.appScan` advertises virtual-root app sessions (`appScanStart`).
/// `memory` is supplied by [`crate::memory::capability`], which mirrors the
/// running service's own greeting rather than a compile-time guess.
fn capabilities_json(memory: Value) -> Value {
  json!({
      "apiMajor": API_MAJOR,
      "contractVersion": CONTRACT_VERSION,
      "scan": { "start": true, "cancel": true, "maxConcurrent": 1, "discovery": true, "appAudit": true, "appScan": true },
      "cleanup": { "plan": true, "execute": true, "mode": "trash", "appPlan": true, "appExecute": true },
      "host": { "folderPicker": true, "appLocationReveal": true, "pathReveal": true, "platform": crate::reveal::host_platform() },
      "icloud": "unavailable",
      "memory": memory,
  })
}

#[tauri::command]
pub fn sl_disconnect(
  window: Window,
  session_id: String,
  state: State<'_, AppState>,
) -> Result<Value, Value> {
  disconnect_window_session(window.label(), &session_id, &state)?;
  Ok(json!({"disconnected": true}))
}

fn require_owner(window: &Window, state: &AppState, session_id: &str) -> Result<(), Value> {
  require_window_owner(window.label(), state, session_id)
}

fn require_window_owner(
  window_label: &str,
  state: &AppState,
  session_id: &str,
) -> Result<(), Value> {
  let owned = state
    .sessions
    .lock()
    .unwrap()
    .get(session_id)
    .is_some_and(|bundle| bundle.window_label == window_label);
  if owned {
    Ok(())
  } else {
    Err(
      json!({"problem": {"code": "Forbidden", "message": "this window does not own that session", "retryable": false}}),
    )
  }
}

fn disconnect_window_session(
  window_label: &str,
  session_id: &str,
  state: &AppState,
) -> Result<(), Value> {
  require_window_owner(window_label, state, session_id)?;
  let bundle = state.sessions.lock().unwrap().get(session_id).cloned();
  if let Some(bundle) = bundle {
    cancel_session_scans(&bundle);
  }
  // Keep the same-window bundle and its active-worker lease. Dropping it
  // here would let a reconnect create another engine before the walk exits.
  Ok(())
}

/// Called by the native window-destroy hook, never by an arbitrary client id.
pub fn disconnect_window(window_label: &str, state: &AppState) {
  let bundles: Vec<Arc<SessionBundle>> = state
    .sessions
    .lock()
    .unwrap()
    .values()
    .filter(|bundle| bundle.window_label == window_label)
    .cloned()
    .collect();
  for bundle in bundles {
    cancel_session_scans(&bundle);
  }
}

fn cancel_session_scans(bundle: &SessionBundle) {
  let mut engine = bundle.engine.lock().unwrap();
  let running: Vec<String> = engine
    .list()
    .into_iter()
    .filter(|status| status.state == "scanning")
    .map(|status| status.scan_id)
    .collect();
  engine.cancel_all_scans();
  for scan_id in running {
    if let Ok(status) = engine.status(&scan_id) {
      let _ = bundle
        .events
        .publish(json!({ "kind": "scan.updated", "status": status }));
    }
  }
}

// --------------------------------------------------------------- closed union

fn default_true() -> bool {
  true
}

fn default_app_audit_limit() -> usize {
  500
}

#[derive(Deserialize)]
#[serde(
  tag = "method",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum ReadRequest {
  Health,
  Capabilities,
  Roots,
  Volumes,
  ScanStatus {
    scan_id: String,
  },
  ScanCancel {
    scan_id: String,
  },
  ScanList,
  TreeSlice {
    scan_id: String,
    node_id: String,
    depth: u32,
    max_children_per_node: usize,
  },
  TreeChildren {
    scan_id: String,
    node_id: String,
    offset: usize,
    limit: usize,
    sort: String,
  },
  Discovery {
    scan_id: String,
    kind: DiscoveryKind,
    #[serde(default)]
    min_size: u64,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_discovery_limit")]
    limit: usize,
  },
  AppAudit {
    #[serde(default)]
    min_size: u64,
    #[serde(default = "default_true")]
    include_heuristic: bool,
    #[serde(default = "default_app_audit_limit")]
    limit: usize,
  },
  AppIcon {
    app_path: String,
  },
  MemorySnapshot,
  MemoryHistory {
    group_ids: Vec<String>,
  },
  MemoryCandidates {
    exclude: Vec<String>,
  },
  MemoryAudit,
}

#[derive(Deserialize)]
#[serde(
  tag = "kind",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum SubmitRequest {
  ScanStart {
    #[serde(flatten)]
    request: ScanStartRequest,
  },
  CleanupPlan {
    #[serde(flatten)]
    request: CleanupPlanRequest,
  },
  CleanupExecute {
    #[serde(flatten)]
    request: CleanupExecuteRequest,
  },
  AppCleanupPlan {
    #[serde(flatten)]
    request: AppCleanupPlanRequest,
  },
  AppCleanupExecute {
    #[serde(flatten)]
    request: AppCleanupExecuteRequest,
  },
  AppScanStart {
    #[serde(flatten)]
    request: AppScanStartRequest,
  },
  MemoryTerminatePlan {
    #[serde(flatten)]
    request: TerminatePlanRequest,
  },
  MemoryTerminateExecute {
    #[serde(flatten)]
    request: TerminateExecuteRequest,
  },
}

#[derive(Deserialize)]
#[serde(
  tag = "kind",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum HostRequest {
  PickDirectory {
    title: Option<String>,
  },
  RevealPath {
    app_id: String,
    path: String,
  },
  RevealScanPath {
    path: String,
    /// `reveal` (default) shows the item in its parent folder, selected;
    /// `open` opens the folder itself.
    #[serde(default)]
    action: Option<crate::reveal::RevealAction>,
  },
  ReopenApp {
    audit_id: String,
  },
}

// -------------------------------------------------------------------- reads

#[tauri::command]
pub async fn sl_read(
  app: AppHandle,
  window: Window,
  session_id: String,
  request: ReadRequest,
  reply: Channel<Value>,
) -> Result<(), Value> {
  // The reply rides a Channel (event delivery). The custom-protocol fetch
  // response body is NOT used: on macOS later fetch responses are lost
  // between the WKURLSchemeHandler and the page (see README known issue).
  let handle = tauri::async_runtime::spawn_blocking(move || {
    let state = app.state::<AppState>();
    let outcome: Result<Value, Value> = (|| {
      require_owner(&window, &state, &session_id)?;
      let sessions = state.sessions.lock().unwrap();
      let bundle = sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}))?;
      drop(sessions);
      if let ReadRequest::Discovery {
        scan_id,
        kind,
        min_size,
        offset,
        limit,
      } = &request
      {
        let discovery_request = DiscoveryRequest {
          scan_id: scan_id.clone(),
          kind: *kind,
          min_size: *min_size,
          offset: *offset,
          limit: *limit,
        };
        let page =
          read_discovery(&bundle, &discovery_request, run_discovery).map_err(problem_to_value)?;
        return serde_json::to_value(page).map_err(|error| problem_value("InternalError", error));
      }
      if let ReadRequest::AppAudit {
        min_size,
        include_heuristic,
        limit,
      } = &request
      {
        return read_app_audit(&bundle, *min_size, *include_heuristic, *limit);
      }
      if let ReadRequest::AppIcon { app_path } = &request {
        return read_app_icon(&bundle, app_path);
      }
      // Memory reads run against the in-process service, never the engine, so
      // they take no engine lock (mirroring appAudit/appIcon above).
      if let ReadRequest::MemorySnapshot = &request {
        let memory = app.state::<MemoryState>();
        return crate::memory::read_snapshot(&memory);
      }
      if let ReadRequest::MemoryHistory { group_ids } = &request {
        let memory = app.state::<MemoryState>();
        return crate::memory::read_history(&memory, group_ids);
      }
      if let ReadRequest::MemoryCandidates { exclude } = &request {
        let memory = app.state::<MemoryState>();
        return crate::memory::read_candidates(&memory, exclude);
      }
      if let ReadRequest::MemoryAudit = &request {
        let memory = app.state::<MemoryState>();
        return crate::memory::read_audit(&memory);
      }
      let mut engine = bundle.engine.lock().unwrap();
      let result: Result<Value, Value> = match request {
        ReadRequest::Health => Ok(json!({
            "status": "ok",
            "serviceInstanceId": SERVICE_INSTANCE_ID,
            "apiMajor": API_MAJOR,
            "contractVersion": CONTRACT_VERSION,
        })),
        ReadRequest::Capabilities => Ok(capabilities_json(crate::memory::capability(
          &app.state::<MemoryState>(),
        ))),
        ReadRequest::Roots => {
          // wrapped to match the HTTP host shape: { roots: ScanTarget[] }
          serde_json::to_value(json!({ "roots": engine.roots() }))
            .map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::Volumes => {
          // Display-only facts for the pre-scan drive list; mount points
          // grant no scan capability by themselves.
          serde_json::to_value(json!({ "volumes": engine.volumes() }))
            .map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::ScanStatus { scan_id } => {
          serde_json::to_value(engine.status(&scan_id).map_err(problem_to_value)?)
            .map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::ScanCancel { scan_id } => {
          let before = engine.status(&scan_id).map_err(problem_to_value)?;
          let status = engine.cancel_scan(&scan_id).map_err(problem_to_value)?;
          if before.state == "scanning" && status.state == "cancelled" {
            let _ = bundle
              .events
              .publish(json!({ "kind": "scan.updated", "status": status }));
          }
          serde_json::to_value(status).map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::ScanList => {
          serde_json::to_value(engine.list()).map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::TreeSlice {
          scan_id,
          node_id,
          depth,
          max_children_per_node,
        } => {
          let request = TreeSliceRequest {
            scan_id,
            node_id,
            depth,
            max_children_per_node,
          };
          serde_json::to_value(engine.slice(&request).map_err(problem_to_value)?)
            .map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::TreeChildren {
          scan_id,
          node_id,
          offset,
          limit,
          sort,
        } => {
          let request = ChildrenPageRequest {
            scan_id,
            node_id,
            offset,
            limit,
            sort,
          };
          serde_json::to_value(engine.children(&request).map_err(problem_to_value)?)
            .map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::Discovery { .. } => {
          unreachable!("discovery handled before locking the engine")
        }
        ReadRequest::AppAudit { .. } => {
          unreachable!("appAudit handled before locking the engine")
        }
        ReadRequest::AppIcon { .. } => {
          unreachable!("appIcon handled before locking the engine")
        }
        ReadRequest::MemorySnapshot
        | ReadRequest::MemoryHistory { .. }
        | ReadRequest::MemoryCandidates { .. }
        | ReadRequest::MemoryAudit => {
          unreachable!("memory reads handled before locking the engine")
        }
      };
      drop(engine);
      result
    })();
    let _ = reply.send(match outcome {
      Ok(value) => json!({ "ok": true, "result": value }),
      Err(problem) => json!({ "ok": false, "problem": problem }),
    });
    Ok(())
  });
  handle
    .await
    .map_err(|error| problem_value("InternalError", error))?
}

// ----------------------------------------------------------------- mutations

/// Standalone audit read: runs the app-storage audit outside the engine lock
/// (mirroring `read_discovery`). The contract bounds `limit` to 1..=1000;
/// enforce it here so a raw IPC caller cannot request beyond that shape.
/// Successful audits extend the session's icon allow-list (`known_paths`;
/// audits accumulate, never clear) with every reported `.app` bundle path AND
/// atomically publish a new `AuditSnapshot` (fresh `audit_id`, exact per-app
/// locations) that app cleanup and reveal authorize against. A failed audit
/// returns without touching the published snapshot; the report JSON carries
/// the generation's `auditId` top-level.
fn read_app_audit(
  bundle: &SessionBundle,
  min_size: u64,
  include_heuristic: bool,
  limit: usize,
) -> Result<Value, Value> {
  read_app_audit_inner(bundle, min_size, include_heuristic, limit, None)
}

fn read_app_audit_inner(
  bundle: &SessionBundle,
  min_size: u64,
  include_heuristic: bool,
  limit: usize,
  home: Option<PathBuf>,
) -> Result<Value, Value> {
  if limit == 0 || limit > 1000 {
    return Err(problem_value(
      "InvalidRequest",
      "appAudit limit must be between 1 and 1000",
    ));
  }
  let mut state = bundle.app_audit.lock().unwrap();
  let options = spacelens_app_audit::AppAuditOptions {
    min_size,
    include_heuristic,
    limit,
    home,
  };
  let report = spacelens_app_audit::audit_installed_apps(&options)
    .map_err(|error| problem_value("InternalError", error))?;
  publish_audit_snapshot(&mut state, &report)
}

/// Publish one successful audit's authorization snapshot: fresh `audit_id`,
/// exact per-app locations, icon allow-list accumulation, and the report
/// serialized ONCE (the snapshot keeps the Value the reply returns, so the
/// wire report and the authorization source always agree). Callers only reach
/// here after a successful audit, so a failed audit never publishes.
fn publish_audit_snapshot(
  state: &mut AppAuditState,
  report: &spacelens_app_audit::AppAuditReport,
) -> Result<Value, Value> {
  let audit_id = crate::app_cleanup::new_audit_id();
  let mut locations_by_app: std::collections::HashMap<String, Vec<PathBuf>> =
    std::collections::HashMap::new();
  let mut app_paths: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
  for app in &report.apps {
    locations_by_app.insert(
      app.id.clone(),
      app
        .locations
        .iter()
        .map(|location| PathBuf::from(&location.path))
        .collect(),
    );
    let Some(app_path) = app.app_path.as_deref() else {
      continue;
    };
    if let Ok(canonical) = std::fs::canonicalize(app_path) {
      state.known_paths.insert(canonical.clone());
      app_paths.insert(app.id.clone(), canonical);
    }
  }
  // Serialize the report ONCE: the snapshot keeps this Value and the reply
  // returns it, so the wire report and the authorization source agree.
  let mut value =
    serde_json::to_value(report).map_err(|error| problem_value("InternalError", error))?;
  value
    .as_object_mut()
    .ok_or_else(|| {
      problem_value(
        "InternalError",
        "audit report did not serialize to an object",
      )
    })?
    .insert("auditId".to_string(), Value::String(audit_id.clone()));
  state.audit = Some(AuditSnapshot {
    audit_id,
    report: value.clone(),
    locations_by_app,
    app_paths,
  });
  Ok(value)
}

/// Standalone icon read: serves a downscaled PNG data URL for one audited app.
/// The path must have been returned by an app storage audit in this session;
/// anything else is `Forbidden`, so a raw IPC caller cannot turn this into an
/// arbitrary-file read. Never touches the engine lock.
fn read_app_icon(bundle: &SessionBundle, app_path: &str) -> Result<Value, Value> {
  // Fail closed on symlinks and missing paths: only real bundles resolve.
  if std::fs::symlink_metadata(app_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
    return Err(problem_value(
      "NotFound",
      format!("no such app: {app_path}"),
    ));
  }
  let canonical = std::fs::canonicalize(app_path)
    .map_err(|_| problem_value("NotFound", format!("no such app: {app_path}")))?;
  let mut state = bundle.app_audit.lock().unwrap();
  if !state.known_paths.contains(&canonical) {
    return Err(problem_value(
      "Forbidden",
      "app icons are only served for apps returned by an app storage audit",
    ));
  }
  if let Some(data_url) = state.icons.get(&canonical) {
    return serde_json::to_value(json!({ "dataUrl": data_url }))
      .map_err(|error| problem_value("InternalError", error));
  }
  let png = spacelens_app_audit::app_icon_png(&canonical);
  let Some(png) = png else {
    return Err(problem_value("NotFound", "no icon available for this app"));
  };
  let data_url = format!(
    "data:image/png;base64,{}",
    base64::engine::general_purpose::STANDARD.encode(&png)
  );
  state.icons.insert(canonical, data_url.clone());
  serde_json::to_value(json!({ "dataUrl": data_url }))
    .map_err(|error| problem_value("InternalError", error))
}

fn read_discovery(
  bundle: &SessionBundle,
  request: &DiscoveryRequest,
  walk: impl FnOnce(DiscoveryInput) -> Result<DiscoveryData, EngineProblem>,
) -> Result<DiscoveryPage, EngineProblem> {
  // A second view waits for the first walk to populate every discovery kind,
  // then checks the cache. Neither the engine nor global session lock is
  // held while waiting or walking.
  let _discovery = bundle.discovery.lock().unwrap();
  // Prepare under a short lock, run any legacy filesystem walk without the
  // engine lock, then register the immutable result before replying.
  // Protected scans never reach `walk`: their results were derived in memory
  // inside `prepare_discovery`, and the Ready/Cached branches here exist so
  // the compiler keeps that guarantee. The prepared value is bound first so
  // the prepare lock guard drops before the arms run.
  let prepared = bundle.engine.lock().unwrap().prepare_discovery(request)?;
  match prepared {
    PreparedDiscovery::Cached => bundle
      .engine
      .lock()
      .unwrap()
      .finish_discovery(request, None),
    PreparedDiscovery::Ready(data) => bundle
      .engine
      .lock()
      .unwrap()
      .finish_discovery(request, Some(data)),
    PreparedDiscovery::Walk(input) => {
      let data = walk(input)?;
      bundle
        .engine
        .lock()
        .unwrap()
        .finish_discovery(request, Some(data))
    }
  }
}

#[tauri::command]
pub async fn sl_submit(
  app: AppHandle,
  window: Window,
  session_id: String,
  request: SubmitRequest,
  reply: Channel<Value>,
) -> Result<(), Value> {
  // State is resolved inside the blocking task via the app handle, whose
  // clone is 'static — the borrowed State itself is not.
  let handle = tauri::async_runtime::spawn_blocking(move || {
    let state = app.state::<AppState>();
    if let Err(problem) = require_owner(&window, &state, &session_id) {
      let _ = reply.send(json!({ "ok": false, "problem": problem }));
      return Ok(());
    }
    let sessions = state.sessions.lock().unwrap();
    let bundle = sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}))?;
    drop(sessions);
    match request {
      // Where the guarded scanner cannot walk (Windows), a localOnly request
      // falls through to the regular scan below rather than finishing empty.
      SubmitRequest::ScanStart { request } if request.local_only && crate::engine::protected_scan_supported() => {
        let prepared = bundle.engine.lock().unwrap().prepare_local_scan(request);
        match prepared {
          Ok((session, task)) => {
            let status = bundle
              .engine
              .lock()
              .unwrap()
              .status(&session.scan_id)
              .map_err(problem_to_value)?;
            let _ = bundle
              .events
              .publish(json!({ "kind": "scan.updated", "status": status }));
            // Return a live scan session first; the long walk and callbacks
            // never retain the engine or session-map lock.
            if let Err(error) = reply.send(json!({ "ok": true, "result": session })) {
              let mut engine = bundle.engine.lock().unwrap();
              let _ = engine.cancel_scan(&task.scan_id);
              // No worker was launched, so there is nothing left to wait for.
              let _ = engine.finish_local_scan(
                &task.scan_id,
                Err(std::io::Error::new(
                  std::io::ErrorKind::Interrupted,
                  "scan reply channel closed",
                )),
              );
              return Err(problem_value("InternalError", error));
            }
            tauri::async_runtime::spawn_blocking(move || run_local_scan_task(&bundle, task));
          }
          Err(problem) => {
            let _ = reply.send(json!({ "ok": false, "problem": problem_to_value(problem) }));
          }
        }
        Ok(())
      }
      SubmitRequest::AppCleanupPlan { .. } | SubmitRequest::AppCleanupExecute { .. } => {
        let roots = state.roots.clone();
        submit_app_cleanup_reply(&bundle, &roots, request, &reply)
      }
      SubmitRequest::AppScanStart { request } => {
        submit_app_scan_reply(bundle, request, &reply);
        Ok(())
      }
      SubmitRequest::MemoryTerminatePlan { request } => {
        // Plans bind to this window's desktop session; the service holds the
        // immutable plan and re-checks identities again at execute time.
        let memory = app.state::<MemoryState>();
        let value = crate::memory::submit_plan(&memory, &session_id, &request);
        let _ = reply.send(match value {
          Ok(value) => json!({ "ok": true, "result": value }),
          Err(problem) => json!({ "ok": false, "problem": problem }),
        });
        Ok(())
      }
      SubmitRequest::MemoryTerminateExecute { request } => {
        // Execute waits out the graceful/force window. This whole task already
        // runs on `tauri::async_runtime::spawn_blocking`, so it never touches
        // the main thread.
        let memory = app.state::<MemoryState>();
        let value = crate::memory::submit_execute(&memory, &session_id, &request);
        let _ = reply.send(match value {
          Ok(value) => json!({ "ok": true, "result": value }),
          Err(problem) => json!({ "ok": false, "problem": problem }),
        });
        Ok(())
      }
      request => {
        let mut engine = bundle.engine.lock().unwrap();
        submit_engine_reply(&mut engine, &bundle.events, request, &reply)
      }
    }
  });
  handle
    .await
    .map_err(|error| problem_value("InternalError", error))?
}

fn run_local_scan_task(bundle: &SessionBundle, task: LocalScanTask) {
  let scan_id = task.scan_id;
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    space_lens::local_scan::scan_local_directory_with_progress(
      task.options,
      task.cancellation,
      |progress| {
        // Serialize publication with cancellation: a captured progress copy
        // must never arrive after the cancelled terminal event.
        let mut engine = bundle.engine.lock().unwrap();
        let updated = engine.update_local_scan_progress(&scan_id, progress);
        if let Ok(status) = updated {
          if status.state == "scanning" {
            let _ = bundle
              .events
              .publish(json!({ "kind": "scan.updated", "status": status }));
          }
        }
      },
    )
  }))
  .unwrap_or_else(|_| Err(std::io::Error::other("protected scanner panicked")));
  // Read before taking the engine lock: it spawns `diskutil`.
  let storage = result.as_ref().ok().and_then(|report| {
    let roots: Vec<ScannedRoot> = report
      .nodes
      .iter()
      .map(|node| ScannedRoot {
        path: node.path.clone(),
        bytes: node.size,
      })
      .collect();
    storage_breakdown(&roots)
  });
  let mut engine = bundle.engine.lock().unwrap();
  let was_scanning = engine
    .status(&scan_id)
    .is_ok_and(|status| status.state == "scanning");
  let status = engine
    .finish_local_scan(&scan_id, result)
    .and_then(|_| engine.attach_storage(&scan_id, storage));
  drop(engine);
  if was_scanning {
    if let Ok(status) = status {
      let kind = if status.state == "ready" {
        "scan.completed"
      } else {
        "scan.updated"
      };
      let _ = bundle
        .events
        .publish(json!({ "kind": kind, "status": status }));
    }
  }
}

/// App-scoped cleanup dispatch: clones the current audit snapshot under a
/// short app-audit lock, then works only against the app-cleanup state. The
/// engine lock is never taken on this path.
fn submit_app_cleanup_plan(
  bundle: &SessionBundle,
  configured_roots: &[PathBuf],
  request: AppCleanupPlanRequest,
) -> Result<Value, Value> {
  let snapshot = bundle.app_audit.lock().unwrap().audit.clone();
  let Some(snapshot) = snapshot else {
    return Err(problem_value(
      "InvalidRequest",
      "audit is stale; re-run the audit",
    ));
  };
  let mut cleanup = bundle.app_cleanup.lock().unwrap();
  crate::app_cleanup::plan_app_cleanup(&mut cleanup, &snapshot, &request, configured_roots)
}

fn submit_app_cleanup_execute(
  bundle: &SessionBundle,
  request: AppCleanupExecuteRequest,
) -> Result<Value, Value> {
  let snapshot = bundle.app_audit.lock().unwrap().audit.clone();
  let mut cleanup = bundle.app_cleanup.lock().unwrap();
  crate::app_cleanup::execute_app_cleanup(&mut cleanup, snapshot.as_ref(), &request)
}

fn submit_app_cleanup_reply(
  bundle: &SessionBundle,
  configured_roots: &[PathBuf],
  request: SubmitRequest,
  reply: &Channel<Value>,
) -> Result<(), Value> {
  // Rejected plans/executions reply through the Channel like every engine
  // failure — they never escape the command and never touch the engine.
  let value: Result<Value, Value> = match request {
    SubmitRequest::AppCleanupPlan { request } => {
      submit_app_cleanup_plan(bundle, configured_roots, request)
    }
    SubmitRequest::AppCleanupExecute { request } => submit_app_cleanup_execute(bundle, request),
    _ => Err(problem_value(
      "InternalError",
      "engine submits never route to app cleanup",
    )),
  };
  reply
    .send(match value {
      Ok(value) => json!({ "ok": true, "result": value }),
      Err(problem) => json!({ "ok": false, "problem": problem }),
    })
    .map_err(|error| problem_value("InternalError", error))
}

/// App-scan dispatch: clones the current audit snapshot under a short
/// app-audit lock (the single A7 pinning point), prepares the session under a
/// short engine lock, replies the session FIRST, and walks outside every lock
/// — mirroring the local-scan session-first flow. A closed reply channel
/// cancels the leased worker like `sl_submit` does for local scans.
fn submit_app_scan_reply(
  bundle: std::sync::Arc<SessionBundle>,
  request: AppScanStartRequest,
  reply: &Channel<Value>,
) {
  let snapshot = bundle.app_audit.lock().unwrap().audit.clone();
  let Some(snapshot) = snapshot else {
    let _ = reply.send(json!({ "ok": false, "problem": { "code": "InvalidRequest", "message": "audit is stale; re-run the audit", "retryable": false } }));
    return;
  };
  // The snapshot keeps the serialized report (carrying top-level `auditId`);
  // deserialize it back to the typed report for the app lookup. The extra
  // `auditId` key is ignored by the report's open shape.
  let report: Result<spacelens_app_audit::AppAuditReport, _> =
    serde_json::from_value(snapshot.report.clone());
  let Ok(report) = report else {
    let _ = reply.send(json!({ "ok": false, "problem": { "code": "InternalError", "message": "audit snapshot is corrupt", "retryable": false } }));
    return;
  };
  let prepared = bundle
    .engine
    .lock()
    .unwrap()
    .prepare_app_scan(request, &snapshot, &report);
  match prepared {
    Ok((session, task)) => {
      // Prepare-time captures for the finish cross-check (A7): cloned from
      // the SAME snapshot, never reread from current audit state.
      let locations: Vec<PathBuf> = snapshot
        .locations_by_app
        .get(
          &session
            .app
            .as_ref()
            .map(|app| app.app_id.clone())
            .unwrap_or_default(),
        )
        .cloned()
        .unwrap_or_default();
      let origin = session.app.clone().unwrap();
      let bundle_info = snapshot.app_paths.get(&origin.app_id).map(|path| {
        let name = report
          .apps
          .iter()
          .find(|app| app.id == origin.app_id)
          .map(|app| app.name.clone())
          .unwrap_or_default();
        let size = report
          .apps
          .iter()
          .find(|app| app.id == origin.app_id)
          .map(|app| app.installed_size)
          .unwrap_or(0);
        (path.clone(), size, name)
      });
      let status = bundle
        .engine
        .lock()
        .unwrap()
        .status(&session.scan_id)
        .map_err(problem_to_value);
      if let Ok(status) = status {
        let _ = bundle
          .events
          .publish(json!({ "kind": "scan.updated", "status": status }));
      }
      if reply
        .send(json!({ "ok": true, "result": session }))
        .is_err()
      {
        let mut engine = bundle.engine.lock().unwrap();
        let _ = engine.cancel_scan(&task.scan_id);
        let _ = engine.finish_app_scan(
          &task.scan_id,
          Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "scan reply channel closed",
          )),
          origin,
          locations,
          bundle_info,
        );
        return;
      }
      tauri::async_runtime::spawn_blocking(move || {
        run_app_scan_task(&bundle, task, origin, locations, bundle_info)
      });
    }
    Err(problem) => {
      let _ = reply.send(json!({ "ok": false, "problem": problem_to_value(problem) }));
    }
  }
}

/// Background app-scan walk: progress events stream while scanning; the
/// engine lock is held only for progress/finish updates, never the walk.
fn run_app_scan_task(
  bundle: &SessionBundle,
  task: LocalScanTask,
  origin: crate::engine::AppScanOrigin,
  locations: Vec<PathBuf>,
  bundle_info: Option<(PathBuf, u64, String)>,
) {
  let scan_id = task.scan_id;
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    space_lens::local_scan::scan_local_directory_with_progress(
      task.options,
      task.cancellation,
      |progress| {
        let mut engine = bundle.engine.lock().unwrap();
        let updated = engine.update_local_scan_progress(&scan_id, progress);
        if let Ok(status) = updated {
          if status.state == "scanning" {
            let _ = bundle
              .events
              .publish(json!({ "kind": "scan.updated", "status": status }));
          }
        }
      },
    )
  }))
  .unwrap_or_else(|_| Err(std::io::Error::other("protected scanner panicked")));
  let mut engine = bundle.engine.lock().unwrap();
  let was_scanning = engine
    .status(&scan_id)
    .is_ok_and(|status| status.state == "scanning");
  let status = engine.finish_app_scan(&scan_id, result, origin, locations, bundle_info);
  drop(engine);
  if was_scanning {
    if let Ok(status) = status {
      let kind = if status.state == "ready" {
        "scan.completed"
      } else {
        "scan.updated"
      };
      let _ = bundle
        .events
        .publish(json!({ "kind": kind, "status": status }));
    }
  }
}

fn submit_engine_reply(
  engine: &mut EngineStore,
  events: &EventSink,
  request: SubmitRequest,
  reply: &Channel<Value>,
) -> Result<(), Value> {
  // Engine failures must stay inside this outcome closure so every result
  // travels through Channel, including stale/forbidden cleanup selections.
  let value: Result<Value, Value> = (|| match request {
    SubmitRequest::ScanStart { request } => {
      let session = engine.start_scan(request).map_err(problem_to_value)?;
      let status = engine.status(&session.scan_id).map_err(problem_to_value)?;
      let _ = events.publish(json!({ "kind": "scan.completed", "status": status }));
      serde_json::to_value(session).map_err(|error| problem_value("InternalError", error))
    }
    SubmitRequest::CleanupPlan { request } => {
      serde_json::to_value(engine.plan(&request).map_err(problem_to_value)?)
        .map_err(|error| problem_value("InternalError", error))
    }
    SubmitRequest::CleanupExecute { request } => {
      serde_json::to_value(engine.execute(&request).map_err(problem_to_value)?)
        .map_err(|error| problem_value("InternalError", error))
    }
    SubmitRequest::AppCleanupPlan { .. } | SubmitRequest::AppCleanupExecute { .. } => {
      Err(problem_value(
        "InternalError",
        "app cleanup is dispatched before the engine lock",
      ))
    }
    SubmitRequest::AppScanStart { .. } => Err(problem_value(
      "InternalError",
      "app scans are dispatched before the engine lock",
    )),
    SubmitRequest::MemoryTerminatePlan { .. } | SubmitRequest::MemoryTerminateExecute { .. } => {
      Err(problem_value(
        "InternalError",
        "memory submits are dispatched before the engine lock",
      ))
    }
  })();
  reply
    .send(match value {
      Ok(value) => json!({ "ok": true, "result": value }),
      Err(problem) => json!({ "ok": false, "problem": problem }),
    })
    .map_err(|error| problem_value("InternalError", error))
}

// --------------------------------------------------------------------- host

#[tauri::command]
pub async fn sl_host_request(
  app: AppHandle,
  window: Window,
  session_id: String,
  request: HostRequest,
  reply: Channel<Value>,
) -> Result<(), Value> {
  // Same Channel reply as every other command: the invoke response body is
  // unreliable on macOS, so answers always ride the channel.
  let handle = tauri::async_runtime::spawn_blocking(move || {
    let state = app.state::<AppState>();
    if let Err(problem) = require_owner(&window, &state, &session_id) {
      let _ = reply.send(json!({ "ok": false, "problem": problem }));
      return Ok(());
    }
    let sessions = state.sessions.lock().unwrap();
    let bundle = sessions.get(&session_id).cloned().ok_or_else(
      || json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}),
    )?;
    drop(sessions);
    let value: Result<Value, Value> = match request {
      HostRequest::PickDirectory { title } => {
        use tauri_plugin_dialog::DialogExt;
        let _ = title;
        // Blocking pick from a blocking-pool thread (never the main
        // thread): the dialog is modal to this window and the
        // WebView stays alive throughout.
        let picked = app.dialog().file().blocking_pick_folder();
        Ok(json!({
            "picked": picked.map(|path| path.to_string()),
        }))
      }
      HostRequest::RevealPath { app_id, path } => {
        // Allow-listed against the current audit generation only; anything
        // else is Forbidden, never a raw filesystem reveal.
        let snapshot = bundle.app_audit.lock().unwrap().audit.clone();
        crate::app_cleanup::reveal_app_location(snapshot.as_ref(), &app_id, &path)
      }
      HostRequest::RevealScanPath { path, action } => {
        // A scanned path: resolved against the roots this shell serves and
        // refused when it sits outside them.
        crate::reveal::reveal_scan_path(
          &state.roots,
          &path,
          action.unwrap_or(crate::reveal::RevealAction::Reveal),
        )
      }
      HostRequest::ReopenApp { audit_id } => {
        // The bundle id comes from the memory audit record alone (§4.7); the
        // arm opens an allow-listed identifier, never a caller-supplied path.
        let memory = app.state::<MemoryState>();
        crate::memory::reopen_app(&memory, &audit_id)
      }
    };
    let _ = reply.send(match value {
      Ok(value) => json!({ "ok": true, "result": value }),
      Err(problem) => json!({ "ok": false, "problem": problem }),
    });
    Ok(())
  });
  handle
    .await
    .map_err(|error| problem_value("InternalError", error))?
}

// ------------------------------------------------------------------- events

#[tauri::command]
pub fn sl_events_subscribe(
  app: AppHandle,
  window: Window,
  session_id: String,
  after_sequence: Option<u64>,
  state: State<'_, AppState>,
) -> Result<SubscriptionAck, Value> {
  require_owner(&window, &state, &session_id)?;
  let sessions = state.sessions.lock().unwrap();
  let bundle = sessions.get(&session_id).ok_or_else(
    || json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}),
  )?;
  // The channel registers before the replay snapshot, so every event
  // published after this call is queued for the window; the synchronous
  // replay below overlaps that channel and the client's sequence watermark
  // deduplicates the overlap.
  let (subscription_id, mut ack, receiver) = bundle.events.subscribe_channel(after_sequence);
  ack.service_instance_id = SERVICE_INSTANCE_ID.to_string();
  let frames: Vec<ScopedEventFrame> = bundle
    .events
    .frames_for(&session_id, &subscription_id, SERVICE_INSTANCE_ID)
    .into_iter()
    .filter(|frame| after_sequence.is_none_or(|since| frame.event.sequence > since))
    .collect();
  let sink = bundle.events.clone();
  let cleanup_sink = sink.clone();
  drop(sessions);
  for frame in frames {
    let _ = app.emit_to(window.label(), EVENT_NAME, frame);
  }
  // Forward live events on a dedicated thread. When the window is gone the
  // emit fails and the thread unsubscribes itself — dropping the sender ends
  // the loop — so a closed window never leaks a thread or subscription.
  let forward_session_id = session_id.clone();
  let forward_subscription_id = subscription_id.clone();
  let label = window.label().to_string();
  let app_handle = app.clone();
  let forwarder = std::thread::Builder::new()
    .name(format!("sl-events-{subscription_id}"))
    .spawn(move || {
      while let Ok(envelope) = receiver.recv() {
        let frame = ScopedEventFrame {
          session_id: forward_session_id.clone(),
          subscription_id: forward_subscription_id.clone(),
          service_instance_id: SERVICE_INSTANCE_ID.to_string(),
          event: envelope,
        };
        if app_handle.emit_to(&label, EVENT_NAME, frame).is_err() {
          sink.unsubscribe(&forward_subscription_id);
          break;
        }
      }
    });
  if let Err(error) = forwarder {
    // No forwarder owns the channel: drop the sender ourselves so the
    // subscription does not linger until the next publish.
    cleanup_sink.unsubscribe(&subscription_id);
    return Err(problem_value("InternalError", error));
  }
  Ok(ack)
}

#[tauri::command]
pub fn sl_events_unsubscribe(
  app: AppHandle,
  window: Window,
  session_id: String,
  subscription_id: String,
  state: State<'_, AppState>,
) -> Result<Value, Value> {
  require_owner(&window, &state, &session_id)?;
  let _ = &app;
  let bundle = state.sessions.lock().unwrap().get(&session_id).cloned();
  let Some(bundle) = bundle else {
    return Err(
      json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}),
    );
  };
  // Removing the sender ends the forwarding thread's recv loop; the thread
  // also unsubscribes itself when it observes a dead window.
  bundle.events.unsubscribe(&subscription_id);
  Ok(json!({ "unsubscribed": true }))
}

fn problem_to_value(problem: crate::engine::EngineProblem) -> Value {
  json!({ "problem": { "code": problem.code, "message": problem.message, "retryable": false } })
}

fn problem_value(code: &str, error: impl std::fmt::Display) -> Value {
  json!({ "problem": { "code": code, "message": error.to_string(), "retryable": false } })
}

#[cfg(test)]
mod tests {
  use super::{submit_engine_reply, ReadRequest, SubmitRequest};

  #[test]
  fn reconnecting_the_same_window_preserves_its_session_and_running_worker_slot() {
    let path = std::env::temp_dir().join("sl-window-registry-no-filesystem-access");
    let state = super::AppState::new(vec![path.clone()]);
    let first = super::connect_window_session("main", &state);
    let session_id = first["sessionId"].as_str().unwrap();
    let bundle = state
      .sessions
      .lock()
      .unwrap()
      .get(session_id)
      .unwrap()
      .clone();
    let request =
      serde_json::from_value(serde_json::json!({ "paths": [path], "localOnly": true })).unwrap();
    let (scan, _task) = bundle
      .engine
      .lock()
      .unwrap()
      .prepare_local_scan(request)
      .unwrap();
    let reconnect = super::connect_window_session("main", &state);
    assert_eq!(
      reconnect["sessionId"], first["sessionId"],
      "a refreshed renderer must recover the original engine"
    );
    assert_eq!(state.sessions.lock().unwrap().len(), 1);
    assert_eq!(
      bundle
        .engine
        .lock()
        .unwrap()
        .status(&scan.scan_id)
        .unwrap()
        .state,
      "scanning"
    );
    let other = super::connect_window_session("other", &state);
    assert_ne!(other["sessionId"], first["sessionId"]);
  }

  #[test]
  fn disconnect_and_window_destroy_cancel_only_owned_scans_without_releasing_running_slots() {
    let path = std::env::temp_dir().join("sl-window-disconnect-no-filesystem-access");
    let state = super::AppState::new(vec![path.clone()]);
    let main = super::connect_window_session("main", &state);
    let other = super::connect_window_session("other", &state);
    let main_id = main["sessionId"].as_str().unwrap();
    let other_id = other["sessionId"].as_str().unwrap();
    let main_bundle = state.sessions.lock().unwrap().get(main_id).unwrap().clone();
    let other_bundle = state
      .sessions
      .lock()
      .unwrap()
      .get(other_id)
      .unwrap()
      .clone();
    let request =
      || serde_json::from_value(serde_json::json!({ "paths": [path], "localOnly": true })).unwrap();
    let (main_scan, main_task) = main_bundle
      .engine
      .lock()
      .unwrap()
      .prepare_local_scan(request())
      .unwrap();
    let (other_scan, other_task) = other_bundle
      .engine
      .lock()
      .unwrap()
      .prepare_local_scan(request())
      .unwrap();
    assert!(super::disconnect_window_session("other", main_id, &state).is_err());
    assert_eq!(
      main_bundle
        .engine
        .lock()
        .unwrap()
        .status(&main_scan.scan_id)
        .unwrap()
        .state,
      "scanning"
    );
    super::disconnect_window_session("main", main_id, &state).unwrap();
    assert!(main_task.cancellation.is_cancelled());
    assert!(!other_task.cancellation.is_cancelled());
    assert_eq!(
      other_bundle
        .engine
        .lock()
        .unwrap()
        .status(&other_scan.scan_id)
        .unwrap()
        .state,
      "scanning"
    );
    assert_eq!(
      super::connect_window_session("main", &state)["sessionId"],
      main["sessionId"]
    );
    assert_eq!(
      main_bundle
        .engine
        .lock()
        .unwrap()
        .prepare_local_scan(request())
        .err()
        .unwrap()
        .code,
      "LimitExceeded"
    );
    let legacy =
      serde_json::from_value(serde_json::json!({ "paths": [path], "localOnly": false })).unwrap();
    assert_eq!(
      main_bundle
        .engine
        .lock()
        .unwrap()
        .start_scan(legacy)
        .unwrap_err()
        .code,
      "LimitExceeded"
    );
    main_bundle
      .engine
      .lock()
      .unwrap()
      .finish_local_scan(
        &main_scan.scan_id,
        Err(std::io::Error::new(
          std::io::ErrorKind::Interrupted,
          "worker exited",
        )),
      )
      .unwrap();
    let (next, next_task) = main_bundle
      .engine
      .lock()
      .unwrap()
      .prepare_local_scan(request())
      .unwrap();
    super::disconnect_window("main", &state);
    assert!(next_task.cancellation.is_cancelled());
    assert_eq!(
      main_bundle
        .engine
        .lock()
        .unwrap()
        .status(&next.scan_id)
        .unwrap()
        .state,
      "cancelled"
    );
    assert!(!other_task.cancellation.is_cancelled());
    assert_eq!(state.sessions.lock().unwrap().len(), 2);
  }

  #[test]
  fn concurrent_same_window_connections_allocate_only_one_session() {
    let state = std::sync::Arc::new(super::AppState::new(Vec::new()));
    let handles: Vec<_> = (0..8)
      .map(|_| {
        let state = state.clone();
        std::thread::spawn(move || super::connect_window_session("main", &state))
      })
      .collect();
    let replies: Vec<_> = handles
      .into_iter()
      .map(|handle| handle.join().unwrap())
      .collect();
    assert!(replies
      .iter()
      .all(|reply| reply["sessionId"] == replies[0]["sessionId"]));
    assert_eq!(state.sessions.lock().unwrap().len(), 1);
  }

  #[cfg(unix)]
  #[test]
  fn protected_native_worker_finishes_a_local_fixture_and_derives_discovery_from_the_report() {
    let path = std::env::temp_dir().join(format!("sl-native-worker-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("file"), vec![b'x'; 8192]).unwrap();
    let path = std::fs::canonicalize(path).unwrap();
    let mut engine = crate::engine::EngineStore::new(vec![path.clone()]);
    let request =
      serde_json::from_value(serde_json::json!({ "paths": [path], "localOnly": true })).unwrap();
    let (session, task) = engine.prepare_local_scan(request).unwrap();
    let bundle = super::SessionBundle {
      window_label: "fixture".into(),
      engine: std::sync::Mutex::new(engine),
      events: crate::events::EventSink::new(),
      discovery: std::sync::Mutex::new(()),
      app_audit: std::sync::Mutex::new(super::AppAuditState::default()),
      app_cleanup: std::sync::Mutex::new(crate::app_cleanup::AppCleanupState::default()),
    };
    super::run_local_scan_task(&bundle, task);
    let status = bundle
      .engine
      .lock()
      .unwrap()
      .status(&session.scan_id)
      .unwrap();
    assert_eq!(status.state, "ready");
    assert_eq!(status.coverage.unwrap().logical_bytes, 8192);
    // Discovery for a protected scan is derived from the indexed report; the
    // panic closure is the sentinel proving the legacy filesystem walk never
    // runs for one.
    let page = super::read_discovery(
      &bundle,
      &crate::engine::DiscoveryRequest {
        scan_id: session.scan_id,
        kind: crate::engine::DiscoveryKind::LargeFiles,
        min_size: 0,
        offset: 0,
        limit: 200,
      },
      |_| panic!("protected discovery entered the legacy filesystem walk"),
    )
    .unwrap();
    assert_eq!(page.total, 1);
    assert!(page.items[0].node.path.ends_with("/file"));
    assert_eq!(page.items[0].category, "Large file");
    let (_, replay) = bundle.events.subscribe(None);
    assert_eq!(
      replay
        .replay
        .iter()
        .filter(|event| event.payload["kind"] == "scan.completed")
        .count(),
      1
    );
    std::fs::remove_dir_all(path).unwrap();
  }

  #[test]
  fn capabilities_declare_only_reachable_operations() {
    let capabilities = super::capabilities_json(serde_json::json!({
      "sample": true,
      "terminate": true,
      "gracefulAppQuit": true,
      "reopen": true,
      "platform": "macos",
    }));
    assert_eq!(capabilities["scan"]["start"], true);
    assert_eq!(capabilities["scan"]["cancel"], true);
    assert_eq!(capabilities["scan"]["discovery"], true);
    assert_eq!(capabilities["scan"]["appAudit"], true);
    assert_eq!(capabilities["scan"]["appScan"], true);
    assert_eq!(capabilities["scan"]["maxConcurrent"], 1);
    assert_eq!(capabilities["cleanup"]["plan"], true);
    assert_eq!(capabilities["cleanup"]["execute"], true);
    assert_eq!(capabilities["cleanup"]["appPlan"], true);
    assert_eq!(capabilities["cleanup"]["appExecute"], true);
    assert_eq!(capabilities["cleanup"]["mode"], "trash");
    assert_eq!(capabilities["host"]["folderPicker"], true);
    assert_eq!(capabilities["host"]["appLocationReveal"], true);
    assert_eq!(capabilities["host"]["pathReveal"], true);
    assert_eq!(
      capabilities["host"]["platform"],
      crate::reveal::host_platform()
    );
    // The memory capability is carried through untouched.
    assert_eq!(capabilities["memory"]["sample"], true);
    assert_eq!(capabilities["memory"]["terminate"], true);
    assert_eq!(capabilities["memory"]["gracefulAppQuit"], true);
    assert_eq!(capabilities["memory"]["reopen"], true);
    assert_eq!(capabilities["memory"]["platform"], "macos");
  }

  #[test]
  fn protected_start_and_cancel_requests_match_the_closed_ipc_contract() {
    let request: SubmitRequest = serde_json::from_value(serde_json::json!({
      "kind": "scanStart", "paths": ["/fixture"], "localOnly": true,
    }))
    .unwrap();
    assert!(matches!(request, SubmitRequest::ScanStart { request } if request.local_only));
    let cancel: ReadRequest = serde_json::from_value(serde_json::json!({
      "method": "scanCancel", "scanId": "scan_fixture",
    }))
    .unwrap();
    assert!(matches!(cancel, ReadRequest::ScanCancel { scan_id } if scan_id == "scan_fixture"));
    assert!(serde_json::from_value::<ReadRequest>(serde_json::json!({
      "method": "scanCancel", "scanId": "scan_fixture", "path": "/arbitrary",
    }))
    .is_err());
  }

  #[test]
  fn concurrent_discovery_requests_share_one_walk_without_blocking_engine_reads() {
    use std::sync::{
      atomic::{AtomicUsize, Ordering},
      mpsc, Arc, Mutex,
    };
    use std::time::Duration;
    let path = std::env::temp_dir().join(format!("sl-discovery-coalesce-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("file"), vec![b'x'; 8192]).unwrap();
    let path = std::fs::canonicalize(path).unwrap();
    let mut engine = crate::engine::EngineStore::new(vec![path.clone()]);
    let scan = engine
      .start_scan(crate::engine::ScanStartRequest {
        paths: vec![path.to_string_lossy().into_owned()],
        local_only: false,
        ignore_hidden: false,
        respect_gitignore: true,
        ignored_mode: Default::default(),
        label: None,
      })
      .unwrap();
    let request = crate::engine::DiscoveryRequest {
      scan_id: scan.scan_id.clone(),
      kind: crate::engine::DiscoveryKind::LargeFiles,
      min_size: 8192,
      offset: 0,
      limit: 200,
    };
    let bundle = Arc::new(super::SessionBundle {
      window_label: "fixture".into(),
      engine: Mutex::new(engine),
      events: crate::events::EventSink::new(),
      discovery: Mutex::new(()),
      app_audit: Mutex::new(super::AppAuditState::default()),
      app_cleanup: Mutex::new(crate::app_cleanup::AppCleanupState::default()),
    });
    let walks = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_bundle = bundle.clone();
    let first_request = request.clone();
    let first_walks = walks.clone();
    let first = std::thread::spawn(move || {
      super::read_discovery(&first_bundle, &first_request, |input| {
        first_walks.fetch_add(1, Ordering::SeqCst);
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        crate::engine::run_discovery(input)
      })
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
      bundle
        .engine
        .try_lock()
        .unwrap()
        .status(&scan.scan_id)
        .unwrap()
        .state,
      "ready"
    );
    let second_bundle = bundle.clone();
    let second_request = crate::engine::DiscoveryRequest {
      kind: crate::engine::DiscoveryKind::Caches,
      ..request
    };
    let second_walks = walks.clone();
    let (second_walk_tx, second_walk_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
      super::read_discovery(&second_bundle, &second_request, |input| {
        second_walks.fetch_add(1, Ordering::SeqCst);
        second_walk_tx.send(()).unwrap();
        crate::engine::run_discovery(input)
      })
    });
    let duplicate_walk_started = second_walk_rx
      .recv_timeout(Duration::from_millis(100))
      .is_ok();
    release_tx.send(()).unwrap();
    let files = first.join().unwrap().unwrap();
    let caches = second.join().unwrap().unwrap();
    std::fs::remove_dir_all(path).unwrap();
    assert!(
      !duplicate_walk_started,
      "the second view started another filesystem walk before the first completed"
    );
    assert_eq!(walks.load(Ordering::SeqCst), 1);
    assert_eq!(files.total, 1);
    assert_eq!(caches.total, 0);
  }

  #[test]
  fn rejected_cleanup_plans_reply_on_the_channel_instead_of_escaping_the_command() {
    let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = messages.clone();
    let channel = tauri::ipc::Channel::<serde_json::Value>::new(move |body| {
      let tauri::ipc::InvokeResponseBody::Json(json) = body else {
        panic!("expected JSON channel reply")
      };
      received
        .lock()
        .unwrap()
        .push(serde_json::from_str::<serde_json::Value>(&json).unwrap());
      Ok(())
    });
    let mut engine = crate::engine::EngineStore::new(Vec::new());
    let result = submit_engine_reply(
      &mut engine,
      &crate::events::EventSink::new(),
      SubmitRequest::CleanupPlan {
        request: crate::engine::CleanupPlanRequest {
          scan_id: "missing".into(),
          node_ids: Vec::new(),
        },
      },
      &channel,
    );
    assert!(result.is_ok());
    let messages = messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["ok"], false);
    assert_eq!(messages[0]["problem"]["problem"]["code"], "NotFound");
  }

  #[test]
  fn discovery_read_defaults_and_closed_fields_match_the_contract() {
    let request: ReadRequest = serde_json::from_value(serde_json::json!({
        "method": "discovery", "scanId": "scan_1", "kind": "large-files"
    }))
    .unwrap();
    assert!(matches!(
      request,
      ReadRequest::Discovery {
        min_size: 0,
        offset: 0,
        limit: 200,
        ..
      }
    ));
    for extra in [
      serde_json::json!({"path": "/arbitrary"}),
      serde_json::json!({"minSize": -1}),
      serde_json::json!({"kind": "unknown"}),
    ] {
      let mut value =
        serde_json::json!({"method": "discovery", "scanId": "scan_1", "kind": "caches"});
      value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
      assert!(serde_json::from_value::<ReadRequest>(value).is_err());
    }
  }

  #[test]
  fn app_audit_read_defaults_and_closed_fields_match_the_contract() {
    let request: ReadRequest = serde_json::from_value(serde_json::json!({
        "method": "appAudit", "minSize": 10, "includeHeuristic": false, "limit": 50
    }))
    .unwrap();
    assert!(matches!(
      request,
      ReadRequest::AppAudit {
        min_size: 10,
        include_heuristic: false,
        limit: 50,
      }
    ));
    let request: ReadRequest =
      serde_json::from_value(serde_json::json!({ "method": "appAudit" })).unwrap();
    assert!(matches!(
      request,
      ReadRequest::AppAudit {
        min_size: 0,
        include_heuristic: true,
        limit: 500,
      }
    ));
    for extra in [
      serde_json::json!({"path": "/arbitrary"}),
      serde_json::json!({"minSize": -1}),
      serde_json::json!({"includeHeuristic": "yes"}),
    ] {
      let mut value = serde_json::json!({"method": "appAudit"});
      value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
      assert!(serde_json::from_value::<ReadRequest>(value).is_err());
    }
  }

  #[test]
  fn app_icon_read_matches_the_closed_contract() {
    let request: ReadRequest = serde_json::from_value(serde_json::json!({
        "method": "appIcon", "appPath": "/Applications/Foo.app",
    }))
    .unwrap();
    assert!(matches!(
      request,
      ReadRequest::AppIcon { app_path } if app_path == "/Applications/Foo.app"
    ));
    // Missing appPath and unknown fields are rejected (deny_unknown_fields).
    assert!(
      serde_json::from_value::<ReadRequest>(serde_json::json!({"method": "appIcon"})).is_err()
    );
    assert!(serde_json::from_value::<ReadRequest>(serde_json::json!({
        "method": "appIcon", "appPath": "/Applications/Foo.app", "path": "/arbitrary",
    }))
    .is_err());
    assert!(serde_json::from_value::<ReadRequest>(serde_json::json!({
        "method": "appIcon", "appPath": 42,
    }))
    .is_err());
  }

  #[test]
  fn app_icon_for_paths_outside_the_audit_allow_list_is_forbidden() {
    // A fresh session has audited nothing: even an existing filesystem path
    // must be refused, proving the allow-list (not mere existence) gates icons.
    let probe = std::env::temp_dir().join(format!("sl-app-icon-forbidden-{}", std::process::id()));
    std::fs::write(&probe, b"probe").unwrap();
    let probe = std::fs::canonicalize(&probe).unwrap();
    let bundle = super::SessionBundle {
      window_label: "fixture".into(),
      engine: std::sync::Mutex::new(crate::engine::EngineStore::new(Vec::new())),
      events: crate::events::EventSink::new(),
      discovery: std::sync::Mutex::new(()),
      app_audit: std::sync::Mutex::new(super::AppAuditState::default()),
      app_cleanup: std::sync::Mutex::new(crate::app_cleanup::AppCleanupState::default()),
    };
    let problem = super::read_app_icon(&bundle, &probe.to_string_lossy()).unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // A missing path never reaches the allow-list check: it is NotFound.
    let missing = probe.with_file_name("sl-app-icon-forbidden-missing");
    let problem = super::read_app_icon(&bundle, &missing.to_string_lossy()).unwrap_err();
    assert_eq!(problem["problem"]["code"], "NotFound");
    // An allow-listed path without a loadable icon is NotFound, not Forbidden:
    // the gate passed, the icon load failed (a temp file is no `.app` bundle).
    bundle
      .app_audit
      .lock()
      .unwrap()
      .known_paths
      .insert(probe.clone());
    let problem = super::read_app_icon(&bundle, &probe.to_string_lossy()).unwrap_err();
    assert_eq!(problem["problem"]["code"], "NotFound");
    std::fs::remove_file(&probe).unwrap();
  }

  // ------------------------------------------------- app storage v2 fixtures

  fn app_cleanup_temp_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sl-app-cleanup-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // Canonicalize up front so expected paths match the canonical strings the
    // snapshot and plan traffic in.
    root.canonicalize().unwrap()
  }

  fn app_cleanup_bundle() -> super::SessionBundle {
    super::SessionBundle {
      window_label: "fixture".into(),
      engine: std::sync::Mutex::new(crate::engine::EngineStore::new(Vec::new())),
      events: crate::events::EventSink::new(),
      discovery: std::sync::Mutex::new(()),
      app_audit: std::sync::Mutex::new(super::AppAuditState::default()),
      app_cleanup: std::sync::Mutex::new(crate::app_cleanup::AppCleanupState::default()),
    }
  }

  fn app_cleanup_snapshot(
    audit_id: &str,
    locations: &[(&str, Vec<std::path::PathBuf>)],
    bundles: &[(&str, std::path::PathBuf)],
  ) -> crate::app_cleanup::AuditSnapshot {
    crate::app_cleanup::AuditSnapshot {
      audit_id: audit_id.into(),
      report: serde_json::json!({}),
      locations_by_app: locations
        .iter()
        .map(|(app, paths)| (app.to_string(), paths.clone()))
        .collect(),
      app_paths: bundles
        .iter()
        .map(|(app, path)| (app.to_string(), path.clone()))
        .collect(),
    }
  }

  fn synthetic_audit_report(
    entries: Vec<(String, Option<String>, Vec<String>)>,
  ) -> spacelens_app_audit::AppAuditReport {
    spacelens_app_audit::AppAuditReport {
      platform: "test".into(),
      generated_at: "2026-10-13T00:00:00Z".into(),
      apps: entries
        .into_iter()
        .map(
          |(id, app_path, locations)| spacelens_app_audit::AppAuditEntry {
            id: id.clone(),
            name: id,
            kind: spacelens_app_audit::AppKind::App,
            source: spacelens_app_audit::AuditSource::Heuristic,
            bundle_id: None,
            app_path,
            installed_size: 0,
            total_size: 0,
            locations: locations
              .into_iter()
              .map(|path| spacelens_app_audit::AppAuditLocation {
                path,
                label: None,
                classification: spacelens_app_audit::Classification::Cache,
                purgeable: true,
                comment: None,
                size: 0,
                source: spacelens_app_audit::AuditSource::Heuristic,
              })
              .collect(),
          },
        )
        .collect(),
      total_apps: 0,
      truncated: false,
    }
  }

  #[test]
  fn audit_publish_swaps_snapshot_with_fresh_id_and_locations() {
    let root = app_cleanup_temp_root("snapshot");
    let location = root.join("cache");
    std::fs::create_dir_all(&location).unwrap();
    std::fs::write(location.join("blob.bin"), vec![b'x'; 4096]).unwrap();
    let bundle_path = root.join("Fixture.app");
    std::fs::create_dir_all(&bundle_path).unwrap();
    let location_string = location.to_string_lossy().into_owned();
    let bundle_string = bundle_path.to_string_lossy().into_owned();

    let mut state = super::AppAuditState::default();
    // A refused audit publishes nothing.
    assert!(state.audit.is_none());
    let first = super::publish_audit_snapshot(
      &mut state,
      &synthetic_audit_report(vec![(
        "app1".into(),
        Some(bundle_string.clone()),
        vec![location_string.clone()],
      )]),
    )
    .unwrap();
    let first_id = first["auditId"].as_str().unwrap().to_string();
    assert!(first_id.starts_with("audit_"));
    assert!(first_id.len() > "audit_".len() + 8);
    let snapshot = state.audit.clone().expect("snapshot published");
    assert_eq!(snapshot.audit_id, first_id);
    assert_eq!(snapshot.report, first);
    assert_eq!(
      snapshot.locations_by_app["app1"],
      vec![std::path::PathBuf::from(&location_string)]
    );
    assert_eq!(
      snapshot.app_paths["app1"],
      std::fs::canonicalize(&bundle_path).unwrap()
    );
    assert!(state
      .known_paths
      .contains(&std::fs::canonicalize(&bundle_path).unwrap()));

    // A second generation replaces the first atomically with a fresh id.
    let second = super::publish_audit_snapshot(
      &mut state,
      &synthetic_audit_report(vec![("app1".into(), None, vec![location_string.clone()])]),
    )
    .unwrap();
    let second_id = second["auditId"].as_str().unwrap();
    assert_ne!(second_id, first_id);
    let snapshot = state.audit.clone().expect("snapshot replaced");
    assert_eq!(snapshot.audit_id, second_id);
    assert_eq!(snapshot.report, second);
    // The icon allow-list accumulates across generations (unchanged behavior).
    assert!(state
      .known_paths
      .contains(&std::fs::canonicalize(&bundle_path).unwrap()));
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn failed_audit_never_publishes_a_snapshot() {
    let bundle = app_cleanup_bundle();
    let problem = super::read_app_audit_inner(&bundle, 0, true, 0, None).unwrap_err();
    assert_eq!(problem["problem"]["code"], "InvalidRequest");
    assert!(bundle.app_audit.lock().unwrap().audit.is_none());
  }

  #[test]
  fn app_cleanup_plan_refuses_foreign_and_misshaped_requests() {
    let root = app_cleanup_temp_root("refusals");
    let app1_loc = root.join("app1-cache");
    let app2_loc = root.join("app2-cache");
    for dir in [&app1_loc, &app2_loc] {
      std::fs::create_dir_all(dir).unwrap();
      std::fs::write(dir.join("blob.bin"), vec![b'x'; 1024]).unwrap();
    }
    let bundle_path = root.join("App1.app");
    std::fs::create_dir_all(&bundle_path).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[
        ("app1", vec![app1_loc.clone()]),
        ("app2", vec![app2_loc.clone()]),
      ],
      &[("app1", bundle_path.clone())],
    );
    let plan = |state: &mut crate::app_cleanup::AppCleanupState,
                req: crate::app_cleanup::AppCleanupPlanRequest| {
      crate::app_cleanup::plan_app_cleanup(state, &snapshot, &req, &[])
    };
    let base = |paths: Vec<String>| crate::app_cleanup::AppCleanupPlanRequest {
      audit_id: "audit_abcdef1234567890abcdef".into(),
      app_id: "app1".into(),
      paths,
    };

    let mut state = crate::app_cleanup::AppCleanupState::default();
    // Stale generation.
    let problem = plan(
      &mut state,
      crate::app_cleanup::AppCleanupPlanRequest {
        audit_id: "audit_000000000000000000000000".into(),
        ..base(vec![app1_loc.to_string_lossy().into_owned()])
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "InvalidRequest");
    // Arbitrary path: never reported.
    let problem = plan(
      &mut state,
      base(vec![root.join("elsewhere").to_string_lossy().into_owned()]),
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Bundle path: allow-listed for icons/reveal, never stageable.
    let problem = plan(
      &mut state,
      base(vec![bundle_path.to_string_lossy().into_owned()]),
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Another app's location.
    let problem = plan(
      &mut state,
      base(vec![app2_loc.to_string_lossy().into_owned()]),
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Unknown app id.
    let problem = plan(
      &mut state,
      crate::app_cleanup::AppCleanupPlanRequest {
        app_id: "nope".into(),
        ..base(vec![app1_loc.to_string_lossy().into_owned()])
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Shape bounds: empty/oversized paths and app ids.
    for bad in [
      base(Vec::new()),
      base(vec!["/tmp/x".into(); 201]),
      crate::app_cleanup::AppCleanupPlanRequest {
        app_id: String::new(),
        ..base(vec![app1_loc.to_string_lossy().into_owned()])
      },
    ] {
      assert_eq!(
        plan(&mut state, bad).unwrap_err()["problem"]["code"],
        "InvalidRequest"
      );
    }
    assert!(crate::app_cleanup::plan_count_for_test(&state) == 0);
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn app_cleanup_plan_prunes_nested_staging() {
    let root = app_cleanup_temp_root("nesting");
    let parent = root.join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("top.bin"), vec![b'x'; 4096]).unwrap();
    std::fs::write(child.join("nested.bin"), vec![b'y'; 4096]).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![parent.clone(), child.clone()])],
      &[],
    );
    let mut state = crate::app_cleanup::AppCleanupState::default();
    // Child listed first proves the shallow-first sort, not input order, wins.
    let value = crate::app_cleanup::plan_app_cleanup(
      &mut state,
      &snapshot,
      &crate::app_cleanup::AppCleanupPlanRequest {
        audit_id: "audit_abcdef1234567890abcdef".into(),
        app_id: "app1".into(),
        paths: vec![
          child.to_string_lossy().into_owned(),
          parent.to_string_lossy().into_owned(),
          parent.to_string_lossy().into_owned(),
        ],
      },
      &[],
    )
    .unwrap();
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "covered descendant must be pruned");
    assert_eq!(entries[0]["path"], parent.to_string_lossy().as_ref());
    assert_eq!(entries[0]["reason"], "staged app location");
    assert_eq!(entries[0]["preset"], serde_json::Value::Null);
    assert_eq!(entries[0]["ignored"], false);
    assert_eq!(value["mode"], "trash");
    assert_eq!(value["auditId"], "audit_abcdef1234567890abcdef");
    assert_eq!(value["appId"], "app1");
    assert!(value["planId"].as_str().unwrap().starts_with("plan_"));
    assert!(value["expiresAt"].as_str().unwrap() > value["createdAt"].as_str().unwrap());
    assert_eq!(value["totalSize"], entries[0]["size"]);
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn app_cleanup_manifest_drift_fails_stale_and_consumes_the_plan() {
    let root = app_cleanup_temp_root("drift");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("data.bin"), vec![b'x'; 4096]).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![target.clone()])],
      &[],
    );
    let mut state = crate::app_cleanup::AppCleanupState::default();
    let plan_id = crate::app_cleanup::plan_app_cleanup(
      &mut state,
      &snapshot,
      &crate::app_cleanup::AppCleanupPlanRequest {
        audit_id: "audit_abcdef1234567890abcdef".into(),
        app_id: "app1".into(),
        paths: vec![target.to_string_lossy().into_owned()],
      },
      &[],
    )
    .unwrap()["planId"]
      .as_str()
      .unwrap()
      .to_string();
    // Grow a grandchild: the target directory's own size/mtime may not move,
    // but the recursive manifest must catch the drift.
    std::fs::write(target.join("data.bin"), vec![b'x'; 8192]).unwrap();
    let problem = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: plan_id.clone(),
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "StalePlan");
    // Consumed plans never run twice, even when the first run refused.
    let replay = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id,
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(replay["problem"]["code"], "NotFound");
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn app_cleanup_plan_expiry_and_confirm_are_enforced() {
    let root = app_cleanup_temp_root("expiry");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("data.bin"), vec![b'x'; 1024]).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![target.clone()])],
      &[],
    );
    let request = crate::app_cleanup::AppCleanupPlanRequest {
      audit_id: "audit_abcdef1234567890abcdef".into(),
      app_id: "app1".into(),
      paths: vec![target.to_string_lossy().into_owned()],
    };
    let mut state = crate::app_cleanup::AppCleanupState::default();
    let plan_id = crate::app_cleanup::plan_app_cleanup(&mut state, &snapshot, &request, &[])
      .unwrap()["planId"]
      .as_str()
      .unwrap()
      .to_string();
    crate::app_cleanup::backdate_plan_for_test(
      &mut state,
      &plan_id,
      std::time::Duration::from_secs(601),
    );
    let problem = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id,
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "StalePlan");

    // confirm:false refuses before the plan is even looked up.
    let plan_id = crate::app_cleanup::plan_app_cleanup(&mut state, &snapshot, &request, &[])
      .unwrap()["planId"]
      .as_str()
      .unwrap()
      .to_string();
    let problem = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: plan_id.clone(),
        confirm: false,
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "InvalidRequest");
    // Unknown plans are NotFound.
    let problem = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: "plan_0000000000000000".into(),
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "NotFound");
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn app_cleanup_execute_returns_the_engine_outcome_shape_and_replays_fail() {
    let root = app_cleanup_temp_root("execute");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("data.bin"), vec![b'x'; 4096]).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![target.clone()])],
      &[],
    );
    let mut state = crate::app_cleanup::AppCleanupState::default();
    let plan_id = crate::app_cleanup::plan_app_cleanup(
      &mut state,
      &snapshot,
      &crate::app_cleanup::AppCleanupPlanRequest {
        audit_id: "audit_abcdef1234567890abcdef".into(),
        app_id: "app1".into(),
        paths: vec![target.to_string_lossy().into_owned()],
      },
      &[],
    )
    .unwrap()["planId"]
      .as_str()
      .unwrap()
      .to_string();
    let outcome = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: plan_id.clone(),
        confirm: true,
      },
    )
    .unwrap();
    // Engine outcome shape field-for-field: { trashed, bytesFreed, failed }.
    assert!(outcome.get("trashed").unwrap().is_array());
    assert!(outcome.get("bytesFreed").unwrap().is_number());
    assert!(outcome.get("failed").unwrap().is_array());
    // Both endings are correct here: `trashed` when the test process may talk
    // to Finder, a typed `failed` entry when it may not (headless CI) —
    // mirroring the engine's real-trash fixture test.
    let attempted =
      outcome["trashed"].as_array().unwrap().len() + outcome["failed"].as_array().unwrap().len();
    assert_eq!(attempted, 1);
    if let Some(failure) = outcome["failed"].as_array().unwrap().first() {
      assert_eq!(failure["code"], "Unavailable");
    }
    // One-shot plan.
    let replay = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id,
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(replay["problem"]["code"], "NotFound");
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn app_cleanup_plan_map_evicts_the_oldest_beyond_eight() {
    let root = app_cleanup_temp_root("evict");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("data.bin"), vec![b'x'; 1024]).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![target.clone()])],
      &[],
    );
    let mut state = crate::app_cleanup::AppCleanupState::default();
    let mut ids = Vec::new();
    for _ in 0..9 {
      let id = crate::app_cleanup::plan_app_cleanup(
        &mut state,
        &snapshot,
        &crate::app_cleanup::AppCleanupPlanRequest {
          audit_id: "audit_abcdef1234567890abcdef".into(),
          app_id: "app1".into(),
          paths: vec![target.to_string_lossy().into_owned()],
        },
        &[],
      )
      .unwrap()["planId"]
        .as_str()
        .unwrap()
        .to_string();
      ids.push(id);
    }
    // The oldest plan was evicted; the newest still executes.
    let evicted = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: ids[0].clone(),
        confirm: true,
      },
    )
    .unwrap_err();
    assert_eq!(evicted["problem"]["code"], "NotFound");
    let latest = ids.last().unwrap().clone();
    let outcome = crate::app_cleanup::execute_app_cleanup(
      &mut state,
      Some(&snapshot),
      &crate::app_cleanup::AppCleanupExecuteRequest {
        plan_id: latest,
        confirm: true,
      },
    )
    .unwrap();
    assert_eq!(
      outcome["trashed"].as_array().unwrap().len() + outcome["failed"].as_array().unwrap().len(),
      1
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn rejected_app_cleanup_replies_on_the_channel_without_the_engine_lock() {
    use std::time::Duration;
    let bundle = std::sync::Arc::new(app_cleanup_bundle());
    // Hold the engine lock for the whole test: any engine-lock acquisition on
    // the worker thread would deadlock and trip the timeout below.
    let _engine_guard = bundle.engine.lock().unwrap();
    let run_on_worker = |bundle: std::sync::Arc<super::SessionBundle>,
                         request: super::SubmitRequest|
     -> serde_json::Value {
      let (done_tx, done_rx) = std::sync::mpsc::channel();
      let channel = tauri::ipc::Channel::<serde_json::Value>::new(move |body| {
        let tauri::ipc::InvokeResponseBody::Json(json) = body else {
          panic!("expected JSON channel reply")
        };
        done_tx
          .send(serde_json::from_str::<serde_json::Value>(&json).unwrap())
          .unwrap();
        Ok(())
      });
      std::thread::spawn(move || {
        super::submit_app_cleanup_reply(&bundle, &[], request, &channel).unwrap();
      });
      done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("app cleanup must reply without the engine lock")
    };

    // No audit yet: stale generation, straight to the channel.
    let reply = run_on_worker(
      bundle.clone(),
      super::SubmitRequest::AppCleanupPlan {
        request: crate::app_cleanup::AppCleanupPlanRequest {
          audit_id: "audit_abcdef1234567890abcdef".into(),
          app_id: "app1".into(),
          paths: vec!["/tmp/x".into()],
        },
      },
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["problem"]["problem"]["code"], "InvalidRequest");

    // Publish a generation, then stage a foreign path: Forbidden, no engine.
    let root = app_cleanup_temp_root("channel");
    let allowed = root.join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    bundle.app_audit.lock().unwrap().audit = Some(app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![allowed])],
      &[],
    ));
    let reply = run_on_worker(
      bundle.clone(),
      super::SubmitRequest::AppCleanupPlan {
        request: crate::app_cleanup::AppCleanupPlanRequest {
          audit_id: "audit_abcdef1234567890abcdef".into(),
          app_id: "app1".into(),
          paths: vec!["/tmp/foreign".into()],
        },
      },
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["problem"]["problem"]["code"], "Forbidden");

    // Unknown execute plan: NotFound, no engine.
    let reply = run_on_worker(
      bundle.clone(),
      super::SubmitRequest::AppCleanupExecute {
        request: crate::app_cleanup::AppCleanupExecuteRequest {
          plan_id: "plan_0000000000000000".into(),
          confirm: true,
        },
      },
    );
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["problem"]["problem"]["code"], "NotFound");
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn app_scan_start_matches_the_closed_ipc_contract() {
    let request: SubmitRequest = serde_json::from_value(serde_json::json!({
      "kind": "appScanStart",
      "auditId": "audit_abcdef1234567890abcdef",
      "appId": "codex-cli",
    }))
    .unwrap();
    assert!(matches!(
      request,
      SubmitRequest::AppScanStart { request } if request.app_id == "codex-cli"
    ));
    // The engine request shape is closed: unknown fields never pass.
    assert!(serde_json::from_value::<SubmitRequest>(serde_json::json!({
      "kind": "appScanStart",
      "auditId": "audit_abcdef1234567890abcdef",
      "appId": "codex-cli",
      "paths": ["/tmp/a"],
    }))
    .is_err());
    assert!(serde_json::from_value::<SubmitRequest>(serde_json::json!({
      "kind": "appScanStart",
      "appId": "codex-cli",
    }))
    .is_err());
    // Engine submits never route to app cleanup and vice versa.
    let result = submit_engine_reply(
      &mut crate::engine::EngineStore::new(Vec::new()),
      &crate::events::EventSink::new(),
      serde_json::from_value(serde_json::json!({
        "kind": "appScanStart",
        "auditId": "audit_abcdef1234567890abcdef",
        "appId": "codex-cli",
      }))
      .unwrap(),
      &tauri::ipc::Channel::<serde_json::Value>::new(|_| Ok(())),
    );
    assert!(result.is_ok());
  }

  #[test]
  fn app_cleanup_submit_and_reveal_match_the_closed_ipc_contract() {
    let plan: super::SubmitRequest = serde_json::from_value(serde_json::json!({
      "kind": "appCleanupPlan",
      "auditId": "audit_abcdef1234567890abcdef",
      "appId": "codex-cli",
      "paths": ["/tmp/a"],
    }))
    .unwrap();
    assert!(matches!(plan, super::SubmitRequest::AppCleanupPlan { .. }));
    let execute: super::SubmitRequest = serde_json::from_value(serde_json::json!({
      "kind": "appCleanupExecute", "planId": "plan_abcdef1234567890", "confirm": true,
    }))
    .unwrap();
    assert!(matches!(
      execute,
      super::SubmitRequest::AppCleanupExecute { request } if request.confirm
    ));
    // Rust accepts a required boolean and refuses `false` at execute time
    // (mirroring the engine); unknown fields never pass the closed union.
    let refused: super::SubmitRequest = serde_json::from_value(serde_json::json!({
      "kind": "appCleanupExecute", "planId": "plan_abcdef1234567890", "confirm": false,
    }))
    .unwrap();
    assert!(matches!(
      refused,
      super::SubmitRequest::AppCleanupExecute { request } if !request.confirm
    ));
    assert!(
      serde_json::from_value::<super::SubmitRequest>(serde_json::json!({
        "kind": "appCleanupPlan",
        "auditId": "audit_abcdef1234567890abcdef",
        "appId": "codex-cli",
        "paths": ["/tmp/a"],
        "mode": "permanent",
      }))
      .is_err()
    );
    let reveal: super::HostRequest = serde_json::from_value(serde_json::json!({
      "kind": "revealPath", "appId": "codex-cli", "path": "/tmp/a",
    }))
    .unwrap();
    assert!(matches!(reveal, super::HostRequest::RevealPath { .. }));
    // The scanned-path reveal is its own arm: one path, no app id.
    let scan_reveal: super::HostRequest = serde_json::from_value(serde_json::json!({
      "kind": "revealScanPath", "path": "/tmp/a",
    }))
    .unwrap();
    assert!(matches!(
      scan_reveal,
      super::HostRequest::RevealScanPath { .. }
    ));
    // The action is optional (it defaults to reveal) but never free-form.
    let scan_reveal_open: super::HostRequest = serde_json::from_value(serde_json::json!({
      "kind": "revealScanPath", "path": "/tmp/a", "action": "open",
    }))
    .unwrap();
    assert!(matches!(
      scan_reveal_open,
      super::HostRequest::RevealScanPath {
        action: Some(crate::reveal::RevealAction::Open),
        ..
      }
    ));
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "revealScanPath", "path": "/tmp/a", "action": "delete",
      }))
      .is_err()
    );
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "revealScanPath",
      }))
      .is_err()
    );
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "revealScanPath", "path": "/tmp/a", "extra": 1,
      }))
      .is_err()
    );
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "revealPath", "appId": "codex-cli",
      }))
      .is_err()
    );
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "revealPath", "appId": "codex-cli", "path": "/tmp/a", "extra": 1,
      }))
      .is_err()
    );
  }

  #[test]
  fn reveal_is_allow_listed_to_the_current_generation() {
    let root = app_cleanup_temp_root("reveal");
    let location = root.join("cache");
    std::fs::create_dir_all(&location).unwrap();
    std::fs::write(location.join("blob.bin"), b"data").unwrap();
    let other = root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let bundle_path = root.join("App1.app");
    std::fs::create_dir_all(&bundle_path).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[
        ("app1", vec![location.clone()]),
        ("app2", vec![other.clone()]),
      ],
      &[("app1", bundle_path.clone())],
    );
    crate::app_cleanup::clear_revealed_for_test();

    // No generation at all: Forbidden, never a raw reveal.
    let problem =
      crate::app_cleanup::reveal_app_location(None, "app1", &location.to_string_lossy())
        .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Foreign path (exists, but not this app's): Forbidden.
    let problem =
      crate::app_cleanup::reveal_app_location(Some(&snapshot), "app1", &other.to_string_lossy())
        .unwrap_err();
    assert_eq!(problem["problem"]["code"], "Forbidden");
    // Missing path: NotFound.
    let problem = crate::app_cleanup::reveal_app_location(
      Some(&snapshot),
      "app1",
      &root.join("missing").to_string_lossy(),
    )
    .unwrap_err();
    assert_eq!(problem["problem"]["code"], "NotFound");
    // Audited location: revealed (the cfg(test) runner records, never spawns).
    let ok =
      crate::app_cleanup::reveal_app_location(Some(&snapshot), "app1", &location.to_string_lossy())
        .unwrap();
    assert_eq!(ok["revealed"], true);
    // Bundle path: allowed for review (not for cleanup).
    let ok = crate::app_cleanup::reveal_app_location(
      Some(&snapshot),
      "app1",
      &bundle_path.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(ok["revealed"], true);
    let revealed = crate::app_cleanup::revealed_for_test();
    assert_eq!(revealed.len(), 2);
    assert!(revealed.contains(&location));
    assert!(revealed.contains(&bundle_path));
    crate::app_cleanup::clear_revealed_for_test();
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn reveal_refuses_symlinked_locations() {
    let root = app_cleanup_temp_root("reveal-link");
    let location = root.join("cache");
    std::fs::create_dir_all(&location).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&location, &link).unwrap();
    let snapshot = app_cleanup_snapshot(
      "audit_abcdef1234567890abcdef",
      &[("app1", vec![location])],
      &[],
    );
    // A symlink leaf never resolves: NotFound, even when it points at real data.
    let problem =
      crate::app_cleanup::reveal_app_location(Some(&snapshot), "app1", &link.to_string_lossy())
        .unwrap_err();
    assert_eq!(problem["problem"]["code"], "NotFound");
    assert!(crate::app_cleanup::revealed_for_test().is_empty());
    std::fs::remove_dir_all(&root).unwrap();
  }

  #[test]
  fn memory_ipc_arms_match_the_closed_union() {
    // Read arms (§4.7): tagged by `method`, camelCase fields.
    for value in [
      serde_json::json!({ "method": "memorySnapshot" }),
      serde_json::json!({ "method": "memoryHistory", "groupIds": ["grp_0123456789abcdef"] }),
      serde_json::json!({ "method": "memoryCandidates", "exclude": ["grp_0123456789abcdef"] }),
      serde_json::json!({ "method": "memoryAudit" }),
    ] {
      assert!(
        serde_json::from_value::<ReadRequest>(value.clone()).is_ok(),
        "read arm should deserialize: {value}"
      );
    }

    // Submit arms: each flattens one frozen request struct.
    for value in [
      serde_json::json!({
        "kind": "memoryTerminatePlan",
        "targets": [{ "kind": "group", "groupId": "grp_0123456789abcdef" }],
        "mode": "graceful",
        "origin": "freeUp",
      }),
      serde_json::json!({
        "kind": "memoryTerminateExecute",
        "planId": "tplan_0123456789ABCDEF",
        "confirm": true,
      }),
    ] {
      assert!(
        serde_json::from_value::<SubmitRequest>(value.clone()).is_ok(),
        "submit arm should deserialize: {value}"
      );
    }

    // Host arm: reopen resolved from an audit id alone.
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "reopenApp", "auditId": "aud_0123456789abcdef",
      }))
      .is_ok()
    );

    // Unknown fields never pass the field-bearing arms, flatten or not. The
    // unit arms (`memorySnapshot`, `memoryAudit`) ignore extra keys exactly as
    // the existing unit methods (`health`, `roots`, …) do.
    assert!(serde_json::from_value::<ReadRequest>(serde_json::json!({
      "method": "memoryHistory", "groupIds": [], "extra": 1,
    }))
    .is_err());
    assert!(serde_json::from_value::<ReadRequest>(serde_json::json!({
      "method": "memoryCandidates", "exclude": [], "extra": 1,
    }))
    .is_err());
    assert!(serde_json::from_value::<SubmitRequest>(serde_json::json!({
      "kind": "memoryTerminatePlan",
      "targets": [{ "kind": "group", "groupId": "grp_0123456789abcdef" }],
      "mode": "graceful", "origin": "manual", "extra": 1,
    }))
    .is_err());
    assert!(serde_json::from_value::<SubmitRequest>(serde_json::json!({
      "kind": "memoryTerminateExecute",
      "planId": "tplan_0123456789ABCDEF", "confirm": true, "extra": 1,
    }))
    .is_err());
    assert!(
      serde_json::from_value::<super::HostRequest>(serde_json::json!({
        "kind": "reopenApp", "auditId": "aud_0123456789abcdef", "extra": 1,
      }))
      .is_err()
    );
  }
}
