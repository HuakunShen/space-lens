//! IPC surface. Closed command set, closed request unions with
//! `deny_unknown_fields`, session bound to the calling window label. The
//! WebView can name ids and intentions only — never a raw operation outside
//! the engine's vocabulary.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::engine::{
  default_discovery_limit, run_discovery, ChildrenPageRequest, CleanupExecuteRequest,
  CleanupPlanRequest, DiscoveryData, DiscoveryInput, DiscoveryKind, DiscoveryPage,
  DiscoveryRequest, EngineProblem, EngineStore, LocalScanTask, ScanStartRequest, TreeSliceRequest,
};
use crate::events::{EventSink, ScopedEventFrame, SubscriptionAck};

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
  let engine = EngineStore::new(state.roots.clone(), true);
  sessions.insert(
    session_id.clone(),
    Arc::new(SessionBundle {
      window_label: window_label.to_string(),
      engine: Mutex::new(engine),
      events: EventSink::new(),
      discovery: Mutex::new(()),
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
}

#[derive(Deserialize)]
#[serde(
  tag = "kind",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum HostRequest {
  PickDirectory { title: Option<String> },
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
      let mut engine = bundle.engine.lock().unwrap();
      let result: Result<Value, Value> = match request {
        ReadRequest::Health => Ok(json!({
            "status": "ok",
            "serviceInstanceId": SERVICE_INSTANCE_ID,
            "apiMajor": API_MAJOR,
            "contractVersion": CONTRACT_VERSION,
        })),
        ReadRequest::Capabilities => Ok(json!({
            "apiMajor": API_MAJOR,
            "contractVersion": CONTRACT_VERSION,
            "scan": { "start": true, "cancel": true, "maxConcurrent": 1, "discovery": true },
            "cleanup": { "plan": true, "execute": true, "mode": "trash" },
            "host": { "folderPicker": true },
            "icloud": "unavailable",
        })),
        ReadRequest::Roots => {
          // wrapped to match the HTTP host shape: { roots: ScanTarget[] }
          serde_json::to_value(json!({ "roots": engine.roots() }))
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

fn read_discovery(
  bundle: &SessionBundle,
  request: &DiscoveryRequest,
  walk: impl FnOnce(DiscoveryInput) -> Result<DiscoveryData, EngineProblem>,
) -> Result<DiscoveryPage, EngineProblem> {
  // A second view waits for the first walk to populate every discovery kind,
  // then checks the cache. Neither the engine nor global session lock is
  // held while waiting or walking.
  let _discovery = bundle.discovery.lock().unwrap();
  // Snapshot inputs while locked, walk without either engine/session lock,
  // then register the immutable result before replying.
  let input = bundle.engine.lock().unwrap().discovery_input(request)?;
  let data = input.map(walk).transpose()?;
  bundle
    .engine
    .lock()
    .unwrap()
    .finish_discovery(request, data)
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
      SubmitRequest::ScanStart { request } if request.local_only => {
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
  let mut engine = bundle.engine.lock().unwrap();
  let was_scanning = engine
    .status(&scan_id)
    .is_ok_and(|status| status.state == "scanning");
  let status = engine.finish_local_scan(&scan_id, result);
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
    let value: Value = match request {
      HostRequest::PickDirectory { title } => {
        use tauri_plugin_dialog::DialogExt;
        let _ = title;
        // Blocking pick from a blocking-pool thread (never the main
        // thread): the dialog is modal to this window and the
        // WebView stays alive throughout.
        let picked = app.dialog().file().blocking_pick_folder();
        json!({
            "picked": picked.map(|path| path.to_string()),
        })
      }
    };
    let _ = reply.send(json!({ "ok": true, "result": value }));
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
  let (subscription_id, mut ack) = bundle.events.subscribe(after_sequence);
  ack.service_instance_id = SERVICE_INSTANCE_ID.to_string();
  let frames: Vec<ScopedEventFrame> = bundle
    .events
    .frames_for(&session_id, &subscription_id, SERVICE_INSTANCE_ID)
    .into_iter()
    .filter(|frame| after_sequence.map_or(true, |since| frame.event.sequence > since))
    .collect();
  drop(sessions);
  for frame in frames {
    let _ = app.emit_to(window.label(), EVENT_NAME, frame);
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
  // nothing to release server-side today; the ack confirms the pairing
  let _ = &app;
  let _ = subscription_id;
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
  fn protected_native_worker_finishes_a_local_fixture_and_discovery_cannot_rescan() {
    let path = std::env::temp_dir().join(format!("sl-native-worker-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("file"), vec![b'x'; 8192]).unwrap();
    let path = std::fs::canonicalize(path).unwrap();
    let mut engine = crate::engine::EngineStore::new(vec![path.clone()], true);
    let request =
      serde_json::from_value(serde_json::json!({ "paths": [path], "localOnly": true })).unwrap();
    let (session, task) = engine.prepare_local_scan(request).unwrap();
    let bundle = super::SessionBundle {
      window_label: "fixture".into(),
      engine: std::sync::Mutex::new(engine),
      events: crate::events::EventSink::new(),
      discovery: std::sync::Mutex::new(()),
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
    let error = super::read_discovery(
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
    .unwrap_err();
    assert_eq!(error.code, "Unavailable");
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
    let mut engine = crate::engine::EngineStore::new(vec![path.clone()], true);
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
    let mut engine = crate::engine::EngineStore::new(Vec::new(), true);
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
}
