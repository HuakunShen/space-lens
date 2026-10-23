//! In-memory doubles for the [`crate::sampler`] seams, compiled into every
//! build (not only tests) so integration tests under `tests/` can drive grouping,
//! safety, plans, history and the service without a real machine or wall-clock
//! sleeps. [`FakeSignaller`] answers identity reads from a map and records the
//! signals it is asked to send; [`FakeClock`] is a manually advanced wall clock
//! and [`FakeSleeper`] moves that clock forward instead of waiting.

use crate::raw::Signal;
use crate::sampler::{Clock, Signaller, Sleeper};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// The identity of one fake process, mirroring the tuple
/// [`Signaller::identity`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
  /// The process start time in microseconds since the Unix epoch.
  pub start_time_us: u64,
  /// The owning uid, if known.
  pub uid: Option<u32>,
  /// The parent pid, if known.
  pub ppid: Option<u32>,
  /// Whether the process is a zombie.
  pub zombie: bool,
}

impl Identity {
  /// Build an identity from its four parts.
  pub fn new(start_time_us: u64, uid: Option<u32>, ppid: Option<u32>, zombie: bool) -> Self {
    Identity {
      start_time_us,
      uid,
      ppid,
      zombie,
    }
  }
}

/// A [`Signaller`] that reads identity from a map and records sent signals.
#[derive(Debug)]
pub struct FakeSignaller {
  own_pid: u32,
  current_uid: Option<u32>,
  identities: Mutex<HashMap<u32, Identity>>,
  recorded: Mutex<Vec<(u32, Signal)>>,
}

impl FakeSignaller {
  /// A signaller for `own_pid` running as `current_uid`, with no processes.
  pub fn new(own_pid: u32, current_uid: Option<u32>) -> Self {
    FakeSignaller {
      own_pid,
      current_uid,
      identities: Mutex::new(HashMap::new()),
      recorded: Mutex::new(Vec::new()),
    }
  }

  /// Add or replace the identity for `pid`.
  pub fn insert(&self, pid: u32, identity: Identity) {
    lock(&self.identities).insert(pid, identity);
  }

  /// Forget `pid`, as if it had exited.
  pub fn remove(&self, pid: u32) {
    lock(&self.identities).remove(&pid);
  }

  /// The signals sent so far, oldest first.
  pub fn recorded(&self) -> Vec<(u32, Signal)> {
    lock(&self.recorded).clone()
  }

  /// Forget every recorded signal.
  pub fn clear(&self) {
    lock(&self.recorded).clear();
  }
}

impl Signaller for FakeSignaller {
  fn current_uid(&self) -> Option<u32> {
    self.current_uid
  }

  fn own_pid(&self) -> u32 {
    self.own_pid
  }

  fn identity(&self, pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
    lock(&self.identities)
      .get(&pid)
      .map(|id| (id.start_time_us, id.uid, id.ppid, id.zombie))
  }

  fn send(&self, pid: u32, signal: Signal) -> Result<(), i32> {
    lock(&self.recorded).push((pid, signal));
    Ok(())
  }
}

/// A [`Clock`] whose time only moves when the test says so.
#[derive(Debug)]
pub struct FakeClock {
  now_ms: AtomicI64,
}

impl FakeClock {
  /// A clock reading `now_ms`.
  pub fn new(now_ms: i64) -> Self {
    FakeClock {
      now_ms: AtomicI64::new(now_ms),
    }
  }

  /// Set the clock to `now_ms`.
  pub fn set(&self, now_ms: i64) {
    self.now_ms.store(now_ms, Ordering::SeqCst);
  }

  /// Move the clock forward by `ms` milliseconds.
  pub fn advance(&self, ms: u64) {
    self.now_ms.fetch_add(ms as i64, Ordering::SeqCst);
  }
}

impl Clock for FakeClock {
  fn now_ms(&self) -> i64 {
    self.now_ms.load(Ordering::SeqCst)
  }
}

/// A [`Sleeper`] that advances a [`FakeClock`] instead of waiting.
#[derive(Debug)]
pub struct FakeSleeper {
  clock: Arc<FakeClock>,
  slept_ms: Mutex<u64>,
}

impl FakeSleeper {
  /// A sleeper that advances `clock`.
  pub fn new(clock: Arc<FakeClock>) -> Self {
    FakeSleeper {
      clock,
      slept_ms: Mutex::new(0),
    }
  }

  /// The total number of milliseconds slept through this sleeper.
  pub fn slept_ms(&self) -> u64 {
    *lock(&self.slept_ms)
  }
}

impl Sleeper for FakeSleeper {
  fn sleep_ms(&self, ms: u64) {
    self.clock.advance(ms);
    *lock(&self.slept_ms) += ms;
  }
}

/// Lock `mutex`, recovering from poisoning: every operation in this module is a
/// plain map or vector edit, so the guarded value is always consistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}
