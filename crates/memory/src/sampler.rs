//! The seams the domain logic talks to (§4.2 of docs/memory-cleaner-plan.md):
//! the only place the crate is allowed to touch the operating system.
//! Production implementations live under `platform`; tests supply fakes, so
//! grouping, safety, planning and history run without a real machine.

use crate::error::MemoryError;
use crate::model::Platform;
use crate::raw::{AppInfo, QuitAck, RawSample, Signal};

/// Reads system and process memory once.
pub trait ProcessSampler: Send {
  /// The platform this sampler runs on.
  fn platform(&self) -> Platform;

  /// Take one sample, or fail with a [`MemoryError::Sampler`].
  fn sample(&mut self) -> Result<RawSample, MemoryError>;
}

/// Knows about running applications and can ask one to quit.
pub trait AppDirectory: Send {
  /// The applications currently running.
  fn apps(&mut self) -> Vec<AppInfo>;

  /// The pid of the frontmost application, if any.
  fn frontmost_pid(&mut self) -> Option<u32>;

  /// Ask an application to quit (macOS AppKit).
  fn request_quit(&mut self, pid: u32) -> Result<QuitAck, MemoryError>;
}

/// Reads fresh process identity and delivers signals.
pub trait Signaller: Send {
  /// The uid of the current user, if it can be determined.
  fn current_uid(&self) -> Option<u32>;

  /// The pid of this process.
  fn own_pid(&self) -> u32;

  /// Fresh identity read: `(start_time_us, uid, ppid, zombie)`, or `None` if
  /// the process is gone.
  fn identity(&self, pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)>;

  /// Deliver `signal`, returning the `errno` on failure.
  fn send(&self, pid: u32, signal: Signal) -> Result<(), i32>;
}

/// A wall clock, injectable for tests.
pub trait Clock: Send + Sync {
  /// The current time in milliseconds since the Unix epoch.
  fn now_ms(&self) -> i64;
}

/// A sleeper, injectable for tests.
pub trait Sleeper: Send + Sync {
  /// Sleep for `ms` milliseconds.
  fn sleep_ms(&self, ms: u64);
}
