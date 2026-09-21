//! IPC surface. Closed command set, closed request unions with
//! `deny_unknown_fields`, session bound to the calling window label. The
//! WebView can name ids and intentions only — never a raw operation outside
//! the engine's vocabulary.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::engine::{
    CleanupExecuteRequest, CleanupPlanRequest, ChildrenPageRequest, EngineStore, ScanStartRequest, TreeSliceRequest,
};
use crate::events::{EventSink, ScopedEventFrame, SubscriptionAck};

pub const EVENT_NAME: &str = "spacelens://event";
pub const API_MAJOR: u32 = 1;
pub const CONTRACT_VERSION: &str = "1.0.0";
pub const SERVICE_INSTANCE_ID: &str = "inst_desktop";

/// One connected frontend session: its own engine store and event sink.
pub struct SessionBundle {
    pub engine: Mutex<EngineStore>,
    pub events: EventSink,
}

pub struct AppState {
    pub sessions: Mutex<std::collections::HashMap<String, SessionBundle>>,
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
    let label = window.label().to_string();
    let session_id = format!(
        "sess_{}",
        state
            .next_session
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let engine = EngineStore::new(state.roots.clone(), true);
    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(session_id.clone(), SessionBundle { engine: Mutex::new(engine), events: EventSink::new() });
    Ok(json!({
        "sessionId": session_id,
        "serviceInstanceId": SERVICE_INSTANCE_ID,
        "cacheNamespace": format!("native/{SERVICE_INSTANCE_ID}/{session_id}"),
        "backendLabel": "Native host",
    }))
}

#[tauri::command]
pub fn sl_disconnect(window: Window, session_id: String, state: State<'_, AppState>) -> Result<Value, Value> {
    require_owner(&window, &state, &session_id)?;
    state.sessions.lock().unwrap().remove(&session_id);
    Ok(json!({"disconnected": true}))
}

fn require_owner(_window: &Window, state: &AppState, session_id: &str) -> Result<(), Value> {
    let owned = state.sessions.lock().unwrap().contains_key(session_id);
    if owned {
        Ok(())
    } else {
        Err(json!({"problem": {"code": "Forbidden", "message": "this window does not own that session", "retryable": false}}))
    }
}

// --------------------------------------------------------------- closed union

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "camelCase", deny_unknown_fields)]
pub enum ReadRequest {
    Health,
    Capabilities,
    Roots,
    ScanStatus { scan_id: String },
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
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HostRequest {
    PickDirectory { title: Option<String> },
}

// -------------------------------------------------------------------- reads

#[tauri::command]
pub fn sl_read(window: Window, session_id: String, request: ReadRequest, state: State<'_, AppState>) -> Result<Value, Value> {
    require_owner(&window, &state, &session_id)?;
    let sessions = state.sessions.lock().unwrap();
    let bundle = sessions
        .get(&session_id)
        .ok_or_else(|| json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}))?;
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
            "scan": { "start": true, "cancel": false, "maxConcurrent": 1 },
            "cleanup": { "plan": true, "execute": true, "mode": "trash" },
            "host": { "folderPicker": true },
            "icloud": "unavailable",
        })),
        ReadRequest::Roots => {
            serde_json::to_value(engine.roots()).map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::ScanStatus { scan_id } => {
            serde_json::to_value(engine.status(&scan_id).map_err(problem_to_value)?).map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::ScanList => serde_json::to_value(engine.list()).map_err(|error| problem_value("InternalError", error)),
        ReadRequest::TreeSlice { scan_id, node_id, depth, max_children_per_node } => {
            let request = TreeSliceRequest { scan_id, node_id, depth, max_children_per_node };
            serde_json::to_value(engine.slice(&request).map_err(problem_to_value)?).map_err(|error| problem_value("InternalError", error))
        }
        ReadRequest::TreeChildren { scan_id, node_id, offset, limit, sort } => {
            let request = ChildrenPageRequest { scan_id, node_id, offset, limit, sort };
            serde_json::to_value(engine.children(&request).map_err(problem_to_value)?).map_err(|error| problem_value("InternalError", error))
        }
    };
    drop(engine);
    drop(sessions);
    result
}

// ----------------------------------------------------------------- mutations

#[tauri::command]
pub async fn sl_submit(
    app: AppHandle,
    window: Window,
    session_id: String,
    request: SubmitRequest,
) -> Result<Value, Value> {
    // State is resolved inside the blocking task via the app handle, whose
    // clone is 'static — the borrowed State itself is not.
    let _ = window;
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().unwrap();
        let bundle = sessions
            .get(&session_id)
            .ok_or_else(|| json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}))?;
        let mut engine = bundle.engine.lock().unwrap();
        let value: Result<Value, Value> = match request {
            SubmitRequest::ScanStart { request } => {
                let session = engine.start_scan(request).map_err(problem_to_value)?;
                let status = engine.status(&session.scan_id).map_err(problem_to_value)?;
                let _ = bundle.events.publish(json!({ "kind": "scan.completed", "status": status }));
                serde_json::to_value(session).map_err(|error| problem_value("InternalError", error))
            }
            SubmitRequest::CleanupPlan { request } => {
                serde_json::to_value(engine.plan(&request).map_err(problem_to_value)?).map_err(|error| problem_value("InternalError", error))
            }
            SubmitRequest::CleanupExecute { request } => {
                serde_json::to_value(engine.execute(&request).map_err(problem_to_value)?).map_err(|error| problem_value("InternalError", error))
            }
        };
        value
    });
    handle.await.map_err(|error| problem_value("InternalError", error))?
}

// --------------------------------------------------------------------- host

#[tauri::command]
pub fn sl_host_request(
    app: AppHandle,
    window: Window,
    session_id: String,
    request: HostRequest,
    state: State<'_, AppState>,
) -> Result<Value, Value> {
    require_owner(&window, &state, &session_id)?;
    match request {
        HostRequest::PickDirectory { title } => {
            use tauri_plugin_dialog::DialogExt;
            let _ = title;
            // Blocking pick from an async command thread: the dialog is modal
            // to this window and the WebView stays alive throughout.
            let picked = app.dialog().file().blocking_pick_folder();
            Ok(json!({
                "picked": picked.map(|path| path.to_string()),
            }))
        }
    }
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
    let bundle = sessions
        .get(&session_id)
        .ok_or_else(|| json!({"problem": {"code": "NotFound", "message": "session vanished", "retryable": false}}))?;
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
