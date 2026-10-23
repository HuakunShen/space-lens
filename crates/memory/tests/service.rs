//! MemoryService cases (T10 of docs/memory-cleaner-plan.md): the background
//! sampler thread, the snapshot lease, interval clamping, history/candidates/
//! audit fan-out and the plan/execute round trip. The service is driven with a
//! canned sampler plus the fakes in `src/fake.rs`, and every sleep advances a
//! [`FakeClock`], so nothing waits on the wall clock.

use spacelens_memory::fake::{FakeClock, FakeSleeper};
use spacelens_memory::platform::NullApps;
use spacelens_memory::raw::{RawProcess, RawSample, RawSystem, Signal};
use spacelens_memory::sampler::{Clock, ProcessSampler, Signaller, Sleeper};
use spacelens_memory::{
  CandidatesRequest, HistoryRequest, MemoryError, MemoryPressure, MemoryService, Platform,
  PressureLevel, PressureSource, TerminateExecuteRequest, TerminateMode, TerminateOrigin,
  TerminatePlanRequest, TerminateStatus, TerminateTarget,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// The instant the fake clock starts at, shared with the plans.
const NOW: i64 = 1_700_000_000_000;
/// A pid that belongs to neither the service nor its sample.
const OWN_PID: u32 = 999_999;
/// The uid the service and the sampled process both run as.
const UID: u32 = 501;
/// The sampled process the plan targets.
const WORKER_PID: u32 = 4001;
/// The exact start time of the sampled process.
const WORKER_START_US: u64 = 1_700_000_000_000_000 + WORKER_PID as u64;

/// Lock `mutex`, recovering from poisoning: a test asserts on the guarded value
/// only after the operation that could have panicked has finished.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The one unprotected process the canned sample contains.
fn worker() -> RawProcess {
  RawProcess {
    pid: WORKER_PID,
    ppid: Some(900_000),
    start_time_us: WORKER_START_US,
    uid: Some(UID),
    name: "worker".to_string(),
    exe: Some("/usr/local/bin/worker".into()),
    cpu_percent: Some(0.5),
    footprint_bytes: Some(512 * 1024 * 1024),
    resident_bytes: Some(256 * 1024 * 1024),
    virtual_bytes: Some(1024 * 1024 * 1024),
    zombie: false,
  }
}

/// The system reading of the canned sample: no VM pages, derived pressure.
fn system() -> RawSystem {
  RawSystem {
    total_bytes: 16_000_000_000,
    used_bytes: 8_000_000_000,
    free_bytes: 4_000_000_000,
    available_bytes: Some(6_000_000_000),
    swap_total_bytes: 0,
    swap_used_bytes: 0,
    vm: None,
    pressure: MemoryPressure {
      level: PressureLevel::Normal,
      source: PressureSource::Derived,
      available_percent: Some(37.5),
    },
  }
}

/// A [`ProcessSampler`] that returns the same sample on every tick, on
/// [`Platform::Linux`] so the tests read the same on every host.
#[derive(Debug)]
struct CannedSampler {
  sample: RawSample,
}

impl CannedSampler {
  fn new() -> Self {
    CannedSampler {
      sample: RawSample {
        system: system(),
        processes: vec![worker()],
      },
    }
  }
}

impl ProcessSampler for CannedSampler {
  fn platform(&self) -> Platform {
    Platform::Linux
  }

  fn sample(&mut self) -> Result<RawSample, MemoryError> {
    Ok(self.sample.clone())
  }
}

/// A [`ProcessSampler`] whose every tick fails, for the error path.
#[derive(Debug)]
struct FailingSampler;

impl ProcessSampler for FailingSampler {
  fn platform(&self) -> Platform {
    Platform::Linux
  }

  fn sample(&mut self) -> Result<RawSample, MemoryError> {
    Err(MemoryError::Sampler {
      detail: "the test sampler always fails.".to_string(),
    })
  }
}

/// A process's fresh identity, mirroring [`Signaller::identity`]'s tuple.
type Identity = (u64, Option<u32>, Option<u32>, bool);

/// A [`Signaller`] that records the signals it is asked to send and forgets a
/// process's identity the moment it is signalled, so the service's poll loop
/// observes it as exited. Its shared state is cloneable, which lets a test keep
/// a handle after the signaller has been boxed into the service.
#[derive(Clone, Debug)]
struct ExitingSignaller {
  own_pid: u32,
  uid: Option<u32>,
  identities: Arc<Mutex<HashMap<u32, Identity>>>,
  sent: Arc<Mutex<Vec<(u32, Signal)>>>,
}

impl ExitingSignaller {
  fn new(own_pid: u32, uid: Option<u32>) -> Self {
    ExitingSignaller {
      own_pid,
      uid,
      identities: Arc::new(Mutex::new(HashMap::new())),
      sent: Arc::new(Mutex::new(Vec::new())),
    }
  }

  fn insert(&self, pid: u32, identity: Identity) {
    lock(&self.identities).insert(pid, identity);
  }

  fn sent(&self) -> Vec<(u32, Signal)> {
    lock(&self.sent).clone()
  }
}

impl Signaller for ExitingSignaller {
  fn current_uid(&self) -> Option<u32> {
    self.uid
  }

  fn own_pid(&self) -> u32 {
    self.own_pid
  }

  fn identity(&self, pid: u32) -> Option<Identity> {
    lock(&self.identities).get(&pid).copied()
  }

  fn send(&self, pid: u32, signal: Signal) -> Result<(), i32> {
    lock(&self.sent).push((pid, signal));
    lock(&self.identities).remove(&pid);
    Ok(())
  }
}

/// Start a service around `sampler` and `signaller` with a fake clock.
fn start(sampler: Box<dyn ProcessSampler>, signaller: Box<dyn Signaller>) -> Arc<MemoryService> {
  let clock = Arc::new(FakeClock::new(NOW));
  let sleeper: Arc<dyn Sleeper> = Arc::new(FakeSleeper::new(clock.clone()));
  let clock: Arc<dyn Clock> = clock;
  MemoryService::start(sampler, Box::new(NullApps), signaller, clock, sleeper, None)
}

/// A service whose sample contains the exiting worker.
fn working_service() -> (Arc<MemoryService>, ExitingSignaller) {
  let signaller = ExitingSignaller::new(OWN_PID, Some(UID));
  signaller.insert(
    WORKER_PID,
    (WORKER_START_US, Some(UID), Some(900_000), false),
  );
  let service = start(Box::new(CannedSampler::new()), Box::new(signaller.clone()));
  (service, signaller)
}

/// Wait until the sampler thread has published a snapshot with `expected` as
/// its interval, bounded so a regression cannot hang the run.
fn wait_for_interval(service: &MemoryService, expected: u32) -> u32 {
  for _ in 0..200_000 {
    let snapshot = service.snapshot().expect("a snapshot while waiting");
    if snapshot.interval_ms == expected {
      return snapshot.interval_ms;
    }
    std::thread::yield_now();
  }
  panic!("the interval never became {expected}");
}

#[test]
fn snapshot_blocks_for_the_first_sample_then_serves_the_latest() {
  let (service, _signaller) = working_service();

  let first = service.snapshot().expect("the first sample arrives");
  assert!(
    first.sequence >= 1,
    "the first snapshot has a positive sequence"
  );
  assert_eq!(first.platform, Platform::Linux);
  assert!(
    first.processes.iter().any(|row| row.pid == WORKER_PID),
    "the sampled process is in the snapshot"
  );
  assert!(
    first
      .notes
      .contains(&spacelens_memory::SnapshotNote::BreakdownUnavailable),
    "a portable sample has no VM breakdown"
  );
  assert!(
    first
      .notes
      .contains(&spacelens_memory::SnapshotNote::PressureDerived),
    "a derived pressure reading is marked as such"
  );

  let second = service.snapshot().expect("the latest sample");
  assert!(
    second.sequence >= first.sequence,
    "the sequence never goes backwards"
  );
}

#[test]
fn set_interval_ms_clamps_to_the_documented_range() {
  let (service, _signaller) = working_service();
  service.snapshot().expect("a first sample");

  service.set_interval_ms(100);
  assert_eq!(wait_for_interval(&service, 1_000), 1_000);

  service.set_interval_ms(90_000);
  assert_eq!(wait_for_interval(&service, 5_000), 5_000);

  service.set_interval_ms(2_000);
  assert_eq!(wait_for_interval(&service, 2_000), 2_000);
}

#[test]
fn history_candidates_and_audit_follow_the_snapshot() {
  let (service, _signaller) = working_service();
  let snapshot = service.snapshot().expect("a first sample");
  let group_id = snapshot.groups[0].group_id.clone();

  let history = service
    .history(&HistoryRequest {
      group_ids: vec![group_id.clone()],
    })
    .expect("history");
  assert!(!history.points.is_empty(), "history has at least one point");
  assert_eq!(history.groups.len(), 1, "the requested group has a series");

  let candidates = service
    .candidates(&CandidatesRequest {
      exclude: Vec::new(),
    })
    .expect("candidates");
  assert!(
    candidates.sequence >= 1,
    "candidates are dated by a real sequence"
  );

  assert!(
    service.audit().records.is_empty(),
    "nothing has been terminated yet"
  );
}

#[test]
fn a_plan_and_execute_through_the_service_quit_a_process() {
  let (service, signaller) = working_service();
  service.snapshot().expect("a first sample");

  let request = TerminatePlanRequest {
    targets: vec![TerminateTarget::Process {
      pid: WORKER_PID,
      start_time_us: WORKER_START_US,
    }],
    mode: TerminateMode::Graceful,
    origin: TerminateOrigin::Manual,
  };
  let plan = service.plan("sess_plan", &request).expect("a plan");
  assert_eq!(plan.entries.len(), 1, "the worker is planned");
  assert!(plan.refused.is_empty(), "the worker is unprotected");

  let outcome = service
    .execute(
      "sess_plan",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id.clone(),
        confirm: true,
      },
    )
    .expect("an outcome");
  assert_eq!(outcome.results.len(), 1);
  assert_eq!(outcome.results[0].status, TerminateStatus::Exited);
  assert_eq!(signaller.sent(), vec![(WORKER_PID, Signal::Term)]);

  let audit = service.audit();
  assert_eq!(audit.records.len(), 1, "one executed entry is audited");
  assert_eq!(audit.records[0].pid, WORKER_PID);
}

#[test]
fn execute_without_confirmation_is_refused_and_signals_nothing() {
  let (service, signaller) = working_service();
  service.snapshot().expect("a first sample");

  let request = TerminatePlanRequest {
    targets: vec![TerminateTarget::Process {
      pid: WORKER_PID,
      start_time_us: WORKER_START_US,
    }],
    mode: TerminateMode::Graceful,
    origin: TerminateOrigin::Manual,
  };
  let plan = service.plan("sess_confirm", &request).expect("a plan");

  let error = service
    .execute(
      "sess_confirm",
      &TerminateExecuteRequest {
        plan_id: plan.plan_id.clone(),
        confirm: false,
      },
    )
    .expect_err("an unconfirmed execute is refused");
  assert_eq!(error.problem_code(), "InvalidRequest");
  assert!(
    signaller.sent().is_empty(),
    "an unconfirmed execute must not signal anything"
  );
}

#[test]
fn a_failing_sampler_is_reported_as_a_retryable_sampler_error() {
  let service = start(
    Box::new(FailingSampler),
    Box::new(spacelens_memory::fake::FakeSignaller::new(
      OWN_PID,
      Some(UID),
    )),
  );

  let error = service
    .snapshot()
    .expect_err("the first sample never arrives");
  assert_eq!(error.problem_code(), "Unavailable");
  assert!(error.retryable(), "a sampler failure is retryable");
}

#[test]
fn capabilities_describe_the_platform_and_process_control() {
  let (service, _signaller) = working_service();
  let hello = service.capabilities();

  assert_eq!(hello.protocol, 1);
  assert_eq!(hello.platform, Platform::Linux);
  assert!(hello.terminate, "a unix platform can terminate processes");
  assert_eq!(
    hello.graceful_app_quit,
    cfg!(all(target_os = "macos", feature = "appkit")),
    "graceful app quit needs macOS with the appkit feature"
  );
}
