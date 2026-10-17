//! The operating-system layer of the memory crate (§4.2 of
//! docs/memory-cleaner-plan.md). Every sampler seam gets exactly one default
//! implementation for the platform this binary is built for, so the service,
//! the CLI and the desktop app never name a platform type directly.
//!
//! [`portable`] is the sysinfo-only sampler that runs wherever sysinfo does but
//! reads no per-process footprint and no kernel pressure; [`macos`] wraps it
//! with the footprint, exact start time, VM page counters and kernel pressure
//! that only macOS offers. [`unix_signal`] is the `kill(2)`-based signaller for
//! every unix, and the AppKit application directory (T11) replaces the default
//! below as it lands.

pub mod portable;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(unix)]
pub mod unix_signal;

use crate::error::MemoryError;
use crate::raw::{AppInfo, QuitAck, Signal};
use crate::sampler::{AppDirectory, ProcessSampler, Signaller};

#[cfg(target_os = "macos")]
pub use macos::MacSampler;
pub use portable::PortableSampler;

/// The sampler for the platform this binary is built for.
///
/// macOS gets [`MacSampler`], which enriches the portable reading with the
/// footprint, exact start time, VM page counters and kernel pressure; every
/// other platform gets the portable, sysinfo-only [`PortableSampler`].
#[cfg(target_os = "macos")]
pub fn default_sampler() -> Box<dyn ProcessSampler> {
  Box::new(MacSampler::new())
}

/// The sampler for platforms without a native enrichment (see the macOS
/// overload above).
#[cfg(not(target_os = "macos"))]
pub fn default_sampler() -> Box<dyn ProcessSampler> {
  Box::new(PortableSampler::new())
}

/// The application directory for the platform this binary is built for.
///
/// macOS gets `AppKitApps` under the `appkit` feature (T11); everywhere else,
/// including a macOS build without that feature, this is [`NullApps`].
pub fn default_apps() -> Box<dyn AppDirectory> {
  Box::new(NullApps)
}

/// The signaller for the platform this binary is built for.
///
/// Unix gets [`unix_signal::UnixSignaller`], which reads a process's exact
/// identity and delivers `SIGTERM`/`SIGKILL`; Windows gets
/// [`UnsupportedSignaller`], whose every `send` fails with `ENOSYS` (38) so no
/// signal can leave the process.
#[cfg(unix)]
pub fn default_signaller() -> Box<dyn Signaller> {
  Box::new(unix_signal::UnixSignaller::new())
}

/// The signaller for platforms with no process signalling (see the unix
/// overload above).
#[cfg(not(unix))]
pub fn default_signaller() -> Box<dyn Signaller> {
  Box::new(UnsupportedSignaller)
}

/// An [`AppDirectory`] for hosts with no application layer: it knows of no
/// running applications and no frontmost pid, and answers every quit request
/// with [`QuitAck::NotAnApp`], which makes graceful termination fall back to
/// `SIGTERM` (§4.2, S10).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullApps;

impl AppDirectory for NullApps {
  fn apps(&mut self) -> Vec<AppInfo> {
    Vec::new()
  }

  fn frontmost_pid(&mut self) -> Option<u32> {
    None
  }

  fn request_quit(&mut self, _pid: u32) -> Result<QuitAck, MemoryError> {
    Ok(QuitAck::NotAnApp)
  }
}

/// `ENOSYS`, the errno every [`Signaller::send`] on [`UnsupportedSignaller`]
/// returns.
const ENOSYS: i32 = 38;

/// A [`Signaller`] for platforms with no process signalling: it can read no
/// identity and every [`Signaller::send`] fails with `ENOSYS` (38).
#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedSignaller;

impl Signaller for UnsupportedSignaller {
  fn current_uid(&self) -> Option<u32> {
    None
  }

  fn own_pid(&self) -> u32 {
    std::process::id()
  }

  fn identity(&self, _pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
    None
  }

  fn send(&self, _pid: u32, _signal: Signal) -> Result<(), i32> {
    Err(ENOSYS)
  }
}
