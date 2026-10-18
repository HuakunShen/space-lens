//! The memory service and its sampler thread (T10 of
//! docs/memory-cleaner-plan.md). [`MemoryService`] owns the live state — the
//! latest snapshot, the history rings, the plan store and the audit ring — and
//! a background thread that ticks the injected sampler while a caller holds a
//! lease. Every consumer (`snapshot`, `history`, `candidates`, `audit`, `plan`,
//! `execute`) runs against the one snapshot the thread last published, so a plan
//! always resolves against the same process identities the view showed.
//!
//! The thread samples through the [`ProcessSampler`] seam and builds the wire
//! snapshot exactly as §4.2 and §5 require: sample → [`build_groups`] →
//! [`safety::apply`] → snapshot (`sequence` incremented) → [`History::push`].
//! A lease lets the thread idle when nothing is watching: [`MemoryService::snapshot`]
//! renews it for ten seconds and wakes the thread, which parks on the condvar
//! once it expires. Pacing between ticks uses the injected [`Sleeper`], so tests
//! advance a fake clock instead of waiting on the wall clock.
//!
//! Nothing here names a platform type: the sampler, application directory and
//! signaller are all supplied by the caller. Audit records are appended as JSONL
//! to an optional file, rotated to a `.1` sibling at one mebibyte.

use crate::candidates::candidates;
use crate::error::MemoryError;
use crate::group::build_groups;
use crate::history::History;
use crate::ids::AuditId;
use crate::model::{
  AuditLog, AuditRecord, CandidatesRequest, FreeUpCandidates, HistoryRequest, MemoryBreakdown,
  MemoryHistory, MemorySnapshot, Platform, PressureSource, SnapshotNote, TerminateExecuteRequest,
  TerminateOutcome, TerminatePlan, TerminatePlanRequest,
};
use crate::plan::{AuditRing, PlanStore, SnapshotIndex};
use crate::protocol::{Hello, PROTOCOL_VERSION};
use crate::raw::{RawSample, RawSystem};
use crate::safety::{self, SafetyContext};
use crate::sampler::{AppDirectory, Clock, ProcessSampler, Signaller, Sleeper};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How long a caller's interest keeps the sampler thread awake.
const LEASE_MS: i64 = 10_000;
/// How long the first [`MemoryService::snapshot`] waits for a sample.
const FIRST_SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);
/// The lowest sampling interval the UI may select.
pub const MIN_INTERVAL_MS: u32 = 1_000;
/// The highest sampling interval the UI may select.
pub const MAX_INTERVAL_MS: u32 = 5_000;
/// The interval the service starts at.
pub const DEFAULT_INTERVAL_MS: u32 = 1_000;
/// The most groups one snapshot carries (§4.1).
const MAX_GROUPS: usize = 5_000;
/// The most process rows one snapshot carries (§4.1).
const MAX_PROCESSES: usize = 20_000;
/// The size at which the audit file rotates to its `.1` sibling.
const AUDIT_MAX_BYTES: u64 = 1024 * 1024;
/// The prefix of a plan identifier.
const PLAN_ID_PREFIX: &str = "tplan_";
/// The prefix of an audit identifier.
const AUDIT_ID_PREFIX: &str = "aud_";
/// The identifier body length and alphabet shared by plan and audit ids.
const ID_BODY_LEN: usize = 16;
const ID_ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// A generator of `[0-9a-zA-Z]` identifier bodies that needs no extra
/// dependency: `RandomState` supplies per-process random hash keys and a
/// monotonic counter keeps every draw distinct, so two services never collide.
#[derive(Debug)]
struct IdGenerator {
  keys: RandomState,
  counter: u64,
}

impl IdGenerator {
  fn new() -> Self {
    IdGenerator {
      keys: RandomState::new(),
      counter: 0,
    }
  }

  /// The next identifier with `prefix` followed by [`ID_BODY_LEN`] random
  /// alphanumeric characters.
  fn next(&mut self, prefix: &str) -> String {
    let mut id = String::with_capacity(prefix.len() + ID_BODY_LEN);
    id.push_str(prefix);
    for index in 0..ID_BODY_LEN as u64 {
      let mut hasher = self.keys.build_hasher();
      hasher.write_u64(self.counter);
      hasher.write_u64(index);
      let value = hasher.finish();
      let slot = (value % ID_ALPHABET.len() as u64) as usize;
      id.push(char::from(ID_ALPHABET[slot]));
    }
    self.counter = self.counter.wrapping_add(1);
    id
  }
}

/// Everything the sampler thread publishes and every consumer reads, behind one
/// mutex so a snapshot, its raw rows and its safety context always agree.
struct State {
  apps: Box<dyn AppDirectory>,
  signaller: Box<dyn Signaller>,
  audit_file: Option<PathBuf>,
  platform: Platform,
  terminate: bool,
  graceful_app_quit: bool,
  interval_ms: u32,
  lease_until_ms: i64,
  shutdown: bool,
  sequence: u64,
  snapshot: Option<Arc<MemorySnapshot>>,
  raw: Vec<crate::raw::RawProcess>,
  safety: Option<SafetyContext>,
  last_frontmost_ms: HashMap<crate::ids::GroupId, i64>,
  history: History,
  plans: PlanStore,
  audit: AuditRing,
  ids: IdGenerator,
  last_error: Option<String>,
}

/// The memory service: the shared state, the condvar that wakes the sampler
/// thread and the thread's detached handle.
pub struct MemoryService {
  clock: Arc<dyn Clock>,
  sleeper: Arc<dyn Sleeper>,
  state: Mutex<State>,
  wake: Condvar,
  /// Kept so the handle outlives the service; the thread is detached and exits
  /// with the process.
  #[allow(dead_code)]
  sampler_thread: Mutex<Option<JoinHandle<()>>>,
}

impl MemoryService {
  /// Start the sampler thread around the injected seams.
  ///
  /// The caller supplies every operating-system dependency, so the service can
  /// be built in a test with fakes: `sampler` reads the machine, `apps` knows
  /// the applications and can ask one to quit, `signaller` reads fresh
  /// identities and delivers signals, and `clock`/`sleeper` pace the ticks.
  /// `audit_file`, when set, receives every executed termination as JSONL.
  pub fn start(
    sampler: Box<dyn ProcessSampler>,
    apps: Box<dyn AppDirectory>,
    signaller: Box<dyn Signaller>,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    audit_file: Option<PathBuf>,
  ) -> Arc<Self> {
    let platform = sampler.platform();
    let state = State {
      apps,
      signaller,
      audit_file,
      platform,
      terminate: matches!(platform, Platform::Macos | Platform::Linux),
      graceful_app_quit: cfg!(all(target_os = "macos", feature = "appkit")),
      interval_ms: DEFAULT_INTERVAL_MS,
      lease_until_ms: 0,
      shutdown: false,
      sequence: 0,
      snapshot: None,
      raw: Vec::new(),
      safety: None,
      last_frontmost_ms: HashMap::new(),
      history: History::new(),
      plans: PlanStore::new(),
      audit: AuditRing::new(),
      ids: IdGenerator::new(),
      last_error: None,
    };

    let service = Arc::new(MemoryService {
      clock,
      sleeper,
      state: Mutex::new(state),
      wake: Condvar::new(),
      sampler_thread: Mutex::new(None),
    });

    let worker = Arc::clone(&service);
    let mut sampler = sampler;
    let handle = thread::spawn(move || worker.run(&mut *sampler));
    *lock(&service.sampler_thread) = Some(handle);
    service
  }

  /// The latest snapshot, renewing the ten-second lease.
  ///
  /// The first call after the thread has parked blocks for at most two seconds
  /// while the first sample is taken; later calls return the latest published
  /// snapshot immediately.
  pub fn snapshot(&self) -> Result<Arc<MemorySnapshot>, MemoryError> {
    let mut guard = lock(&self.state);
    guard.lease_until_ms = self.clock.now_ms().saturating_add(LEASE_MS);
    self.wake.notify_all();

    if let Some(snapshot) = guard.snapshot.clone() {
      return Ok(snapshot);
    }

    let deadline = Instant::now() + FIRST_SAMPLE_TIMEOUT;
    while guard.snapshot.is_none() && guard.last_error.is_none() {
      let now = Instant::now();
      if now >= deadline {
        break;
      }
      let (next, _) = self
        .wake
        .wait_timeout(guard, deadline - now)
        .unwrap_or_else(|poison| poison.into_inner());
      guard = next;
    }

    if let Some(snapshot) = guard.snapshot.clone() {
      return Ok(snapshot);
    }
    if let Some(detail) = guard.last_error.clone() {
      return Err(MemoryError::Sampler { detail });
    }
    Err(MemoryError::Sampler {
      detail: "the first memory sample did not arrive within two seconds.".to_string(),
    })
  }

  /// Set the sampling interval, clamped to [`MIN_INTERVAL_MS`]..=[`MAX_INTERVAL_MS`].
  pub fn set_interval_ms(&self, ms: u32) {
    let mut guard = lock(&self.state);
    guard.interval_ms = ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
  }

  /// The downsampled system history and the requested group series.
  pub fn history(&self, request: &HistoryRequest) -> Result<MemoryHistory, MemoryError> {
    let guard = lock(&self.state);
    Ok(guard.history.query(request))
  }

  /// The free-up candidates for the latest snapshot.
  pub fn candidates(&self, request: &CandidatesRequest) -> Result<FreeUpCandidates, MemoryError> {
    let mut guard = lock(&self.state);
    let now_ms = self.clock.now_ms();
    let state = &mut *guard;
    let snapshot = state.snapshot.as_ref().ok_or_else(no_sample)?;
    Ok(candidates(snapshot, &state.history, request, now_ms))
  }

  /// The audit ring, newest first.
  pub fn audit(&self) -> AuditLog {
    let guard = lock(&self.state);
    AuditLog {
      records: guard.audit.records(),
    }
  }

  /// Build a single-use termination plan bound to `session`.
  pub fn plan(
    &self,
    session: &str,
    request: &TerminatePlanRequest,
  ) -> Result<TerminatePlan, MemoryError> {
    let mut guard = lock(&self.state);
    let now_ms = self.clock.now_ms();
    let State {
      plans,
      ids,
      safety,
      raw,
      snapshot,
      last_frontmost_ms,
      ..
    } = &mut *guard;

    let snapshot = snapshot.as_ref().ok_or_else(no_sample)?;
    let context = safety.as_ref().ok_or_else(no_sample)?;
    let index = SnapshotIndex::new(snapshot, raw, last_frontmost_ms, now_ms);
    let mut new_id = || ids.next(PLAN_ID_PREFIX);
    plans.build(session, request, &index, context, now_ms, &mut new_id)
  }

  /// Execute a plan once, recording each executed entry in the audit ring and,
  /// when configured, the audit file.
  pub fn execute(
    &self,
    session: &str,
    request: &TerminateExecuteRequest,
  ) -> Result<TerminateOutcome, MemoryError> {
    let mut guard = lock(&self.state);
    let State {
      plans,
      ids,
      safety,
      signaller,
      apps,
      audit,
      audit_file,
      ..
    } = &mut *guard;

    let context = safety.as_ref().ok_or_else(no_sample)?.clone();
    let mut new_id = || ids.next(AUDIT_ID_PREFIX);
    let outcome = plans.execute(
      session,
      request,
      &context,
      &**signaller,
      &mut **apps,
      self.clock.as_ref(),
      self.sleeper.as_ref(),
      audit,
      &mut new_id,
    )?;

    if let Some(path) = audit_file.as_ref() {
      let records = records_for(audit, &outcome.audit_ids);
      append_records(path, &records);
    }
    Ok(outcome)
  }

  /// The service's platform and process-control capabilities (§4.4).
  pub fn capabilities(&self) -> Hello {
    let guard = lock(&self.state);
    Hello {
      protocol: PROTOCOL_VERSION,
      platform: guard.platform,
      terminate: guard.terminate,
      graceful_app_quit: guard.graceful_app_quit,
    }
  }

  /// The sampler thread: idle while the lease is expired, then tick every
  /// interval until it lapses again.
  fn run(self: Arc<Self>, sampler: &mut dyn ProcessSampler) {
    loop {
      {
        let mut guard = lock(&self.state);
        while !guard.shutdown && self.clock.now_ms() >= guard.lease_until_ms {
          guard = self
            .wake
            .wait(guard)
            .unwrap_or_else(|poison| poison.into_inner());
        }
        if guard.shutdown {
          return;
        }
      }

      loop {
        {
          let guard = lock(&self.state);
          if guard.shutdown || self.clock.now_ms() >= guard.lease_until_ms {
            break;
          }
        }
        let sampled = sampler.sample();
        let interval = self.publish(sampled);
        self.sleeper.sleep_ms(u64::from(interval));
      }
    }
  }

  /// Publish one tick: on success build and store the snapshot, on failure keep
  /// the previous one and remember the error. Returns the current interval so
  /// the thread paces the next tick, and wakes every waiter.
  fn publish(&self, sampled: Result<RawSample, MemoryError>) -> u32 {
    let mut guard = lock(&self.state);
    match sampled {
      Err(error) => guard.last_error = Some(error.to_string()),
      Ok(raw) => {
        let now_ms = self.clock.now_ms();
        let apps = guard.apps.apps();
        let frontmost = guard.apps.frontmost_pid();
        let current_uid = guard.signaller.current_uid();
        let own_pid = guard.signaller.own_pid();

        let (mut groups, mut rows) = build_groups(&raw.processes, &apps, frontmost, current_uid);
        let mut truncated = false;
        if groups.len() > MAX_GROUPS {
          groups.truncate(MAX_GROUPS);
          truncated = true;
        }
        if rows.len() > MAX_PROCESSES {
          rows.truncate(MAX_PROCESSES);
          truncated = true;
        }

        let context = safety::context(&raw.processes, own_pid, current_uid, &groups);
        safety::apply(&mut groups, &mut rows, &raw.processes, &context);

        for group in &groups {
          if group.frontmost {
            guard
              .last_frontmost_ms
              .insert(group.group_id.clone(), now_ms);
          }
        }
        for group in &mut groups {
          if let Some(at) = guard.last_frontmost_ms.get(&group.group_id) {
            group.last_frontmost_at = Some(iso_from_ms(*at));
          }
        }

        guard.sequence = guard.sequence.wrapping_add(1);
        let breakdown = breakdown(&raw.system);
        let notes = derive_notes(&raw.system, &breakdown, &rows, truncated);
        let snapshot = MemorySnapshot {
          sequence: guard.sequence,
          sampled_at: iso_from_ms(now_ms),
          interval_ms: guard.interval_ms,
          platform: guard.platform,
          pressure: raw.system.pressure.clone(),
          breakdown,
          groups,
          processes: rows,
          notes,
        };

        guard.history.push(&snapshot, now_ms);
        guard.raw = raw.processes;
        guard.safety = Some(context);
        guard.snapshot = Some(Arc::new(snapshot));
        guard.last_error = None;
      }
    }
    let interval = guard.interval_ms;
    drop(guard);
    self.wake.notify_all();
    interval
  }
}

/// The error returned when an operation needs a snapshot the thread has not
/// published yet.
fn no_sample() -> MemoryError {
  MemoryError::Sampler {
    detail: "no memory sample is available yet.".to_string(),
  }
}

/// Lock `mutex`, recovering from poisoning. The guarded value is a plain
/// aggregate, so a panic elsewhere leaves it consistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The wire breakdown for one raw system reading.
///
/// On macOS the VM page counters give the full breakdown; everywhere else (and
/// on a macOS sample without them) the portable totals are used and the
/// optional parts stay `None`, which the view marks unavailable.
fn breakdown(system: &RawSystem) -> MemoryBreakdown {
  #[cfg(target_os = "macos")]
  if let Some(pages) = system.vm.as_ref() {
    return crate::platform::macos::breakdown_from_pages(
      pages,
      system.total_bytes,
      (system.swap_total_bytes, system.swap_used_bytes),
    );
  }

  MemoryBreakdown {
    total_bytes: system.total_bytes,
    used_bytes: system.used_bytes,
    free_bytes: system.free_bytes,
    available_bytes: system.available_bytes,
    app_bytes: None,
    wired_bytes: None,
    compressed_bytes: None,
    cached_files_bytes: None,
    swap_total_bytes: system.swap_total_bytes,
    swap_used_bytes: system.swap_used_bytes,
  }
}

/// The caveats a snapshot carries, in enum order (§4.1).
fn derive_notes(
  system: &RawSystem,
  breakdown: &MemoryBreakdown,
  rows: &[crate::model::ProcessRow],
  truncated: bool,
) -> Vec<SnapshotNote> {
  let mut notes = Vec::new();
  if !rows.is_empty() && rows.iter().all(|row| row.cpu_percent.is_none()) {
    notes.push(SnapshotNote::CpuWarmingUp);
  }
  if system.vm.is_some() && rows.iter().any(|row| row.restricted) {
    notes.push(SnapshotNote::RestrictedProcesses);
  }
  if breakdown.app_bytes.is_none() {
    notes.push(SnapshotNote::BreakdownUnavailable);
  }
  if system.pressure.source == PressureSource::Derived {
    notes.push(SnapshotNote::PressureDerived);
  }
  if truncated {
    notes.push(SnapshotNote::Truncated);
  }
  notes
}

/// The audit records named by `ids`, oldest first.
fn records_for(ring: &AuditRing, ids: &[AuditId]) -> Vec<AuditRecord> {
  if ids.is_empty() {
    return Vec::new();
  }
  let wanted: HashSet<&AuditId> = ids.iter().collect();
  let mut records: Vec<AuditRecord> = ring
    .records()
    .into_iter()
    .filter(|record| wanted.contains(&record.audit_id))
    .collect();
  records.reverse();
  records
}

/// Append `records` to the JSONL audit file at `path`, best effort: a failure
/// to write must never fail a termination that already happened.
fn append_records(path: &Path, records: &[AuditRecord]) {
  let mut lines = String::new();
  for record in records {
    if let Ok(line) = serde_json::to_string(record) {
      lines.push_str(&line);
      lines.push('\n');
    }
  }
  if lines.is_empty() {
    return;
  }
  let _ = append_jsonl(path, &lines);
}

/// Append `payload` to `path`, rotating it to a `.1` sibling first when the
/// result would exceed [`AUDIT_MAX_BYTES`].
fn append_jsonl(path: &Path, payload: &str) -> std::io::Result<()> {
  if let Ok(metadata) = std::fs::metadata(path) {
    if metadata.len() + payload.len() as u64 > AUDIT_MAX_BYTES {
      let _ = std::fs::rename(path, rotated_path(path));
    }
  }
  let mut file = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(path)?;
  file.write_all(payload.as_bytes())?;
  Ok(())
}

/// The `.1` sibling of an audit file.
fn rotated_path(path: &Path) -> PathBuf {
  let mut rotated = path.as_os_str().to_os_string();
  rotated.push(".1");
  PathBuf::from(rotated)
}

/// Render milliseconds since the Unix epoch as an ISO-8601 UTC instant with
/// millisecond precision (`2026-10-18T09:00:00.000Z`), matching `z.iso.datetime`.
fn iso_from_ms(now_ms: i64) -> String {
  let total_seconds = now_ms.div_euclid(1_000);
  let millis = now_ms.rem_euclid(1_000) as u32;
  let days = total_seconds.div_euclid(86_400);
  let second_of_day = total_seconds.rem_euclid(86_400);
  let (year, month, day) = civil_from_days(days);
  let hour = second_of_day / 3_600;
  let minute = (second_of_day % 3_600) / 60;
  let second = second_of_day % 60;
  format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Days since the Unix epoch → `(year, month, day)` (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
  let shifted = days + 719_468;
  let era = if shifted >= 0 {
    shifted
  } else {
    shifted - 146_096
  } / 146_097;
  let day_of_era = shifted - era * 146_097;
  let year_of_era =
    (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
  let year = year_of_era + era * 400;
  let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
  let month_prime = (5 * day_of_year + 2) / 153;
  let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
  let month = (if month_prime < 10 {
    month_prime + 3
  } else {
    month_prime - 9
  }) as u32;
  let year = if month <= 2 { year + 1 } else { year };
  (year, month, day)
}
