//! Termination-plan and execution cases from §5 of docs/memory-cleaner-plan.md:
//! one test per rule S9–S21 (`s9_…` … `s21_…`). Everything runs against the
//! fakes in `src/fake.rs` plus two small local doubles (a signaller that can
//! make a process exit and an app directory), so no real machine or wall-clock
//! sleep is involved.

use spacelens_memory::fake::{FakeClock, FakeSleeper};
use spacelens_memory::group::build_groups;
use spacelens_memory::plan::{AuditRing, PlanStore, SnapshotIndex};
use spacelens_memory::raw::{AppInfo, QuitAck, RawProcess, Signal};
use spacelens_memory::safety::{self, SafetyContext};
use spacelens_memory::sampler::{AppDirectory, Clock, Signaller};
use spacelens_memory::{
  AuditRecord, GroupId, MemoryBreakdown, MemoryError, MemoryPressure, MemorySnapshot, Platform,
  PressureLevel, PressureSource, RefusalCode, RiskFlag, TerminateExecuteRequest, TerminateMethod,
  TerminateMode, TerminateOrigin, TerminatePlanRequest, TerminateStatus, TerminateTarget,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

const NOW: i64 = 1_700_000_000_000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn proc(
  pid: u32,
  ppid: Option<u32>,
  uid: Option<u32>,
  name: &str,
  exe: Option<&str>,
) -> RawProcess {
  RawProcess {
    pid,
    ppid,
    start_time_us: 1_700_000_000_000_000 + u64::from(pid),
    uid,
    name: name.to_string(),
    exe: exe.map(Into::into),
    cpu_percent: None,
    footprint_bytes: Some(1_000_000 * u64::from(pid)),
    resident_bytes: Some(500_000),
    virtual_bytes: None,
    zombie: false,
  }
}

fn app(pid: u32, bundle_id: &str, name: &str) -> AppInfo {
  AppInfo {
    pid,
    bundle_id: Some(bundle_id.to_string()),
    name: Some(name.to_string()),
    regular: true,
  }
}

fn breakdown() -> MemoryBreakdown {
  MemoryBreakdown {
    total_bytes: 16_000_000_000,
    used_bytes: 8_000_000_000,
    free_bytes: 4_000_000_000,
    available_bytes: Some(6_000_000_000),
    app_bytes: None,
    wired_bytes: None,
    compressed_bytes: None,
    cached_files_bytes: None,
    swap_total_bytes: 0,
    swap_used_bytes: 0,
  }
}

fn pressure() -> MemoryPressure {
  MemoryPressure {
    level: PressureLevel::Normal,
    source: PressureSource::Kernel,
    available_percent: Some(70.0),
  }
}

fn snapshot(
  processes: &[RawProcess],
  apps: &[AppInfo],
  frontmost: Option<u32>,
  current_uid: Option<u32>,
  own_pid: u32,
  last_frontmost: &HashMap<GroupId, i64>,
) -> (SnapshotIndex, SafetyContext) {
  let (mut groups, mut rows) = build_groups(processes, apps, frontmost, current_uid);
  let ctx = safety::context(processes, own_pid, current_uid, &groups);
  safety::apply(&mut groups, &mut rows, processes, &ctx);
  let snap = MemorySnapshot {
    sequence: 1,
    sampled_at: "2026-10-18T00:00:00.000Z".to_string(),
    interval_ms: 1_000,
    platform: Platform::Macos,
    breakdown: breakdown(),
    pressure: pressure(),
    groups,
    processes: rows,
    notes: Vec::new(),
  };
  let index = SnapshotIndex::new(&snap, processes, last_frontmost, NOW);
  (index, ctx)
}

fn request(
  targets: Vec<TerminateTarget>,
  mode: TerminateMode,
  origin: TerminateOrigin,
) -> TerminatePlanRequest {
  TerminatePlanRequest {
    targets,
    mode,
    origin,
  }
}

fn process_target(p: &RawProcess) -> TerminateTarget {
  TerminateTarget::Process {
    pid: p.pid,
    start_time_us: p.start_time_us,
  }
}

fn counter(prefix: &str) -> impl FnMut() -> String + '_ {
  let mut n = 0u64;
  move || {
    n += 1;
    format!("{prefix}{n:016x}")
  }
}

/// A signaller whose identities can be changed or made to disappear, and which
/// One fake process identity: start time, uid, ppid, zombie.
type IdentityTuple = (u64, Option<u32>, Option<u32>, bool);

/// can mark a pid gone as soon as it is signalled (or after a number of reads),
/// so the poll loop is exercised without a real clock.
#[derive(Debug)]
struct TestSignaller {
  own_pid: u32,
  current_uid: Option<u32>,
  identities: Mutex<HashMap<u32, IdentityTuple>>,
  recorded: Mutex<Vec<(u32, Signal)>>,
  exit_on_send: bool,
  exit_after_reads: Mutex<HashMap<u32, u32>>,
  reads: Mutex<HashMap<u32, u32>>,
}

impl TestSignaller {
  fn new(own_pid: u32, current_uid: Option<u32>) -> Self {
    TestSignaller {
      own_pid,
      current_uid,
      identities: Mutex::new(HashMap::new()),
      recorded: Mutex::new(Vec::new()),
      exit_on_send: false,
      exit_after_reads: Mutex::new(HashMap::new()),
      reads: Mutex::new(HashMap::new()),
    }
  }

  fn insert(&self, pid: u32, identity: IdentityTuple) {
    lock(&self.identities).insert(pid, identity);
  }

  fn recorded(&self) -> Vec<(u32, Signal)> {
    lock(&self.recorded).clone()
  }
}

impl Signaller for TestSignaller {
  fn current_uid(&self) -> Option<u32> {
    self.current_uid
  }

  fn own_pid(&self) -> u32 {
    self.own_pid
  }

  fn identity(&self, pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
    let count = {
      let mut reads = lock(&self.reads);
      let slot = reads.entry(pid).or_insert(0);
      *slot += 1;
      *slot
    };
    let limit = lock(&self.exit_after_reads).get(&pid).copied();
    if let Some(limit) = limit {
      if count > limit {
        return None;
      }
    }
    lock(&self.identities).get(&pid).copied()
  }

  fn send(&self, pid: u32, signal: Signal) -> Result<(), i32> {
    lock(&self.recorded).push((pid, signal));
    if self.exit_on_send {
      lock(&self.identities).remove(&pid);
    }
    Ok(())
  }
}

/// An app directory that knows a fixed set of pids.
#[derive(Debug, Default)]
struct TestApps {
  known: Vec<u32>,
  quit_calls: Mutex<Vec<u32>>,
}

impl AppDirectory for TestApps {
  fn apps(&mut self) -> Vec<AppInfo> {
    Vec::new()
  }

  fn frontmost_pid(&mut self) -> Option<u32> {
    None
  }

  fn request_quit(&mut self, pid: u32) -> Result<QuitAck, MemoryError> {
    lock(&self.quit_calls).push(pid);
    if self.known.contains(&pid) {
      Ok(QuitAck::Sent)
    } else {
      Ok(QuitAck::NotAnApp)
    }
  }
}

fn dia() -> Vec<RawProcess> {
  vec![
    proc(
      1000,
      Some(1),
      Some(501),
      "Dia",
      Some("/Applications/Dia.app/Contents/MacOS/Dia"),
    ),
    proc(
      1001,
      Some(1000),
      Some(501),
      "Dia Helper",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper.app/Contents/MacOS/Dia Helper"),
    ),
  ]
}

fn dia_group_id() -> GroupId {
  build_groups(
    &dia(),
    &[app(1000, "company.thebrowser.dia", "Dia")],
    Some(1000),
    Some(501),
  )
  .0
  .into_iter()
  .find(|g| g.bundle_path.as_deref() == Some("/Applications/Dia.app"))
  .expect("Dia group")
  .group_id
}

fn group_refusal(plan: &spacelens_memory::TerminatePlan, code: RefusalCode) -> bool {
  plan.refused.iter().any(|r| r.code == code)
}

#[test]
fn s9_group_target_expands_and_protected_members_are_refused() {
  // The helper belongs to another user, so S7 refuses it; the main is usable.
  let processes = vec![
    proc(
      1000,
      Some(1),
      Some(501),
      "Dia",
      Some("/Applications/Dia.app/Contents/MacOS/Dia"),
    ),
    proc(
      1001,
      Some(1000),
      Some(502),
      "Dia Helper",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper.app/Contents/MacOS/Dia Helper"),
    ),
  ];
  let (index, ctx) = snapshot(&processes, &[], None, Some(501), 9999, &HashMap::new());
  let dia = dia_group_id();

  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![TerminateTarget::Group {
          group_id: dia.clone(),
        }],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");

  assert!(
    plan.entries.iter().any(|e| e.pid == 1000),
    "the main process is an entry"
  );
  assert!(
    !plan.entries.iter().any(|e| e.pid == 1001),
    "a protected member is never an entry"
  );
  let refusal = plan
    .refused
    .iter()
    .find(|r| r.pid == Some(1001))
    .expect("the helper is refused");
  assert_eq!(refusal.code, RefusalCode::Protected);
  assert_eq!(
    refusal.protection,
    Some(spacelens_memory::Protection::OtherUser)
  );

  // An unknown group is refused NotFound instead of erroring the whole plan.
  let unknown = GroupId::try_from("grp_00000000deadbeef".to_string()).expect("valid id");
  let plan = store
    .build(
      "s",
      &request(
        vec![TerminateTarget::Group { group_id: unknown }],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert!(plan.entries.is_empty());
  assert!(group_refusal(&plan, RefusalCode::NotFound));
}

#[test]
fn s10_graceful_signals_only_the_app_main_force_signals_every_member() {
  let processes = dia();
  let (index, ctx) = snapshot(
    &processes,
    &[app(1000, "company.thebrowser.dia", "Dia")],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let dia = dia_group_id();
  let targets = vec![TerminateTarget::Group {
    group_id: dia.clone(),
  }];

  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");

  let graceful = store
    .build(
      "s",
      &request(
        targets.clone(),
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert_eq!(graceful.entries.len(), 1, "only the app main is signalled");
  assert_eq!(graceful.entries[0].pid, 1000);
  assert_eq!(graceful.entries[0].method, TerminateMethod::AppQuit);

  let force = store
    .build(
      "s",
      &request(targets, TerminateMode::Force, TerminateOrigin::Manual),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let pids: Vec<u32> = force.entries.iter().map(|e| e.pid).collect();
  assert_eq!(
    pids,
    vec![1000, 1001],
    "force signals every member, main first"
  );
  assert!(force
    .entries
    .iter()
    .all(|e| e.method == TerminateMethod::Sigkill));
}

#[test]
fn s11_too_many_targets_entries_and_duplicates() {
  // 70 helpers under one bundle: entries cap at 64, the rest are TooMany.
  let mut processes = vec![proc(
    2000,
    Some(1),
    Some(501),
    "Big",
    Some("/Applications/Big.app/Contents/MacOS/Big"),
  )];
  for pid in 2001..2071u32 {
    processes.push(proc(
      pid,
      Some(2000),
      Some(501),
      "Big Helper",
      Some("/Applications/Big.app/Contents/Helpers/big-helper"),
    ));
  }
  let (index, ctx) = snapshot(&processes, &[], None, Some(501), 9999, &HashMap::new());
  let big = build_groups(&processes, &[], None, Some(501))
    .0
    .into_iter()
    .find(|g| g.bundle_path.as_deref() == Some("/Applications/Big.app"))
    .expect("Big group")
    .group_id;

  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![TerminateTarget::Group {
          group_id: big.clone(),
        }],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert_eq!(plan.entries.len(), 64);
  assert!(
    plan.refused.iter().any(|r| r.code == RefusalCode::TooMany),
    "entries beyond the cap are refused TooMany"
  );

  // 21 targets: the 21st is refused TooMany.
  let mut targets = vec![TerminateTarget::Group {
    group_id: big.clone(),
  }];
  for pid in 2000..2020u32 {
    targets.push(TerminateTarget::Process {
      pid,
      start_time_us: 1_700_000_000_000_000 + u64::from(pid),
    });
  }
  let plan = store
    .build(
      "s",
      &request(targets, TerminateMode::Force, TerminateOrigin::Manual),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert!(
    plan.refused.iter().any(|r| r.code == RefusalCode::TooMany),
    "targets beyond the cap are refused TooMany"
  );

  // The same process twice: one entry, one Duplicate.
  let p = proc(3000, Some(1), Some(501), "dupe", Some("/bin/dupe"));
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p), process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert_eq!(plan.entries.len(), 1);
  assert!(group_refusal(&plan, RefusalCode::Duplicate));
}

#[test]
fn s12_frontmost_group_is_refused_except_for_manual() {
  let processes = dia();
  let apps = [app(1000, "company.thebrowser.dia", "Dia")];
  let (index, ctx) = snapshot(
    &processes,
    &apps,
    Some(1000),
    Some(501),
    9999,
    &HashMap::new(),
  );
  let targets = vec![TerminateTarget::Group {
    group_id: dia_group_id(),
  }];

  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");

  for origin in [TerminateOrigin::FreeUp, TerminateOrigin::Advisor] {
    let plan = store
      .build(
        "s",
        &request(targets.clone(), TerminateMode::Force, origin),
        &index,
        &ctx,
        NOW,
        &mut ids,
      )
      .expect("plan builds");
    assert!(
      group_refusal(&plan, RefusalCode::Frontmost),
      "{origin:?} refuses the frontmost group"
    );
    assert!(plan.entries.is_empty());
  }

  let manual = store
    .build(
      "s",
      &request(targets, TerminateMode::Force, TerminateOrigin::Manual),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert!(
    !manual.entries.is_empty(),
    "manual may quit the frontmost app"
  );
  assert!(manual
    .entries
    .iter()
    .any(|e| e.risks.contains(&RiskFlag::RecentlyUsed)));
}

#[test]
fn s13_plans_are_session_bound_single_use_expiring_and_capped() {
  let p = proc(
    4000,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let target = process_target(&p);

  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![target.clone()],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");

  // Bound to the creating session.
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    4000,
    (1_700_000_000_000_000 + 4000, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  let err = store
    .execute(
      "other",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id.clone(),
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("a different session cannot execute");
  assert!(matches!(err, MemoryError::PlanSessionMismatch));

  // Single use: the first execute succeeds, the second is NotFound.
  let ok = store.execute(
    "s",
    &TerminateExecuteRequest {
      plan_id: plan.plan_id.clone(),
      confirm: true,
    },
    &ctx,
    &signaller,
    &mut TestApps::default(),
    clock.as_ref(),
    &sleeper,
    &mut audit,
    &mut audit_ids,
  );
  assert!(ok.is_ok(), "the owning session executes once: {ok:?}");
  let again = store.execute(
    "s",
    &TerminateExecuteRequest {
      plan_id: plan.plan_id.clone(),
      confirm: true,
    },
    &ctx,
    &signaller,
    &mut TestApps::default(),
    clock.as_ref(),
    &sleeper,
    &mut audit,
    &mut audit_ids,
  );
  assert!(matches!(again, Err(MemoryError::PlanNotFound)));

  // Expiry: 60 s after creation the plan is StalePlan.
  let expired = store
    .build(
      "s2",
      &request(
        vec![target.clone()],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let late = Arc::new(FakeClock::new(NOW + 60_001));
  let late_sleeper = FakeSleeper::new(Arc::clone(&late));
  let err = store
    .execute(
      "s2",
      &TerminateExecuteRequest {
        plan_id: expired.plan_id.clone(),
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      late.as_ref(),
      &late_sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("an expired plan is refused");
  assert!(matches!(err, MemoryError::PlanExpired));

  // At most 4 live plans per session: the oldest is evicted.
  let mut plan_ids = Vec::new();
  for _ in 0..5 {
    let plan = store
      .build(
        "s3",
        &request(
          vec![target.clone()],
          TerminateMode::Force,
          TerminateOrigin::Manual,
        ),
        &index,
        &ctx,
        NOW,
        &mut ids,
      )
      .expect("plan builds");
    plan_ids.push(plan.plan_id);
  }
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let evicted = store
    .execute(
      "s3",
      &TerminateExecuteRequest {
        plan_id: plan_ids[0].clone(),
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("the oldest plan was evicted");
  assert!(matches!(evicted, MemoryError::PlanNotFound));
}

#[test]
fn s14_execute_requires_confirmation() {
  let p = proc(
    4100,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");

  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    4100,
    (1_700_000_000_000_000 + 4100, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");

  let err = store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id.clone(),
        confirm: false,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("confirmation is mandatory");
  assert!(matches!(err, MemoryError::NotConfirmed));
  assert!(signaller.recorded().is_empty(), "nothing was sent");
  assert_eq!(audit.len(), 0);

  // The plan survives the refused attempt and executes with confirm true.
  let ok = store.execute(
    "s",
    &TerminateExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    },
    &ctx,
    &signaller,
    &mut TestApps::default(),
    clock.as_ref(),
    &sleeper,
    &mut audit,
    &mut audit_ids,
  );
  assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn s15_identity_mismatch_or_gone_sends_nothing() {
  let p = proc(
    4200,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");

  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  // Same pid, different start time: pid reuse.
  signaller.insert(
    4200,
    (1_700_000_000_000_000 + 999_999, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  let outcome = store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id.clone(),
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.results[0].status, TerminateStatus::IdentityChanged);
  assert!(signaller.recorded().is_empty(), "nothing was sent");

  // A pid that is gone is AlreadyGone.
  let p = proc(
    4300,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let plan = store
    .build(
      "s2",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let signaller = TestSignaller::new(9999, Some(501));
  // no identity for 4300
  let outcome = store
    .execute(
      "s2",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.results[0].status, TerminateStatus::AlreadyGone);
}

#[test]
fn s16_rules_are_re_evaluated_on_the_fresh_identity() {
  let p = proc(
    4400,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");

  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  // Same identity, but the uid now belongs to another user (a setuid exec).
  signaller.insert(
    4400,
    (1_700_000_000_000_000 + 4400, Some(999), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  let outcome = store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.results[0].status, TerminateStatus::Refused);
  assert!(signaller.recorded().is_empty());
  assert_eq!(audit.len(), 0, "a refused entry is not audited");
}

#[test]
fn s17_rate_limits_and_stronger_signals() {
  let p = proc(
    4500,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    4500,
    (1_700_000_000_000_000 + 4500, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");

  // ≤ 3 executes per session per 10 s.
  let mut plans = Vec::new();
  for _ in 0..4 {
    plans.push(
      store
        .build(
          "burst",
          &request(
            vec![process_target(&p)],
            TerminateMode::Force,
            TerminateOrigin::Manual,
          ),
          &index,
          &ctx,
          NOW,
          &mut ids,
        )
        .expect("plan builds")
        .plan_id,
    );
  }
  for plan_id in &plans[..3] {
    store
      .execute(
        "burst",
        &TerminateExecuteRequest {
          plan_id: plan_id.clone(),
          confirm: true,
        },
        &ctx,
        &signaller,
        &mut TestApps::default(),
        clock.as_ref(),
        &sleeper,
        &mut audit,
        &mut audit_ids,
      )
      .expect("the first three are allowed");
  }
  let limited = store
    .execute(
      "burst",
      &TerminateExecuteRequest {
        plan_id: plans[3].clone(),
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("the fourth execute is limited");
  assert!(matches!(limited, MemoryError::RateLimited { .. }));

  // After the window passes the session may execute again.
  clock.advance(10_000);
  let plan = store
    .build(
      "burst",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      clock.now_ms(),
      &mut ids,
    )
    .expect("plan builds");
  store
    .execute(
      "burst",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("the window has reset");

  // ≤ 60 signals per minute per service.
  let mut members = vec![proc(
    5000,
    Some(1),
    Some(501),
    "Sig",
    Some("/Applications/Sig.app/Contents/MacOS/Sig"),
  )];
  for pid in 5001..5061u32 {
    members.push(proc(
      pid,
      Some(5000),
      Some(501),
      "h",
      Some("/Applications/Sig.app/Contents/Helpers/h"),
    ));
  }
  let (index, ctx) = snapshot(&members, &[], None, Some(501), 9999, &HashMap::new());
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  let sig_group = build_groups(&members, &[], None, Some(501))
    .0
    .into_iter()
    .find(|g| g.bundle_path.as_deref() == Some("/Applications/Sig.app"))
    .expect("Sig group")
    .group_id;
  for member in &members {
    signaller.insert(
      member.pid,
      (member.start_time_us, Some(501), member.ppid, false),
    );
  }
  let plan = store
    .build(
      "svc",
      &request(
        vec![TerminateTarget::Group {
          group_id: sig_group,
        }],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  let err = store
    .execute(
      "svc",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("61 signals exceed the minute budget");
  assert!(matches!(err, MemoryError::RateLimited { .. }));
  assert!(
    signaller.recorded().is_empty(),
    "the budget is checked before any signal is sent"
  );

  // A recent signal may only be followed by a stronger one.
  let p = proc(
    4700,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let mut signaller = TestSignaller::new(9999, Some(501));
  signaller.exit_on_send = true;
  let identity = (1_700_000_000_000_000 + 4700, Some(501), Some(1), false);
  signaller.insert(4700, identity);

  // First a graceful SIGTERM, which exits at once (no fake time passes).
  let gentle = store
    .build(
      "sig",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  store
    .execute(
      "sig",
      &TerminateExecuteRequest {
        plan_id: gentle.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("the first graceful signal is allowed");
  assert_eq!(signaller.recorded(), vec![(4700, Signal::Term)]);

  // A stronger forced signal within 2 s is allowed.
  signaller.insert(4700, identity);
  let strong = store
    .build(
      "sig",
      &request(
        vec![process_target(&p)],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  store
    .execute(
      "sig",
      &TerminateExecuteRequest {
        plan_id: strong.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("a stronger signal is allowed");
  assert_eq!(
    signaller.recorded(),
    vec![(4700, Signal::Term), (4700, Signal::Kill)]
  );

  // A weaker graceful plan on the same identity within 2 s is limited.
  signaller.insert(4700, identity);
  let weak = store
    .build(
      "sig2",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let err = store
    .execute(
      "sig2",
      &TerminateExecuteRequest {
        plan_id: weak.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect_err("a weaker signal right after a stronger one is limited");
  assert!(matches!(err, MemoryError::RateLimited { .. }));
}

#[test]
fn s18_polls_until_the_wait_is_over_then_reports_still_running() {
  let p = proc(
    4800,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    4800,
    (1_700_000_000_000_000 + 4800, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");

  // Never exits: graceful waits the full 8 s.
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let outcome = store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.results[0].status, TerminateStatus::StillRunning);
  assert!(sleeper.slept_ms() >= 8_000, "graceful waited up to 8 s");

  // Exits on the second poll read: still polled, then Exited.
  let p = proc(
    4900,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let plan = store
    .build(
      "s2",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    4900,
    (1_700_000_000_000_000 + 4900, Some(501), Some(1), false),
  );
  lock(&signaller.exit_after_reads).insert(4900, 2);
  let outcome = store
    .execute(
      "s2",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.results[0].status, TerminateStatus::Exited);
}

#[test]
fn s19_every_executed_entry_is_audited_and_the_ring_caps_at_200() {
  let p = proc(
    5100,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    5100,
    (1_700_000_000_000_000 + 5100, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::FreeUp,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let outcome = store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(outcome.audit_ids.len(), 1);
  assert_eq!(audit.len(), 1);
  let record: AuditRecord = audit.records()[0].clone();
  assert_eq!(record.origin, TerminateOrigin::FreeUp);
  assert_eq!(record.mode, TerminateMode::Graceful);
  assert_eq!(record.status, TerminateStatus::StillRunning);
  assert_eq!(record.pid, 5100);

  // The ring keeps only the newest 200 records.
  for n in 0..250u64 {
    let mut record = audit.records()[0].clone();
    record.audit_id = spacelens_memory::AuditId::try_from(format!("aud_{n:016x}")).expect("id");
    audit.push(record);
  }
  assert_eq!(audit.len(), 200);
  assert_eq!(audit.records()[0].audit_id.as_str(), "aud_00000000000000f9");
}

#[test]
fn s20_advisor_origin_is_visible_in_the_plan_and_audit() {
  let p = proc(
    5200,
    Some(1),
    Some(501),
    "tool",
    Some("/usr/local/bin/tool"),
  );
  let (index, ctx) = snapshot(
    std::slice::from_ref(&p),
    &[],
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let plan = store
    .build(
      "s",
      &request(
        vec![process_target(&p)],
        TerminateMode::Graceful,
        TerminateOrigin::Advisor,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  assert_eq!(plan.origin, TerminateOrigin::Advisor);

  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = TestSignaller::new(9999, Some(501));
  signaller.insert(
    5200,
    (1_700_000_000_000_000 + 5200, Some(501), Some(1), false),
  );
  let mut audit = AuditRing::new();
  let mut audit_ids = counter("aud_");
  store
    .execute(
      "s",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      },
      &ctx,
      &signaller,
      &mut TestApps::default(),
      clock.as_ref(),
      &sleeper,
      &mut audit,
      &mut audit_ids,
    )
    .expect("execute runs");
  assert_eq!(audit.records()[0].origin, TerminateOrigin::Advisor);
}

#[test]
fn s21_risk_flags_are_shown() {
  // Graceful App: AppMayPromptToSave.
  let dia_processes = dia();
  let dia_apps = [app(1000, "company.thebrowser.dia", "Dia")];
  let (index, ctx) = snapshot(
    &dia_processes,
    &dia_apps,
    None,
    Some(501),
    9999,
    &HashMap::new(),
  );
  let mut store = PlanStore::new();
  let mut ids = counter("tplan_");
  let graceful = store
    .build(
      "s",
      &request(
        vec![TerminateTarget::Group {
          group_id: dia_group_id(),
        }],
        TerminateMode::Graceful,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let main = graceful
    .entries
    .iter()
    .find(|e| e.pid == 1000)
    .expect("main");
  assert!(main.risks.contains(&RiskFlag::AppMayPromptToSave));
  assert!(main.risks.contains(&RiskFlag::EndsChildProcesses));

  // Force App: UnsavedWorkMayBeLost.
  let force = store
    .build(
      "s",
      &request(
        vec![TerminateTarget::Group {
          group_id: dia_group_id(),
        }],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let main = force.entries.iter().find(|e| e.pid == 1000).expect("main");
  assert!(main.risks.contains(&RiskFlag::UnsavedWorkMayBeLost));

  // A background Agent and a recently-used app.
  let agent_processes = vec![proc(
    6000,
    Some(1),
    Some(501),
    "Raycast",
    Some("/Applications/Raycast.app/Contents/MacOS/Raycast"),
  )];
  let agent_apps = [AppInfo {
    pid: 6000,
    bundle_id: Some("com.raycast.macos".to_string()),
    name: Some("Raycast".to_string()),
    regular: false,
  }];
  let mut last_frontmost = HashMap::new();
  last_frontmost.insert(
    build_groups(&agent_processes, &agent_apps, None, Some(501)).0[0]
      .group_id
      .clone(),
    NOW - 60_000,
  );
  let (index, ctx) = snapshot(
    &agent_processes,
    &agent_apps,
    None,
    Some(501),
    9999,
    &last_frontmost,
  );
  let plan = store
    .build(
      "s2",
      &request(
        vec![process_target(&agent_processes[0])],
        TerminateMode::Force,
        TerminateOrigin::Manual,
      ),
      &index,
      &ctx,
      NOW,
      &mut ids,
    )
    .expect("plan builds");
  let entry = &plan.entries[0];
  assert!(entry.risks.contains(&RiskFlag::BackgroundAgent));
  assert!(entry.risks.contains(&RiskFlag::Restarts));
  assert!(entry.risks.contains(&RiskFlag::RecentlyUsed));
  assert_eq!(entry.descendant_count, 0);
}
