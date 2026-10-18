//! Desktop wiring for the memory service (T16 of `docs/memory-cleaner-plan.md`).
//!
//! [`MemoryState`] owns the one in-process [`MemoryService`] every window
//! shares: the sampler thread, the history rings, the plan store and the audit
//! ring. The IPC arms in [`crate::commands`] delegate here, and every failure is
//! rendered in the existing problem JSON shape from [`MemoryError::problem_code`]
//! — the memory crate invents no new codes (§4.1).
//!
//! The audit file lives beside the app's other data
//! (`<app data>/memory-audit.jsonl`); when that directory cannot be resolved the
//! service still runs and simply keeps its in-memory ring. Reopen resolves the
//! bundle id from the audit record alone and only ever runs `/usr/bin/open -b`
//! for an identifier matching the allow-listed shape (§4.7), so a raw IPC
//! caller cannot use it to open an arbitrary path.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use std::process::Command;

use serde_json::{json, Value};
use spacelens_memory::sampler::{Clock, Sleeper};
use spacelens_memory::{
  CandidatesRequest, GroupId, HistoryRequest, MemoryError, MemoryService, Platform,
  TerminateExecuteRequest, TerminatePlanRequest,
};
use tauri::Manager;

/// The audit file name inside the app data directory.
const AUDIT_FILE_NAME: &str = "memory-audit.jsonl";

/// The desktop's memory service, managed once and read by every IPC arm.
pub struct MemoryState {
  /// The live service; shared through the managed state and every window.
  pub service: Arc<MemoryService>,
}

impl MemoryState {
  /// Start the in-process service with the platform defaults and the desktop
  /// audit file.
  ///
  /// The sampler, application directory and signaller are the crate's defaults
  /// for this build (`appkit` is enabled on macOS by this crate's manifest), so
  /// the desktop names no platform type itself. The audit file is best effort:
  /// a failure to resolve the app data directory leaves [`None`] and the service
  /// keeps the in-memory ring only.
  pub fn new(app: &tauri::App) -> Self {
    let audit_file = app.path().app_data_dir().ok().map(|dir| {
      let _ = std::fs::create_dir_all(&dir);
      dir.join(AUDIT_FILE_NAME)
    });
    let service = MemoryService::start(
      spacelens_memory::platform::default_sampler(),
      spacelens_memory::platform::default_apps(),
      spacelens_memory::platform::default_signaller(),
      Arc::new(SystemClock),
      Arc::new(ThreadSleeper),
      audit_file,
    );
    Self { service }
  }
}

/// The `memory` capability object the desktop advertises (§4.4).
///
/// `sample` and `terminate` mirror the service's own greeting, so the desktop
/// can never claim an operation the running sampler cannot perform;
/// `gracefulAppQuit` comes from the same greeting (the memory crate knows
/// whether its `appkit` feature is on). `reopen` is desktop-plus-macOS only.
pub fn capability(state: &MemoryState) -> Value {
  let hello = state.service.capabilities();
  // §4.4: the desktop samples on macOS and Linux only (Windows is v3), so the
  // capability follows the sampler, never a compile-time guess.
  let sample = matches!(hello.platform, Platform::Macos | Platform::Linux);
  json!({
    "sample": sample,
    "terminate": hello.terminate,
    "gracefulAppQuit": hello.graceful_app_quit,
    "reopen": cfg!(target_os = "macos"),
    "platform": hello.platform,
  })
}

/// The latest snapshot (§4.5). Renews the service lease and blocks up to two
/// seconds for the first sample.
pub fn read_snapshot(state: &MemoryState) -> Result<Value, Value> {
  let snapshot = state.service.snapshot().map_err(problem)?;
  serde_json::to_value(&*snapshot).map_err(internal)
}

/// The downsampled history for the requested groups.
pub fn read_history(state: &MemoryState, group_ids: &[String]) -> Result<Value, Value> {
  let request = HistoryRequest {
    group_ids: parse_group_ids(group_ids)?,
  };
  let history = state.service.history(&request).map_err(problem)?;
  serde_json::to_value(&history).map_err(internal)
}

/// The free-up candidates for the latest snapshot, excluding `exclude`.
pub fn read_candidates(state: &MemoryState, exclude: &[String]) -> Result<Value, Value> {
  let request = CandidatesRequest {
    exclude: parse_group_ids(exclude)?,
  };
  let candidates = state.service.candidates(&request).map_err(problem)?;
  serde_json::to_value(&candidates).map_err(internal)
}

/// The audit ring, newest first.
pub fn read_audit(state: &MemoryState) -> Result<Value, Value> {
  serde_json::to_value(state.service.audit()).map_err(internal)
}

/// Build a single-use termination plan bound to the desktop session.
///
/// The request is the frozen [`TerminatePlanRequest`] the IPC arm deserialized;
/// nothing is re-shaped here.
pub fn submit_plan(
  state: &MemoryState,
  session_id: &str,
  request: &TerminatePlanRequest,
) -> Result<Value, Value> {
  let plan = state.service.plan(session_id, request).map_err(problem)?;
  serde_json::to_value(&plan).map_err(internal)
}

/// Execute a plan once, on the caller's blocking thread. The service waits out
/// the graceful/force window itself, so this runs inside
/// `tauri::async_runtime::spawn_blocking` (see `sl_submit`), never the main
/// thread.
pub fn submit_execute(
  state: &MemoryState,
  session_id: &str,
  request: &TerminateExecuteRequest,
) -> Result<Value, Value> {
  let outcome = state
    .service
    .execute(session_id, request)
    .map_err(problem)?;
  serde_json::to_value(&outcome).map_err(internal)
}

/// Reopen the application named by an audit record, and nothing else.
///
/// The bundle id is read from the record alone; a missing record, a record with
/// no bundle id, or a bundle id outside the allow-listed shape all answer
/// `{"reopened": false}` rather than opening anything.
pub fn reopen_app(state: &MemoryState, audit_id: &str) -> Result<Value, Value> {
  let audit = state.service.audit();
  let bundle_id = audit
    .records
    .iter()
    .find(|record| record.audit_id.as_str() == audit_id)
    .and_then(|record| record.bundle_id.as_deref());
  let Some(bundle_id) = bundle_id else {
    return Ok(json!({ "reopened": false }));
  };
  if !valid_bundle_id(bundle_id) {
    return Ok(json!({ "reopened": false }));
  }
  Ok(json!({ "reopened": open_bundle(bundle_id) }))
}

/// Parse a list of wire group ids, refusing a malformed one.
fn parse_group_ids(ids: &[String]) -> Result<Vec<GroupId>, Value> {
  ids
    .iter()
    .cloned()
    .map(|id| GroupId::try_from(id).map_err(problem))
    .collect()
}

/// The bundle id shape `/usr/bin/open -b` may receive: `^[A-Za-z0-9][A-Za-z0-9.-]{0,254}$`.
fn valid_bundle_id(value: &str) -> bool {
  let bytes = value.as_bytes();
  if bytes.is_empty() || bytes.len() > 255 {
    return false;
  }
  if !bytes[0].is_ascii_alphanumeric() {
    return false;
  }
  bytes
    .iter()
    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'-')
}

/// Ask Launch Services to open the bundle, returning whether it succeeded.
#[cfg(target_os = "macos")]
fn open_bundle(bundle_id: &str) -> bool {
  Command::new("/usr/bin/open")
    .arg("-b")
    .arg(bundle_id)
    .status()
    .map(|status| status.success())
    .unwrap_or(false)
}

/// Reopen is a macOS capability; elsewhere the request is answered, not opened.
#[cfg(not(target_os = "macos"))]
fn open_bundle(_bundle_id: &str) -> bool {
  false
}

/// Render a memory failure as the existing problem object (§4.1). The caller
/// wraps it as `{ "ok": false, "problem": … }`, matching the engine's shape.
fn problem(error: MemoryError) -> Value {
  json!({
    "problem": {
      "code": error.problem_code(),
      "message": error.to_string(),
      "retryable": error.retryable(),
    }
  })
}

/// An internal serialization failure, in the same problem shape.
fn internal(error: impl std::fmt::Display) -> Value {
  json!({
    "problem": {
      "code": "InternalError",
      "message": error.to_string(),
      "retryable": false,
    }
  })
}

/// The wall clock the service paces its sampler thread with.
struct SystemClock;

impl Clock for SystemClock {
  fn now_ms(&self) -> i64 {
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
      .unwrap_or(0)
  }
}

/// The sleeper the service paces its sampler thread with.
struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
  fn sleep_ms(&self, ms: u64) {
    thread::sleep(Duration::from_millis(ms));
  }
}

#[cfg(test)]
mod tests {
  use super::valid_bundle_id;

  #[test]
  fn bundle_id_allow_list_matches_the_contract() {
    for good in ["com.apple.TextEdit", "com.apple.Safari", "a", "A1-b2.c3"] {
      assert!(valid_bundle_id(good), "{good} should be allowed");
    }
    for bad in [
      "",
      "-com.apple.Safari",
      ".com.apple.Safari",
      "com/apple/Safari",
      "com apple Safari",
      "com.apple.Safari;open /etc/passwd",
    ] {
      assert!(!valid_bundle_id(bad), "{bad} should be refused");
    }
    let longest = format!("a{}", "b".repeat(254));
    assert!(valid_bundle_id(&longest));
    let too_long = format!("a{}", "b".repeat(255));
    assert!(!valid_bundle_id(&too_long));
  }
}
