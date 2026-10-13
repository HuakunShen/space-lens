//! App-scoped cleanup outside `EngineStore`: plan/execute for whole audited
//! locations plus Finder reveal, all authorized by a per-audit-generation
//! snapshot (`AuditSnapshot`) swapped in atomically per successful audit.
//!
//! Plan/execute mirrors the engine's discipline: live fingerprints captured at
//! plan time, `confirm: true` required, plans consumed atomically on execute,
//! 10-minute expiry, full re-verification before the first `trash::delete`,
//! and a per-target recheck immediately before each delete. The outcome
//! envelope reuses the engine's `CleanupOutcome` shape field-for-field.
//!
//! Two deliberate strengthenings over the engine baseline:
//! - a directory's top-level size/mtime is not a recursive fingerprint, so the
//!   plan also captures a PRIVATE recursive manifest (capped at 100k entries)
//!   and execute re-verifies it before the first trash call;
//! - staged paths must EXACTLY match locations reported for that app in that
//!   audit generation — never bundle paths, never descendants, never raw IPC
//!   guesses.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::engine::{CleanupOutcome, EntryFingerprint, FailedEntry, PlanEntry, TrashedEntry};
use space_lens::{scan_directory, IgnoredMode, ScanOptions};

/// One successful audit's authorization state. Replaced atomically per audit;
/// a failed audit never publishes a snapshot.
#[derive(Clone, Debug)]
pub struct AuditSnapshot {
  pub audit_id: String,
  /// The serialized report (already carrying top-level `auditId`).
  pub report: Value,
  /// App id -> exact canonical location paths reported for that app.
  pub locations_by_app: HashMap<String, Vec<PathBuf>>,
  /// App id -> canonical `.app` bundle path (reveal allow-list only; bundle
  /// paths are never stageable for cleanup).
  pub app_paths: HashMap<String, PathBuf>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppCleanupPlanRequest {
  pub audit_id: String,
  pub app_id: String,
  pub paths: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppCleanupExecuteRequest {
  pub plan_id: String,
  pub confirm: bool,
}

/// Session-owned app cleanup plans. Bounded: at most 8 live plans, oldest
/// evicted first, so bundles cannot grow without bound across re-audits.
#[derive(Default)]
pub struct AppCleanupState {
  plans: HashMap<String, StoredPlan>,
}

pub struct StoredPlan {
  /// The plan response, retained verbatim (debugging/introspection).
  #[allow(dead_code)]
  pub plan: Value,
  pub audit_id: String,
  pub app_id: String,
  pub targets: Vec<CleanupTarget>,
  /// Private recursive manifest: canonical path string -> (len, mtime_ms).
  pub manifest: HashMap<String, (u64, i64)>,
  pub created_at: Instant,
}

#[derive(Clone, Debug)]
pub struct CleanupTarget {
  pub path: PathBuf,
  pub path_string: String,
  pub size: u64,
  pub fingerprint: EntryFingerprint,
}

/// Cap for the private recursive manifest across one plan.
pub const MAX_MANIFEST_ENTRIES: usize = 100_000;
/// Plan lifetime, matching the engine's 10-minute enforcement.
const PLAN_TTL_SECS: u64 = 600;
/// Live plans retained per session.
const MAX_PLANS: usize = 8;

fn problem_value(code: &str, message: impl std::fmt::Display) -> Value {
  json!({ "problem": { "code": code, "message": message.to_string(), "retryable": false } })
}

pub fn new_audit_id() -> String {
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();
  let pid = std::process::id();
  // `audit_` + 24 hex chars (hex is alphanumeric, satisfying the contract
  // regex `audit_[0-9a-zA-Z]{8,64}`); the counter keeps ids unique within a
  // session even when two audits share a timestamp.
  let mut first = DefaultHasher::new();
  (now.as_secs(), now.subsec_nanos(), pid, counter).hash(&mut first);
  let mut second = DefaultHasher::new();
  (counter, pid, now.subsec_nanos(), now.as_secs()).hash(&mut second);
  format!(
    "audit_{:016x}{:08x}",
    first.finish(),
    second.finish() & 0xFFFF_FFFF
  )
}

fn new_plan_id() -> String {
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();
  let mut hasher = DefaultHasher::new();
  (now.subsec_nanos(), std::process::id(), counter).hash(&mut hasher);
  format!("plan_{:016x}", hasher.finish())
}

/// Minimal RFC3339 formatter (mirrors the engine's dependency-free
/// formatter so the contract's ISO instants match without chrono here).
fn iso_of(when: SystemTime) -> String {
  let now = when.duration_since(UNIX_EPOCH).unwrap_or_default();
  let secs = now.as_secs();
  let millis = now.subsec_millis();
  let days = secs / 86_400;
  let rem = secs % 86_400;
  let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
  // civil-from-days (Howard Hinnant's algorithm)
  let z = days as i64 + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z.rem_euclid(146_097);
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let y = yoe + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let month = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = if month <= 2 { y + 1 } else { y };
  format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Fail closed if a path or any ancestor component was replaced by a link,
/// the canonical form drifted from the registered string, or the destination
/// sits under `Group Containers`. Returns live metadata on success.
fn safe_target_metadata(path: &Path) -> Option<std::fs::Metadata> {
  // Every component from the filesystem root: any symlink rejects the target.
  // (Audit locations live under home, but bundles may not; walking from root
  // covers both without needing a home prefix.)
  let mut prefix = PathBuf::new();
  for component in path.components() {
    prefix.push(component);
    let metadata = std::fs::symlink_metadata(&prefix).ok()?;
    if metadata.file_type().is_symlink() {
      return None;
    }
  }
  let canonical = std::fs::canonicalize(path).ok()?;
  if canonical != path {
    return None;
  }
  if canonical
    .components()
    .any(|component| component.as_os_str() == "Group Containers")
  {
    return None;
  }
  std::fs::symlink_metadata(path).ok()
}

fn fingerprint_of(metadata: &std::fs::Metadata) -> EntryFingerprint {
  let mtime_ms = metadata
    .modified()
    .ok()
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as f64)
    .unwrap_or(0.0);
  EntryFingerprint {
    size: metadata.len(),
    mtime_ms,
  }
}

fn manifest_stamp(metadata: &std::fs::Metadata) -> (u64, i64) {
  let mtime_ms = metadata
    .modified()
    .ok()
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as i64)
    .unwrap_or(0);
  (metadata.len(), mtime_ms)
}

#[derive(Debug, PartialEq)]
enum ManifestError {
  TooLarge,
  Unverifiable,
}

/// Recursive walk collecting (len, mtime_ms) for the target and every
/// descendant. Symlinks are recorded but never followed or descended into.
/// Any unreadable directory or vanished entry fails the whole walk: cleanup
/// must never infer completeness from a partial listing.
fn collect_manifest(
  target: &Path,
  manifest: &mut HashMap<String, (u64, i64)>,
) -> Result<(), ManifestError> {
  fn push(
    path: &Path,
    metadata: &std::fs::Metadata,
    manifest: &mut HashMap<String, (u64, i64)>,
  ) -> Result<(), ManifestError> {
    if manifest.len() >= MAX_MANIFEST_ENTRIES {
      return Err(ManifestError::TooLarge);
    }
    manifest.insert(
      path.to_string_lossy().into_owned(),
      manifest_stamp(metadata),
    );
    Ok(())
  }

  let root_meta = std::fs::symlink_metadata(target).map_err(|_| ManifestError::Unverifiable)?;
  if root_meta.file_type().is_symlink() {
    return Err(ManifestError::Unverifiable);
  }
  push(target, &root_meta, manifest)?;
  if !root_meta.is_dir() {
    return Ok(());
  }
  let mut stack = vec![target.to_path_buf()];
  while let Some(dir) = stack.pop() {
    let entries = std::fs::read_dir(&dir).map_err(|_| ManifestError::Unverifiable)?;
    for entry in entries {
      let entry = entry.map_err(|_| ManifestError::Unverifiable)?;
      let path = entry.path();
      let metadata = std::fs::symlink_metadata(&path).map_err(|_| ManifestError::Unverifiable)?;
      push(&path, &metadata, manifest)?;
      if metadata.is_dir() && !metadata.file_type().is_symlink() {
        stack.push(path);
      }
    }
  }
  Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppCleanupPlanResponse {
  plan_id: String,
  audit_id: String,
  app_id: String,
  mode: &'static str,
  entries: Vec<PlanEntry>,
  total_size: u64,
  errors: Vec<String>,
  created_at: String,
  expires_at: String,
}

/// Stage whole audited locations of one app in one audit generation for
/// trash cleanup. Never touches the engine lock; the caller holds only the
/// app-audit snapshot (cloned) and the app-cleanup state.
pub fn plan_app_cleanup(
  state: &mut AppCleanupState,
  snapshot: &AuditSnapshot,
  req: &AppCleanupPlanRequest,
) -> Result<Value, Value> {
  if req.audit_id != snapshot.audit_id {
    return Err(problem_value(
      "InvalidRequest",
      "audit is stale; re-run the audit",
    ));
  }
  if req.app_id.is_empty() || req.app_id.len() > 128 {
    return Err(problem_value(
      "InvalidRequest",
      "appId must be between 1 and 128 characters",
    ));
  }
  if req.paths.is_empty() || req.paths.len() > 200 {
    return Err(problem_value(
      "InvalidRequest",
      "cleanup stages between 1 and 200 paths",
    ));
  }
  let Some(reported) = snapshot.locations_by_app.get(&req.app_id) else {
    return Err(problem_value(
      "Forbidden",
      "only audited locations of the selected app can be staged",
    ));
  };
  let reported_strings: HashSet<String> = reported
    .iter()
    .map(|path| path.to_string_lossy().into_owned())
    .collect();
  // Exact canonical-string match only: bundle paths, descendants, and other
  // apps' locations are all refused here.
  let mut staged: Vec<String> = Vec::new();
  let mut seen: HashSet<String> = HashSet::new();
  for path in &req.paths {
    if !reported_strings.contains(path) {
      return Err(problem_value(
        "Forbidden",
        "only audited locations of the selected app can be staged",
      ));
    }
    if seen.insert(path.clone()) {
      staged.push(path.clone());
    }
  }
  // Prune descendants covered by a staged ancestor (mirror engine.plan's
  // nesting drop): shallowest first, keep unless already covered.
  staged.sort_by(|a, b| {
    Path::new(a)
      .components()
      .count()
      .cmp(&Path::new(b).components().count())
      .then_with(|| a.cmp(b))
  });
  let mut retained: Vec<PathBuf> = Vec::new();
  for path in &staged {
    let candidate = PathBuf::from(path);
    if !retained.iter().any(|kept| candidate.starts_with(kept)) {
      retained.push(candidate);
    }
  }
  // Fail closed per the audit path-safety rules before measuring anything.
  let mut fingerprints: HashMap<PathBuf, EntryFingerprint> = HashMap::new();
  for target in &retained {
    let metadata = safe_target_metadata(target).ok_or_else(|| {
      problem_value(
        "StalePlan",
        format!(
          "path is missing, traverses a symbolic link, or left its audited location: {}",
          target.display()
        ),
      )
    })?;
    fingerprints.insert(target.clone(), fingerprint_of(&metadata));
  }
  // ONE shared-inode pass, deepest-first: partition sizes with no double
  // counting (they were pruned above, so plain partitions are right).
  let mut deepest = retained.clone();
  deepest.sort_by(|a, b| {
    b.components()
      .count()
      .cmp(&a.components().count())
      .then_with(|| a.cmp(b))
  });
  let nodes = scan_directory(ScanOptions {
    directories: deepest,
    ignore_hidden: false,
    full_path: true,
    respect_gitignore: false,
    ignored_mode: IgnoredMode::Exclude,
    follow_symlinks: false,
  });
  let mut sizes: HashMap<PathBuf, u64> = HashMap::with_capacity(nodes.len());
  for node in nodes {
    sizes.insert(node.path.clone(), node.size);
  }
  let mut targets: Vec<CleanupTarget> = Vec::with_capacity(retained.len());
  for target in &retained {
    let Some(size) = sizes.get(target).copied() else {
      return Err(problem_value(
        "StalePlan",
        format!(
          "path changed since the audit and cannot be measured: {}; re-run the audit",
          target.display()
        ),
      ));
    };
    let path_string = target.to_string_lossy().into_owned();
    targets.push(CleanupTarget {
      path: target.clone(),
      path_string,
      size,
      fingerprint: fingerprints[target].clone(),
    });
  }
  // Private recursive manifest across the whole plan.
  let mut manifest: HashMap<String, (u64, i64)> = HashMap::new();
  for target in &retained {
    match collect_manifest(target, &mut manifest) {
      Ok(()) => {}
      Err(ManifestError::TooLarge) => {
        return Err(problem_value(
          "InvalidRequest",
          "location too large to verify safely",
        ));
      }
      Err(ManifestError::Unverifiable) => {
        return Err(problem_value(
          "InvalidRequest",
          format!(
            "location cannot be verified safely; re-run the audit: {}",
            target.display()
          ),
        ));
      }
    }
  }
  let entries: Vec<PlanEntry> = targets
    .iter()
    .map(|target| PlanEntry {
      path: target.path_string.clone(),
      size: target.size,
      reason: "staged app location".into(),
      preset: None,
      ignored: false,
      fingerprint: target.fingerprint.clone(),
    })
    .collect();
  let total_size = entries.iter().map(|entry| entry.size).sum();
  let plan_id = new_plan_id();
  let response = AppCleanupPlanResponse {
    plan_id: plan_id.clone(),
    audit_id: snapshot.audit_id.clone(),
    app_id: req.app_id.clone(),
    mode: "trash",
    entries,
    total_size,
    errors: Vec::new(),
    created_at: iso_of(SystemTime::now()),
    expires_at: iso_of(SystemTime::now() + Duration::from_secs(PLAN_TTL_SECS)),
  };
  let value =
    serde_json::to_value(&response).map_err(|error| problem_value("InternalError", error))?;
  while state.plans.len() >= MAX_PLANS {
    let oldest = state
      .plans
      .iter()
      .min_by_key(|(_, stored)| stored.created_at)
      .map(|(id, _)| id.clone());
    match oldest {
      Some(id) => {
        state.plans.remove(&id);
      }
      None => break,
    }
  }
  state.plans.insert(
    plan_id,
    StoredPlan {
      plan: value.clone(),
      audit_id: snapshot.audit_id.clone(),
      app_id: req.app_id.clone(),
      targets,
      manifest,
      created_at: Instant::now(),
    },
  );
  Ok(value)
}

/// Execute a staged app cleanup plan: consume atomically, enforce expiry and
/// the current audit generation, re-verify fingerprints AND the private
/// manifest before the first trash call, then trash with a per-target recheck
/// just before each delete. Returns the engine-shaped `CleanupOutcome`.
pub fn execute_app_cleanup(
  state: &mut AppCleanupState,
  snapshot: Option<&AuditSnapshot>,
  req: &AppCleanupExecuteRequest,
) -> Result<Value, Value> {
  if !req.confirm {
    return Err(problem_value(
      "InvalidRequest",
      "cleanup requires confirm: true",
    ));
  }
  // Atomically consumed: removed before executing, so replays and races fail.
  let stored = state
    .plans
    .remove(&req.plan_id)
    .ok_or_else(|| problem_value("NotFound", format!("no plan {}", req.plan_id)))?;
  if stored.created_at.elapsed().as_secs() > PLAN_TTL_SECS {
    return Err(problem_value(
      "StalePlan",
      "plan expired; plan again from a fresh audit",
    ));
  }
  let Some(snapshot) = snapshot else {
    return Err(problem_value(
      "StalePlan",
      "audit is stale; re-run the audit",
    ));
  };
  if stored.audit_id != snapshot.audit_id {
    return Err(problem_value(
      "StalePlan",
      "audit is stale; re-run the audit",
    ));
  }
  let reported_strings: HashSet<String> = snapshot
    .locations_by_app
    .get(&stored.app_id)
    .map(|paths| {
      paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
    })
    .unwrap_or_default();
  for target in &stored.targets {
    // Still the same audited location in the CURRENT generation, and still
    // path-safe right now.
    if !reported_strings.contains(&target.path_string) {
      return Err(problem_value(
        "StalePlan",
        format!(
          "path is no longer an audited location of this app: {}",
          target.path_string
        ),
      ));
    }
    if safe_target_metadata(&target.path).is_none() {
      return Err(problem_value(
        "StalePlan",
        format!(
          "path is missing, traverses a symbolic link, or left its audited location: {}",
          target.path_string
        ),
      ));
    }
  }
  let mut changed = 0usize;
  for target in &stored.targets {
    let matches = safe_target_metadata(&target.path)
      .map(|metadata| fingerprint_of(&metadata) == target.fingerprint)
      .unwrap_or(false);
    if !matches {
      changed += 1;
    }
  }
  if changed > 0 {
    return Err(problem_value(
      "StalePlan",
      format!(
        "{changed} of {} entries changed since the plan was made",
        stored.targets.len()
      ),
    ));
  }
  // Re-verify the private recursive manifest before the FIRST trash call: a
  // changed grandchild can leave top-level fingerprints untouched.
  let mut current: HashMap<String, (u64, i64)> = HashMap::new();
  for target in &stored.targets {
    if collect_manifest(&target.path, &mut current).is_err() {
      return Err(problem_value(
        "StalePlan",
        "locations changed since the plan was made; plan again",
      ));
    }
  }
  if current != stored.manifest {
    return Err(problem_value(
      "StalePlan",
      "locations changed since the plan was made; plan again",
    ));
  }
  let mut outcome = CleanupOutcome {
    trashed: Vec::new(),
    bytes_freed: 0,
    failed: Vec::new(),
  };
  for target in &stored.targets {
    // Immediate recheck just before this target's delete; drift marks that
    // entry failed and continues with the rest.
    let fresh = safe_target_metadata(&target.path)
      .map(|metadata| fingerprint_of(&metadata) == target.fingerprint)
      .unwrap_or(false);
    if !fresh {
      outcome.failed.push(FailedEntry {
        path: target.path_string.clone(),
        code: "StalePlan".into(),
        message: format!("path changed before deletion: {}", target.path_string),
      });
      continue;
    }
    match trash::delete(&target.path) {
      Ok(()) => {
        outcome.bytes_freed += target.size;
        outcome.trashed.push(TrashedEntry {
          path: target.path_string.clone(),
          size: target.size,
        });
      }
      Err(error) => outcome.failed.push(FailedEntry {
        path: target.path_string.clone(),
        code: "Unavailable".into(),
        message: format!("trash failed: {error}"),
      }),
    }
  }
  serde_json::to_value(&outcome).map_err(|error| problem_value("InternalError", error))
}

/// Reveal an audited app location (or the app's own bundle) in Finder via
/// `open -R`. The path must canonicalize to exactly a reported location of
/// the app or its bundle path in the current generation — nothing else.
pub fn reveal_app_location(
  snapshot: Option<&AuditSnapshot>,
  app_id: &str,
  path: &str,
) -> Result<Value, Value> {
  let Some(snapshot) = snapshot else {
    return Err(problem_value(
      "Forbidden",
      "only audited locations of the selected app can be revealed",
    ));
  };
  // Fail closed on symlinks and missing paths, mirroring the icon read.
  if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
    return Err(problem_value(
      "NotFound",
      format!("no such location: {path}"),
    ));
  }
  let canonical = std::fs::canonicalize(path)
    .map_err(|_| problem_value("NotFound", format!("no such location: {path}")))?;
  let location_hit = snapshot
    .locations_by_app
    .get(app_id)
    .is_some_and(|paths| paths.contains(&canonical));
  let bundle_hit = snapshot
    .app_paths
    .get(app_id)
    .is_some_and(|bundle| *bundle == canonical);
  if !location_hit && !bundle_hit {
    return Err(problem_value(
      "Forbidden",
      "only audited locations of the selected app can be revealed",
    ));
  }
  open_in_finder(&canonical).map_err(|error| problem_value("InternalError", error))?;
  Ok(json!({ "revealed": true }))
}

#[cfg(test)]
fn open_in_finder(canonical: &Path) -> Result<(), String> {
  REVEALED_TEST_HOOK
    .lock()
    .unwrap()
    .push(canonical.to_path_buf());
  Ok(())
}

#[cfg(not(test))]
fn open_in_finder(canonical: &Path) -> Result<(), String> {
  // Spawn and ignore output; never a shell, never interpolated.
  std::process::Command::new("open")
    .arg("-R")
    .arg(canonical)
    .spawn()
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[cfg(test)]
static REVEALED_TEST_HOOK: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Test hook: paths the cfg(test) reveal runner was asked to open.
#[cfg(test)]
pub fn revealed_for_test() -> Vec<PathBuf> {
  REVEALED_TEST_HOOK.lock().unwrap().clone()
}

/// Test hook: clear recorded reveal invocations.
#[cfg(test)]
pub fn clear_revealed_for_test() {
  REVEALED_TEST_HOOK.lock().unwrap().clear();
}

/// Test hook: backdate a live plan so expiry can be exercised deterministically.
#[cfg(test)]
pub fn backdate_plan_for_test(state: &mut AppCleanupState, plan_id: &str, age: Duration) {
  if let Some(stored) = state.plans.get_mut(plan_id) {
    stored.created_at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
  }
}

/// Test hook: number of live plans.
#[cfg(test)]
pub fn plan_count_for_test(state: &AppCleanupState) -> usize {
  state.plans.len()
}
