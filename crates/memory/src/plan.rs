//! Termination plans, execution, rate limits and the audit ring (§5, rules
//! S9–S21 of docs/memory-cleaner-plan.md). This is the only place that turns a
//! user-approved request into signals: it re-runs the safety rules against a
//! fresh identity read ([`crate::safety::classify`]) immediately before each
//! signal (S16), refuses anything that changed (S15), enforces the rate limits
//! (S17), waits for the processes to exit (S18) and records one audit entry per
//! executed process (S19).
//!
//! [`PlanStore`] owns the live plans, bound to the session that built them
//! (S13); [`SnapshotIndex`] is the latest snapshot plus the raw process rows a
//! plan resolves its targets against (S9); [`AuditRing`] is the bounded audit
//! log. Nothing here touches the operating system directly: it works through
//! the [`Signaller`]/[`AppDirectory`]/[`Clock`]/[`Sleeper`] seams, so the rules
//! are unit tested without a real machine or a wall-clock sleep.

use crate::error::MemoryError;
use crate::ids::{AuditId, GroupId, PlanId, ProcessKey};
use crate::model::{
  AuditRecord, MemorySnapshot, ProcessGroup, ProcessKind, ProcessRow, Protection, RefusalCode,
  RiskFlag, TerminateExecuteRequest, TerminateMethod, TerminateMode, TerminateOrigin,
  TerminateOutcome, TerminatePlan, TerminatePlanEntry, TerminatePlanRequest, TerminateRefusal,
  TerminateResult, TerminateStatus, TerminateTarget,
};
use crate::raw::{RawProcess, Signal};
use crate::safety::{classify, SafetyContext};
use crate::sampler::{AppDirectory, Clock, Signaller, Sleeper};
use std::collections::{HashMap, HashSet, VecDeque};

/// A plan expires this long after it is created (S13).
pub const PLAN_TTL_MS: i64 = 60_000;
/// The most targets one request may name (S11).
pub const MAX_TARGETS: usize = 20;
/// The most resolved entries one plan may hold (S11).
pub const MAX_ENTRIES: usize = 64;
/// The most live plans one session may hold (S13).
pub const MAX_PLANS_PER_SESSION: usize = 4;
/// How long a graceful termination is given to take effect (S18).
pub const GRACEFUL_WAIT_MS: u64 = 8_000;
/// How long a forced termination is given to take effect (S18).
pub const FORCE_WAIT_MS: u64 = 3_000;
/// How often the identity of a signalled process is re-read (S18).
pub const POLL_MS: u64 = 100;
/// How many audit records the ring keeps (S19).
pub const AUDIT_RING_CAP: usize = 200;

/// The sliding window for the per-session execute limit (S17).
const SESSION_EXEC_WINDOW_MS: i64 = 10_000;
/// The number of executes allowed per session per window (S17).
const SESSION_EXEC_LIMIT: usize = 3;
/// The sliding window for the per-service signal limit (S17).
const SIGNAL_WINDOW_MS: i64 = 60_000;
/// The number of signals allowed per service per window (S17).
const SIGNAL_LIMIT: usize = 60;
/// A signal this recently sent may only be followed by a stronger one (S17).
const RECENT_SIGNAL_MS: i64 = 2_000;
/// A group seen frontmost this recently counts as recently used (S21).
const RECENTLY_USED_MS: i64 = 600_000;

/// The bounded, newest-first audit log (S19).
#[derive(Debug, Default)]
pub struct AuditRing {
  records: VecDeque<AuditRecord>,
}

impl AuditRing {
  /// An empty ring.
  pub fn new() -> Self {
    AuditRing {
      records: VecDeque::new(),
    }
  }

  /// Add `record` as the newest entry, dropping the oldest beyond
  /// [`AUDIT_RING_CAP`].
  pub fn push(&mut self, record: AuditRecord) {
    self.records.push_front(record);
    while self.records.len() > AUDIT_RING_CAP {
      self.records.pop_back();
    }
  }

  /// The number of records held.
  pub fn len(&self) -> usize {
    self.records.len()
  }

  /// Whether the ring is empty.
  pub fn is_empty(&self) -> bool {
    self.records.is_empty()
  }

  /// The records, newest first.
  pub fn records(&self) -> Vec<AuditRecord> {
    self.records.iter().cloned().collect()
  }

  /// Remove every record.
  pub fn clear(&mut self) {
    self.records.clear();
  }
}

/// The latest snapshot plus the raw process rows a plan resolves against (S9).
///
/// It answers the questions [`PlanStore::build`] and [`PlanStore::execute`]
/// need: the group and row for a pid, the group containing a pid, which group
/// is frontmost and when each group was last seen frontmost, and how many
/// descendants a pid has in this sample.
#[derive(Debug)]
pub struct SnapshotIndex {
  groups: HashMap<GroupId, ProcessGroup>,
  rows: HashMap<u32, ProcessRow>,
  raw: HashMap<u32, RawProcess>,
  group_by_pid: HashMap<u32, GroupId>,
  children: HashMap<u32, Vec<u32>>,
  frontmost_group: Option<GroupId>,
  last_frontmost: HashMap<GroupId, i64>,
  now_ms: i64,
}

impl SnapshotIndex {
  /// Index `snap` and `raw` for planning at `now_ms`. `last_frontmost_ms` maps a
  /// group to the last time it was observed frontmost.
  pub fn new(
    snap: &MemorySnapshot,
    raw: &[RawProcess],
    last_frontmost_ms: &HashMap<GroupId, i64>,
    now_ms: i64,
  ) -> Self {
    let groups: HashMap<GroupId, ProcessGroup> = snap
      .groups
      .iter()
      .map(|group| (group.group_id.clone(), group.clone()))
      .collect();
    let rows: HashMap<u32, ProcessRow> = snap
      .processes
      .iter()
      .map(|row| (row.pid, row.clone()))
      .collect();
    let raw_by_pid: HashMap<u32, RawProcess> = raw
      .iter()
      .map(|process| (process.pid, process.clone()))
      .collect();

    let mut group_by_pid: HashMap<u32, GroupId> = HashMap::new();
    for group in &snap.groups {
      for pid in &group.pids {
        group_by_pid.insert(*pid, group.group_id.clone());
      }
    }

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for process in raw {
      if let Some(ppid) = process.ppid {
        children.entry(ppid).or_default().push(process.pid);
      }
    }

    let frontmost_group = snap
      .groups
      .iter()
      .find(|group| group.frontmost)
      .map(|group| group.group_id.clone());

    SnapshotIndex {
      groups,
      rows,
      raw: raw_by_pid,
      group_by_pid,
      children,
      frontmost_group,
      last_frontmost: last_frontmost_ms.clone(),
      now_ms,
    }
  }

  /// The group with this id, if the snapshot has it.
  pub fn group(&self, id: &GroupId) -> Option<&ProcessGroup> {
    self.groups.get(id)
  }

  /// The table row for this pid, if the snapshot has it.
  pub fn row(&self, pid: u32) -> Option<&ProcessRow> {
    self.rows.get(&pid)
  }

  /// The raw process for this pid, if the snapshot has it.
  pub fn raw(&self, pid: u32) -> Option<&RawProcess> {
    self.raw.get(&pid)
  }

  /// The id of the group containing `pid`.
  pub fn group_by_pid(&self, pid: u32) -> Option<&GroupId> {
    self.group_by_pid.get(&pid)
  }

  /// The frontmost group, if one was observed.
  pub fn frontmost_group(&self) -> Option<&GroupId> {
    self.frontmost_group.as_ref()
  }

  /// When the group was last observed frontmost, if ever.
  pub fn last_frontmost_ms(&self, id: &GroupId) -> Option<i64> {
    self.last_frontmost.get(id).copied()
  }

  /// Whether the group is frontmost now or was within the last 10 minutes.
  pub fn is_recently_frontmost(&self, id: &GroupId) -> bool {
    if self.frontmost_group.as_ref() == Some(id) {
      return true;
    }
    match self.last_frontmost.get(id) {
      Some(at) => self.now_ms - *at <= RECENTLY_USED_MS,
      None => false,
    }
  }

  /// How many descendants of `pid` are in this sample.
  pub fn descendant_count(&self, pid: u32) -> u32 {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut stack = vec![pid];
    while let Some(current) = stack.pop() {
      if let Some(kids) = self.children.get(&current) {
        for kid in kids {
          if seen.insert(*kid) {
            stack.push(*kid);
          }
        }
      }
    }
    seen.len() as u32
  }
}

/// One planned entry plus the metadata the audit record needs.
#[derive(Debug, Clone)]
struct StoredEntry {
  raw: RawProcess,
  method: TerminateMethod,
  group_id: GroupId,
  group_name: String,
  bundle_id: Option<String>,
}

/// A plan as the store keeps it: the wire plan, its session and the planned raw
/// processes to re-check at execute time (S16).
#[derive(Debug)]
struct StoredPlan {
  session: String,
  plan: TerminatePlan,
  created_ms: i64,
  entries: Vec<StoredEntry>,
}

/// The rate-limit state of one service (S17).
#[derive(Debug, Default)]
struct Limiter {
  signal_times: VecDeque<i64>,
  session_executes: HashMap<String, VecDeque<i64>>,
  recent: HashMap<ProcessKey, (TerminateMethod, i64)>,
}

impl Limiter {
  /// Allow at most three executes per session per ten seconds.
  fn check_session(&mut self, session: &str, now_ms: i64) -> Result<(), MemoryError> {
    let order = self
      .session_executes
      .entry(session.to_string())
      .or_default();
    while let Some(front) = order.front() {
      if now_ms - *front >= SESSION_EXEC_WINDOW_MS {
        order.pop_front();
      } else {
        break;
      }
    }
    if order.len() >= SESSION_EXEC_LIMIT {
      let oldest = order.front().copied().unwrap_or(now_ms);
      let retry_after_ms = (SESSION_EXEC_WINDOW_MS - (now_ms - oldest)).max(0) as u64;
      return Err(MemoryError::RateLimited { retry_after_ms });
    }
    order.push_back(now_ms);
    Ok(())
  }

  /// Allow at most sixty signals per service per minute.
  fn check_budget(&mut self, planned: usize, now_ms: i64) -> Result<(), MemoryError> {
    while let Some(front) = self.signal_times.front() {
      if now_ms - *front >= SIGNAL_WINDOW_MS {
        self.signal_times.pop_front();
      } else {
        break;
      }
    }
    if self.signal_times.len() + planned > SIGNAL_LIMIT {
      let oldest = self.signal_times.front().copied().unwrap_or(now_ms);
      let retry_after_ms = (SIGNAL_WINDOW_MS - (now_ms - oldest)).max(0) as u64;
      return Err(MemoryError::RateLimited { retry_after_ms });
    }
    Ok(())
  }

  /// Refuse a signal that is not stronger than one sent to the same identity in
  /// the last two seconds.
  fn check_recent(
    &self,
    pid: u32,
    start_time_us: u64,
    method: TerminateMethod,
    now_ms: i64,
  ) -> Result<(), MemoryError> {
    let key = ProcessKey { pid, start_time_us };
    if let Some((previous, at)) = self.recent.get(&key) {
      if now_ms - *at < RECENT_SIGNAL_MS && strength(*previous) >= strength(method) {
        let retry_after_ms = (RECENT_SIGNAL_MS - (now_ms - *at)).max(0) as u64;
        return Err(MemoryError::RateLimited { retry_after_ms });
      }
    }
    Ok(())
  }

  /// Record that a signal was sent.
  fn record(&mut self, pid: u32, start_time_us: u64, method: TerminateMethod, now_ms: i64) {
    self.signal_times.push_back(now_ms);
    self
      .recent
      .insert(ProcessKey { pid, start_time_us }, (method, now_ms));
  }
}

/// How forceful one method is (S17): an Apple quit event is the gentlest, then
/// SIGTERM, then SIGKILL.
fn strength(method: TerminateMethod) -> u8 {
  match method {
    TerminateMethod::AppQuit => 0,
    TerminateMethod::Sigterm => 1,
    TerminateMethod::Sigkill => 2,
  }
}

/// The plan store: the live plans, the per-session order and the rate limiter.
#[derive(Debug, Default)]
pub struct PlanStore {
  plans: HashMap<PlanId, StoredPlan>,
  session_order: HashMap<String, VecDeque<PlanId>>,
  limiter: Limiter,
  executing: bool,
}

impl PlanStore {
  /// An empty store.
  pub fn new() -> Self {
    PlanStore {
      plans: HashMap::new(),
      session_order: HashMap::new(),
      limiter: Limiter::default(),
      executing: false,
    }
  }

  /// Resolve `req` against `snap` and store an immutable, single-use plan (S9,
  /// S10, S11, S12, S13, S21).
  pub fn build(
    &mut self,
    session: &str,
    req: &TerminatePlanRequest,
    snap: &SnapshotIndex,
    ctx: &SafetyContext,
    now_ms: i64,
    new_id: &mut dyn FnMut() -> String,
  ) -> Result<TerminatePlan, MemoryError> {
    self.purge_expired(now_ms);

    let plan_id = PlanId::try_from(new_id()).map_err(|_| MemoryError::Internal {
      detail: "the plan id generator produced an invalid identifier.".to_string(),
    })?;

    let mut entries: Vec<TerminatePlanEntry> = Vec::new();
    let mut stored: Vec<StoredEntry> = Vec::new();
    let mut refused: Vec<TerminateRefusal> = Vec::new();
    let mut seen: HashSet<ProcessKey> = HashSet::new();
    let mut expected_release_bytes: u64 = 0;

    for (target_index, target) in req.targets.iter().enumerate() {
      if target_index >= MAX_TARGETS {
        refused.push(too_many_target(target, snap));
        continue;
      }
      match target {
        TerminateTarget::Group { group_id } => {
          let Some(group) = snap.group(group_id) else {
            refused.push(refusal(
              None,
              Some(group_id.clone()),
              "",
              RefusalCode::NotFound,
              None,
            ));
            continue;
          };
          let mut pids = group.pids.clone();
          pids.sort_unstable();
          if let Some(position) = pids.iter().position(|pid| *pid == group.main_pid) {
            let main = pids.remove(position);
            pids.insert(0, main);
          }
          for pid in pids {
            let Some(raw) = snap.raw(pid) else {
              refused.push(refusal(
                Some(pid),
                Some(group_id.clone()),
                "",
                RefusalCode::NotFound,
                None,
              ));
              continue;
            };
            let key = ProcessKey {
              pid,
              start_time_us: raw.start_time_us,
            };
            if !seen.insert(key) {
              refused.push(refusal(
                Some(pid),
                Some(group_id.clone()),
                &raw.name,
                RefusalCode::Duplicate,
                None,
              ));
              continue;
            }
            if let Some(protection) = classify(raw, ctx, group_id) {
              refused.push(refusal(
                Some(pid),
                Some(group_id.clone()),
                &raw.name,
                RefusalCode::Protected,
                Some(protection),
              ));
              continue;
            }
            if req.origin != TerminateOrigin::Manual && snap.frontmost_group() == Some(group_id) {
              refused.push(refusal(
                Some(pid),
                Some(group_id.clone()),
                &raw.name,
                RefusalCode::Frontmost,
                None,
              ));
              continue;
            }
            let is_main = pid == group.main_pid;
            let method = match req.mode {
              TerminateMode::Force => TerminateMethod::Sigkill,
              TerminateMode::Graceful => {
                if !is_main {
                  // Helpers are never signalled: they exit with their app (S10).
                  continue;
                }
                graceful_method(group.kind)
              }
            };
            if entries.len() >= MAX_ENTRIES {
              refused.push(refusal(
                Some(pid),
                Some(group_id.clone()),
                &raw.name,
                RefusalCode::TooMany,
                None,
              ));
              continue;
            }
            push_entry(
              &mut entries,
              &mut stored,
              &mut expected_release_bytes,
              raw,
              group_id,
              group.name.clone(),
              group.bundle_id.clone(),
              method,
              snap,
            );
          }
        }
        TerminateTarget::Process { pid, start_time_us } => {
          let Some(raw) = snap.raw(*pid) else {
            refused.push(refusal(Some(*pid), None, "", RefusalCode::NotFound, None));
            continue;
          };
          if raw.start_time_us != *start_time_us {
            refused.push(refusal(
              Some(*pid),
              None,
              &raw.name,
              RefusalCode::IdentityChanged,
              None,
            ));
            continue;
          }
          let Some(group_id) = snap.group_by_pid(*pid).cloned() else {
            refused.push(refusal(
              Some(*pid),
              None,
              &raw.name,
              RefusalCode::NotFound,
              None,
            ));
            continue;
          };
          let key = ProcessKey {
            pid: *pid,
            start_time_us: *start_time_us,
          };
          if !seen.insert(key) {
            refused.push(refusal(
              Some(*pid),
              Some(group_id),
              &raw.name,
              RefusalCode::Duplicate,
              None,
            ));
            continue;
          }
          if let Some(protection) = classify(raw, ctx, &group_id) {
            refused.push(refusal(
              Some(*pid),
              Some(group_id),
              &raw.name,
              RefusalCode::Protected,
              Some(protection),
            ));
            continue;
          }
          if req.origin != TerminateOrigin::Manual && snap.frontmost_group() == Some(&group_id) {
            refused.push(refusal(
              Some(*pid),
              Some(group_id),
              &raw.name,
              RefusalCode::Frontmost,
              None,
            ));
            continue;
          }
          let group = snap.group(&group_id);
          let kind = group.map(|group| group.kind).unwrap_or(ProcessKind::Other);
          let method = match req.mode {
            TerminateMode::Force => TerminateMethod::Sigkill,
            TerminateMode::Graceful => graceful_method(kind),
          };
          if entries.len() >= MAX_ENTRIES {
            refused.push(refusal(
              Some(*pid),
              Some(group_id),
              &raw.name,
              RefusalCode::TooMany,
              None,
            ));
            continue;
          }
          let group_name = group.map(|group| group.name.clone()).unwrap_or_default();
          let bundle_id = group.and_then(|group| group.bundle_id.clone());
          push_entry(
            &mut entries,
            &mut stored,
            &mut expected_release_bytes,
            raw,
            &group_id,
            group_name,
            bundle_id,
            method,
            snap,
          );
        }
      }
    }

    let plan = TerminatePlan {
      plan_id: plan_id.clone(),
      mode: req.mode,
      origin: req.origin,
      entries,
      refused,
      expected_release_bytes,
      created_at: iso_from_ms(now_ms),
      expires_at: iso_from_ms(now_ms + PLAN_TTL_MS),
    };

    self.plans.insert(
      plan_id.clone(),
      StoredPlan {
        session: session.to_string(),
        plan: plan.clone(),
        created_ms: now_ms,
        entries: stored,
      },
    );
    let order = self.session_order.entry(session.to_string()).or_default();
    order.push_back(plan_id);
    while order.len() > MAX_PLANS_PER_SESSION {
      if let Some(oldest) = order.pop_front() {
        self.plans.remove(&oldest);
      }
    }

    Ok(plan)
  }

  /// Execute a plan once (S13–S19). The plan is removed first, so it can never
  /// run twice, and every entry is re-checked against a fresh identity read
  /// before a signal is sent.
  #[allow(clippy::too_many_arguments)]
  pub fn execute(
    &mut self,
    session: &str,
    req: &TerminateExecuteRequest,
    ctx: &SafetyContext,
    sig: &dyn Signaller,
    apps: &mut dyn AppDirectory,
    clock: &dyn Clock,
    sleeper: &dyn Sleeper,
    audit: &mut AuditRing,
    new_audit_id: &mut dyn FnMut() -> String,
  ) -> Result<TerminateOutcome, MemoryError> {
    if self.executing {
      return Err(MemoryError::Busy);
    }
    self.executing = true;
    let outcome = self.execute_locked(
      session,
      req,
      ctx,
      sig,
      apps,
      clock,
      sleeper,
      audit,
      new_audit_id,
    );
    self.executing = false;
    outcome
  }

  #[allow(clippy::too_many_arguments)]
  fn execute_locked(
    &mut self,
    session: &str,
    req: &TerminateExecuteRequest,
    ctx: &SafetyContext,
    sig: &dyn Signaller,
    apps: &mut dyn AppDirectory,
    clock: &dyn Clock,
    sleeper: &dyn Sleeper,
    audit: &mut AuditRing,
    new_audit_id: &mut dyn FnMut() -> String,
  ) -> Result<TerminateOutcome, MemoryError> {
    // S14: confirmation is mandatory and is checked before the plan is used.
    if !req.confirm {
      return Err(MemoryError::NotConfirmed);
    }

    let now_ms = clock.now_ms();
    let stored = self.take_plan(session, &req.plan_id, now_ms)?;
    self.limiter.check_session(session, now_ms)?;

    let mut slots: Vec<Option<TerminateResult>> = vec![None; stored.entries.len()];
    let mut ready: Vec<Attempt> = Vec::new();

    for (index, entry) in stored.entries.iter().enumerate() {
      let planned_start = entry.raw.start_time_us;
      match sig.identity(entry.raw.pid) {
        None => {
          slots[index] = Some(result(
            entry,
            TerminateStatus::AlreadyGone,
            entry.method,
            None,
          ));
        }
        Some((start, uid, ppid, zombie)) => {
          if start != planned_start {
            // S15: the pid now names a different process.
            slots[index] = Some(result(
              entry,
              TerminateStatus::IdentityChanged,
              entry.method,
              None,
            ));
            continue;
          }
          let mut copy = entry.raw.clone();
          copy.uid = uid;
          copy.ppid = ppid;
          copy.zombie = zombie;
          if classify(&copy, ctx, &entry.group_id).is_some() {
            // S16: the rules are re-evaluated on the fresh identity.
            slots[index] = Some(result(entry, TerminateStatus::Refused, entry.method, None));
            continue;
          }
          ready.push(Attempt {
            index,
            pid: entry.raw.pid,
            start_time_us: planned_start,
            method: entry.method,
          });
        }
      }
    }

    // S17: the signal budget and the stronger-signal rule are checked before
    // any signal is sent, so a limited plan has no partial effect.
    self.limiter.check_budget(ready.len(), now_ms)?;
    for attempt in &ready {
      self
        .limiter
        .check_recent(attempt.pid, attempt.start_time_us, attempt.method, now_ms)?;
    }

    let mut pending: Vec<Pending> = Vec::new();
    for attempt in &ready {
      let outcome = send_one(attempt, sig, apps);
      match outcome {
        SendOutcome::Signalled(method) => {
          self
            .limiter
            .record(attempt.pid, attempt.start_time_us, method, now_ms);
          pending.push(Pending {
            index: attempt.index,
            pid: attempt.pid,
            start_time_us: attempt.start_time_us,
            method,
          });
        }
        SendOutcome::Failed(errno) => {
          let entry = &stored.entries[attempt.index];
          slots[attempt.index] = Some(result(
            entry,
            TerminateStatus::Failed,
            attempt.method,
            errno,
          ));
        }
      }
    }

    // S18: poll every still-pending entry together until each has exited or
    // changed identity, or the wait for this mode is over.
    let wait_ms = match stored.plan.mode {
      TerminateMode::Graceful => GRACEFUL_WAIT_MS,
      TerminateMode::Force => FORCE_WAIT_MS,
    };
    let mut elapsed_ms: u64 = 0;
    loop {
      let all_done = pending
        .iter()
        .all(|pending| match sig.identity(pending.pid) {
          None => true,
          Some((start, ..)) => start != pending.start_time_us,
        });
      if all_done || elapsed_ms >= wait_ms {
        break;
      }
      sleeper.sleep_ms(POLL_MS);
      elapsed_ms += POLL_MS;
    }
    for pending in &pending {
      let status = match sig.identity(pending.pid) {
        None => TerminateStatus::Exited,
        Some((start, ..)) if start != pending.start_time_us => TerminateStatus::IdentityChanged,
        Some(_) => TerminateStatus::StillRunning,
      };
      let entry = &stored.entries[pending.index];
      slots[pending.index] = Some(result(entry, status, pending.method, None));
    }

    let at = iso_from_ms(clock.now_ms());
    let mut results = Vec::with_capacity(slots.len());
    let mut audit_ids = Vec::new();
    let mut released_bytes_estimate: u64 = 0;

    for (index, slot) in slots.into_iter().enumerate() {
      let Some(result) = slot else {
        continue;
      };
      let entry = &stored.entries[index];
      if result.status == TerminateStatus::Exited {
        released_bytes_estimate =
          released_bytes_estimate.saturating_add(entry.raw.footprint_bytes.unwrap_or(0));
      }
      // S19: every executed entry except a refusal is audited.
      if result.status != TerminateStatus::Refused {
        let audit_id = AuditId::try_from(new_audit_id()).map_err(|_| MemoryError::Internal {
          detail: "the audit id generator produced an invalid identifier.".to_string(),
        })?;
        audit.push(AuditRecord {
          audit_id: audit_id.clone(),
          at: at.clone(),
          origin: stored.plan.origin,
          mode: stored.plan.mode,
          method: result.method,
          pid: result.pid,
          start_time_us: result.start_time_us,
          name: result.name.clone(),
          group_name: entry.group_name.clone(),
          bundle_id: entry.bundle_id.clone(),
          status: result.status,
          footprint_bytes: entry.raw.footprint_bytes,
        });
        audit_ids.push(audit_id);
      }
      results.push(result);
    }

    Ok(TerminateOutcome {
      plan_id: stored.plan.plan_id.clone(),
      results,
      released_bytes_estimate,
      audit_ids,
    })
  }

  /// Remove the plan if it belongs to `session` and has not expired.
  fn take_plan(
    &mut self,
    session: &str,
    plan_id: &PlanId,
    now_ms: i64,
  ) -> Result<StoredPlan, MemoryError> {
    let Some(plan) = self.plans.get(plan_id) else {
      return Err(MemoryError::PlanNotFound);
    };
    if plan.session != session {
      return Err(MemoryError::PlanSessionMismatch);
    }
    if now_ms - plan.created_ms > PLAN_TTL_MS {
      self.remove_plan(plan_id);
      return Err(MemoryError::PlanExpired);
    }
    let plan = match self.plans.remove(plan_id) {
      Some(plan) => plan,
      None => return Err(MemoryError::PlanNotFound),
    };
    if let Some(order) = self.session_order.get_mut(session) {
      order.retain(|id| id != plan_id);
    }
    Ok(plan)
  }

  /// Drop a plan and forget it in its session's order.
  fn remove_plan(&mut self, plan_id: &PlanId) {
    if let Some(removed) = self.plans.remove(plan_id) {
      if let Some(order) = self.session_order.get_mut(&removed.session) {
        order.retain(|id| id != plan_id);
      }
    }
  }

  /// Drop every plan whose creation is older than [`PLAN_TTL_MS`].
  fn purge_expired(&mut self, now_ms: i64) {
    let expired: Vec<PlanId> = self
      .plans
      .iter()
      .filter(|(_, plan)| now_ms - plan.created_ms > PLAN_TTL_MS)
      .map(|(id, _)| id.clone())
      .collect();
    for id in expired {
      self.remove_plan(&id);
    }
  }
}

/// One entry that passed its checks and is about to be signalled.
#[derive(Debug)]
struct Attempt {
  index: usize,
  pid: u32,
  start_time_us: u64,
  method: TerminateMethod,
}

/// One entry that was signalled and is being waited on.
#[derive(Debug)]
struct Pending {
  index: usize,
  pid: u32,
  start_time_us: u64,
  method: TerminateMethod,
}

/// How an attempted signal turned out.
#[derive(Debug)]
enum SendOutcome {
  Signalled(TerminateMethod),
  Failed(Option<i32>),
}

/// Send one entry's signal, falling back from a refused app quit to SIGTERM.
fn send_one(attempt: &Attempt, sig: &dyn Signaller, apps: &mut dyn AppDirectory) -> SendOutcome {
  match attempt.method {
    TerminateMethod::AppQuit => match apps.request_quit(attempt.pid) {
      Ok(crate::raw::QuitAck::Sent) => SendOutcome::Signalled(TerminateMethod::AppQuit),
      Ok(crate::raw::QuitAck::NotAnApp) => {
        send_signal(attempt.pid, Signal::Term, TerminateMethod::Sigterm, sig)
      }
      Err(_) => SendOutcome::Failed(None),
    },
    TerminateMethod::Sigterm => {
      send_signal(attempt.pid, Signal::Term, TerminateMethod::Sigterm, sig)
    }
    TerminateMethod::Sigkill => {
      send_signal(attempt.pid, Signal::Kill, TerminateMethod::Sigkill, sig)
    }
  }
}

/// Deliver one signal and describe the outcome.
fn send_signal(
  pid: u32,
  signal: Signal,
  method: TerminateMethod,
  sig: &dyn Signaller,
) -> SendOutcome {
  match sig.send(pid, signal) {
    Ok(()) => SendOutcome::Signalled(method),
    Err(errno) => SendOutcome::Failed(Some(errno)),
  }
}

/// The graceful method for a group kind (S10): an app or background agent is
/// asked to quit; everything else gets SIGTERM.
fn graceful_method(kind: ProcessKind) -> TerminateMethod {
  match kind {
    ProcessKind::App | ProcessKind::Agent => TerminateMethod::AppQuit,
    _ => TerminateMethod::Sigterm,
  }
}

/// Append one entry and its stored metadata, and grow the release estimate.
#[allow(clippy::too_many_arguments)]
fn push_entry(
  entries: &mut Vec<TerminatePlanEntry>,
  stored: &mut Vec<StoredEntry>,
  expected_release_bytes: &mut u64,
  raw: &RawProcess,
  group_id: &GroupId,
  group_name: String,
  bundle_id: Option<String>,
  method: TerminateMethod,
  snap: &SnapshotIndex,
) {
  let descendant_count = snap.descendant_count(raw.pid);
  let risks = entry_risks(
    method,
    descendant_count,
    snap.is_recently_frontmost(group_id),
    group_kind(snap, group_id),
  );
  *expected_release_bytes = expected_release_bytes.saturating_add(raw.footprint_bytes.unwrap_or(0));
  entries.push(TerminatePlanEntry {
    pid: raw.pid,
    start_time_us: raw.start_time_us,
    name: raw.name.clone(),
    group_id: group_id.clone(),
    footprint_bytes: raw.footprint_bytes,
    method,
    risks,
    descendant_count,
  });
  stored.push(StoredEntry {
    raw: raw.clone(),
    method,
    group_id: group_id.clone(),
    group_name,
    bundle_id,
  });
}

/// The kind of the group with this id, defaulting to `Other`.
fn group_kind(snap: &SnapshotIndex, group_id: &GroupId) -> ProcessKind {
  snap
    .group(group_id)
    .map(|group| group.kind)
    .unwrap_or(ProcessKind::Other)
}

/// The risk flags shown for one entry (S21), in enum order.
fn entry_risks(
  method: TerminateMethod,
  descendant_count: u32,
  recently_frontmost: bool,
  kind: ProcessKind,
) -> Vec<RiskFlag> {
  let mut risks = Vec::new();
  if method == TerminateMethod::AppQuit {
    risks.push(RiskFlag::AppMayPromptToSave);
  }
  if kind == ProcessKind::App
    && matches!(method, TerminateMethod::Sigkill | TerminateMethod::Sigterm)
  {
    risks.push(RiskFlag::UnsavedWorkMayBeLost);
  }
  if descendant_count > 0 {
    risks.push(RiskFlag::EndsChildProcesses);
  }
  if kind == ProcessKind::Agent {
    risks.push(RiskFlag::BackgroundAgent);
  }
  if recently_frontmost {
    risks.push(RiskFlag::RecentlyUsed);
  }
  if kind == ProcessKind::Agent {
    // A background agent is normally launchd-managed and comes back.
    risks.push(RiskFlag::Restarts);
  }
  risks
}

/// Build a refusal.
fn refusal(
  pid: Option<u32>,
  group_id: Option<GroupId>,
  name: &str,
  code: RefusalCode,
  protection: Option<Protection>,
) -> TerminateRefusal {
  TerminateRefusal {
    pid,
    group_id,
    name: name.to_string(),
    code,
    protection,
  }
}

/// A `TooMany` refusal for a target beyond the per-request cap (S11).
fn too_many_target(target: &TerminateTarget, snap: &SnapshotIndex) -> TerminateRefusal {
  match target {
    TerminateTarget::Group { group_id } => {
      let name = snap
        .group(group_id)
        .map(|group| group.name.clone())
        .unwrap_or_default();
      refusal(
        None,
        Some(group_id.clone()),
        &name,
        RefusalCode::TooMany,
        None,
      )
    }
    TerminateTarget::Process { pid, .. } => {
      let name = snap
        .raw(*pid)
        .map(|raw| raw.name.clone())
        .unwrap_or_default();
      let group_id = snap.group_by_pid(*pid).cloned();
      refusal(Some(*pid), group_id, &name, RefusalCode::TooMany, None)
    }
  }
}

/// Build one result.
fn result(
  entry: &StoredEntry,
  status: TerminateStatus,
  method: TerminateMethod,
  errno: Option<i32>,
) -> TerminateResult {
  TerminateResult {
    pid: entry.raw.pid,
    start_time_us: entry.raw.start_time_us,
    name: entry.raw.name.clone(),
    status,
    method,
    errno,
  }
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
