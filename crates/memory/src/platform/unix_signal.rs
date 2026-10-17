//! The unix signaller (T9 of docs/memory-cleaner-plan.md): the
//! [`Signaller`] implementation behind the termination flow on macOS and Linux.
//! It hides the two things the plan code must not know about — how to read a
//! process's exact identity from the kernel (so a reused pid is caught before a
//! signal leaves, S15) and how to deliver `SIGTERM`/`SIGKILL` — behind the
//! [`Signaller`] seam, so plan execution is exercised with fakes and only this
//! file needs `unsafe`.
//!
//! Identity is read fresh on every call and never cached. macOS reads
//! `proc_pidinfo(PROC_PIDTBSDINFO)`, whose `proc_bsdinfo` carries the start
//! time in microseconds, the owning uid, the parent pid and the process status.
//! Linux reads `/proc/<pid>/stat` (field 4 `ppid`, field 22 `starttime` in clock
//! ticks since boot, resolved against the boot time `btime` in `/proc/stat`) and
//! the owning uid from the `Uid:` line of `/proc/<pid>/status`. A process the
//! kernel no longer reports, or a zombie that holds no memory and is already
//! gone, is `None`: on macOS `proc_pidinfo` fails for a zombie, and on Linux a
//! `/proc/<pid>/stat` whose state is `Z` is treated the same way, so both
//! platforms agree that a zombie is gone.

#![allow(unsafe_code)]

use crate::raw::Signal;
use crate::sampler::Signaller;

/// A [`Signaller`] for unix: it reads the current user's uid, a process's fresh
/// identity through the platform's process APIs, and delivers `SIGTERM` and
/// `SIGKILL` with `kill(2)`, returning the `errno` when the call fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnixSignaller;

impl UnixSignaller {
  /// A signaller for the current process and user.
  pub fn new() -> Self {
    UnixSignaller
  }
}

impl Signaller for UnixSignaller {
  fn current_uid(&self) -> Option<u32> {
    // `getuid` cannot fail on a POSIX system.
    Some(unsafe { libc::getuid() })
  }

  fn own_pid(&self) -> u32 {
    std::process::id()
  }

  fn identity(&self, pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
    identity(pid)
  }

  fn send(&self, pid: u32, signal: Signal) -> Result<(), i32> {
    let signum = match signal {
      Signal::Term => libc::SIGTERM,
      Signal::Kill => libc::SIGKILL,
    };
    let result = unsafe { libc::kill(pid as libc::pid_t, signum) };
    if result == 0 {
      Ok(())
    } else {
      Err(last_errno())
    }
  }
}

/// The errno of the most recent failed system call, or `0` when the platform
/// exposes none.
fn last_errno() -> i32 {
  std::io::Error::last_os_error()
    .raw_os_error()
    .unwrap_or_default()
}

/// Read the fresh identity of `pid` on this unix, or `None` if it is gone.
#[cfg(target_os = "macos")]
fn identity(pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
  let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
  let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
  let result = unsafe {
    libc::proc_pidinfo(
      pid as libc::c_int,
      libc::PROC_PIDTBSDINFO,
      0,
      &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
      size,
    )
  };
  if result != size {
    return None;
  }
  // `proc_pidinfo` already fails for a zombie, but a status of `SZOMB` is
  // treated as gone regardless, so a signal is never sent to a dead process.
  if info.pbi_status == libc::SZOMB {
    return None;
  }
  let start_time_us = info
    .pbi_start_tvsec
    .saturating_mul(1_000_000)
    .saturating_add(info.pbi_start_tvusec);
  Some((
    start_time_us,
    Some(info.pbi_uid),
    Some(info.pbi_ppid),
    false,
  ))
}

/// Read the fresh identity of `pid` from procfs, or `None` if it is gone.
#[cfg(target_os = "linux")]
fn identity(pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
  let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
  // The command name (field 2) is wrapped in parentheses and may itself contain
  // spaces and parentheses, so parse from the last `)` and count fields from the
  // state field (3) that follows it.
  let after_command = &stat[stat.rfind(')')?.checked_add(1)?..];
  let mut fields = after_command.split_whitespace();
  let state = fields.next()?;
  let ppid: u32 = fields.next()?.parse().ok()?;
  // Field 22, `starttime`, is the 18th token after the state field (fields 5–22).
  let starttime: u64 = fields.nth(17)?.parse().ok()?;
  if state == "Z" {
    // A zombie holds no memory; report it as gone, as macOS does.
    return None;
  }
  let clock_ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
  if clock_ticks <= 0 {
    return None;
  }
  let boot_time_seconds = boot_time_seconds()?;
  let start_time_us = boot_time_seconds
    .saturating_mul(1_000_000)
    .saturating_add(starttime.saturating_mul(1_000_000) / clock_ticks as u64);
  Some((start_time_us, linux_uid(pid), Some(ppid), false))
}

/// Read the second field of `/proc/stat`'s `btime` line, the boot time in
/// seconds since the Unix epoch.
#[cfg(target_os = "linux")]
fn boot_time_seconds() -> Option<u64> {
  let stat = std::fs::read_to_string("/proc/stat").ok()?;
  for line in stat.lines() {
    if let Some(rest) = line.strip_prefix("btime ") {
      return rest.trim().parse().ok();
    }
  }
  None
}

/// Read the real uid of `pid` from the `Uid:` line of `/proc/<pid>/status`.
#[cfg(target_os = "linux")]
fn linux_uid(pid: u32) -> Option<u32> {
  let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
  for line in status.lines() {
    if let Some(rest) = line.strip_prefix("Uid:") {
      return rest.split_whitespace().next()?.parse().ok();
    }
  }
  None
}

/// Read the fresh identity of `pid` on a unix this crate has no reader for.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn identity(_pid: u32) -> Option<(u64, Option<u32>, Option<u32>, bool)> {
  None
}
