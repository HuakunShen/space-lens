//! The portable, sysinfo-only sampler (T7 of docs/memory-cleaner-plan.md):
//! system memory totals, swap and one row per process, without an
//! operating-system specific reading. It is the fallback wherever sysinfo runs
//! and the base the macOS sampler (T8) enriches with per-process footprints,
//! VM page counters and kernel pressure.
//!
//! Everything sysinfo cannot supply stays `None` rather than being guessed: a
//! portable sample has no [`RawProcess::footprint_bytes`] (which is the macOS
//! `ri_phys_footprint` concept) and no [`RawSystem::vm`] page counters, so the
//! view falls back to RSS and says so. Pressure is derived from the share of
//! memory still available and marked [`PressureSource::Derived`], never passed
//! off as a kernel reading.

use crate::error::MemoryError;
use crate::model::{MemoryPressure, Platform, PressureLevel, PressureSource, ProcessState};
use crate::raw::{RawProcess, RawSample, RawSystem};
use crate::sampler::ProcessSampler;
use std::path::Path;
use sysinfo::{
  ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, RefreshKind, System, UpdateKind, Users,
};

/// Below this share of total memory available, pressure is `Critical`.
const CRITICAL_AVAILABLE_PERCENT: f32 = 10.0;

/// Below this share of total memory available, pressure is `Warning`.
const WARNING_AVAILABLE_PERCENT: f32 = 20.0;

/// A [`ProcessSampler`] built on a retained [`sysinfo::System`].
///
/// The [`System`] is kept between ticks because sysinfo computes CPU usage from
/// the difference between two refreshes; the first tick therefore reports
/// [`RawProcess::cpu_percent`] as `None` (the service marks that snapshot
/// `cpuWarmingUp`) and every later tick reports a real value.
#[derive(Debug)]
pub struct PortableSampler {
  system: System,
  /// The account list, loaded once: process owner ids are stable for the life
  /// of a machine session, and a per-tick passwd read would be wasteful.
  users: Users,
  /// Whether a previous process refresh exists to diff CPU usage against.
  warmed_up: bool,
}

impl PortableSampler {
  /// A sampler that has not taken a sample yet.
  pub fn new() -> Self {
    PortableSampler {
      system: System::new_with_specifics(RefreshKind::nothing()),
      users: Users::new_with_refreshed_list(),
      warmed_up: false,
    }
  }
}

impl Default for PortableSampler {
  fn default() -> Self {
    PortableSampler::new()
  }
}

/// What to read about each process (§4.2): CPU, memory, the executable path,
/// the owner uid, the command line, the scheduler state and the thread count.
/// The command line and executable path are read once per pid (`OnlyIfNotSet`)
/// because neither changes for a live process. Environments, working and root
/// directories and disk usage are never read: the memory view does not show
/// them, and the environment in particular can hold secrets.
fn process_refresh_kind() -> ProcessRefreshKind {
  ProcessRefreshKind::nothing()
    .with_cpu()
    .with_memory()
    .with_exe(UpdateKind::OnlyIfNotSet)
    .with_user(UpdateKind::OnlyIfNotSet)
    .with_cmd(UpdateKind::OnlyIfNotSet)
    .with_tasks()
}

impl ProcessSampler for PortableSampler {
  fn platform(&self) -> Platform {
    platform()
  }

  fn sample(&mut self) -> Result<RawSample, MemoryError> {
    self.system.refresh_memory();
    self
      .system
      .refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh_kind());

    let warmed_up = self.warmed_up;
    self.warmed_up = true;

    let total_bytes = self.system.total_memory();
    let available_bytes = self.system.available_memory();

    let processes: Vec<RawProcess> = self
      .system
      .processes()
      .values()
      .map(|process| RawProcess {
        pid: process.pid().as_u32(),
        ppid: process.parent().map(|parent| parent.as_u32()),
        start_time_us: process.start_time() * 1_000_000,
        uid: process_uid(process),
        name: process.name().to_string_lossy().into_owned(),
        exe: process.exe().map(Path::to_path_buf),
        command: command_line(process),
        user: process
          .user_id()
          .and_then(|uid| self.users.get_user_by_id(uid))
          .map(|user| user.name().to_string()),
        state: process_state(process.status()),
        threads: process.tasks().map(|tasks| tasks.len() as u32),
        listening_ports: Vec::new(),
        cpu_percent: if warmed_up {
          Some(process.cpu_usage())
        } else {
          None
        },
        footprint_bytes: None,
        resident_bytes: Some(process.memory()),
        virtual_bytes: Some(process.virtual_memory()),
        zombie: process.status() == ProcessStatus::Zombie,
      })
      .collect();

    Ok(RawSample {
      system: RawSystem {
        total_bytes,
        used_bytes: self.system.used_memory(),
        free_bytes: self.system.free_memory(),
        available_bytes: Some(available_bytes),
        swap_total_bytes: self.system.total_swap(),
        swap_used_bytes: self.system.used_swap(),
        vm: None,
        pressure: derive_pressure(total_bytes, available_bytes),
      },
      processes,
    })
  }
}

/// The command line of `process`, joined with spaces, or `None` when the
/// platform exposes no argv (or refuses this caller).
fn command_line(process: &sysinfo::Process) -> Option<String> {
  command_line_from_args(
    process
      .cmd()
      .iter()
      .map(|argument| argument.to_string_lossy().into_owned()),
  )
}

/// Join argv, stopping before anything that looks like an environment
/// assignment.
///
/// macOS keeps argv and the environment in one `KERN_PROCARGS2` buffer, and a
/// process that calls `setproctitle` can leave empty argv slots between them.
/// sysinfo's parser fills those slots from the environment in that case
/// (observed with Raycast Backend: argc 4, three empty argv slots, then
/// `COMMAND_MODE=…`). Stopping at the first `KEY=value` token keeps the
/// environment out of the memory wire; a legitimate `make FOO=bar` argument is
/// truncated too, which is the safer trade.
fn command_line_from_args(arguments: impl Iterator<Item = String>) -> Option<String> {
  let mut line = String::new();
  for argument in arguments {
    if is_environment_assignment(&argument) {
      break;
    }
    if !line.is_empty() {
      line.push(' ');
    }
    line.push_str(&argument);
  }
  let line = line.trim();
  (!line.is_empty()).then(|| line.to_string())
}

/// Whether `argument` is a POSIX-style `NAME=value` environment entry.
fn is_environment_assignment(argument: &str) -> bool {
  let Some((name, _)) = argument.split_once('=') else {
    return false;
  };
  let mut chars = name.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  if !(first.is_ascii_alphabetic() || first == '_') {
    return false;
  }
  chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Map sysinfo's platform-specific statuses onto the six wire states.
fn process_state(status: ProcessStatus) -> ProcessState {
  match status {
    ProcessStatus::Run => ProcessState::Running,
    ProcessStatus::Sleep => ProcessState::Sleeping,
    ProcessStatus::Idle => ProcessState::Idle,
    ProcessStatus::Stop | ProcessStatus::Tracing => ProcessState::Stopped,
    ProcessStatus::Zombie => ProcessState::Zombie,
    _ => ProcessState::Unknown,
  }
}

/// The platform this build targets, resolved at compile time.
fn platform() -> Platform {
  if cfg!(target_os = "macos") {
    Platform::Macos
  } else if cfg!(target_os = "linux") {
    Platform::Linux
  } else if cfg!(target_os = "windows") {
    Platform::Windows
  } else {
    Platform::Unsupported
  }
}

/// Derive pressure from the share of physical memory still available (§4.2).
///
/// Below 10 % the level is `Critical`, below 20 % `Warning`, and otherwise
/// `Normal`; the source is [`PressureSource::Derived`] and the share is
/// reported so the view can say where the level came from. With no memory total
/// to divide by there is nothing to derive, so the reading is `Unknown` and the
/// source [`PressureSource::Unavailable`].
fn derive_pressure(total_bytes: u64, available_bytes: u64) -> MemoryPressure {
  if total_bytes == 0 {
    return MemoryPressure {
      level: PressureLevel::Unknown,
      source: PressureSource::Unavailable,
      available_percent: None,
    };
  }
  let available_percent = available_bytes as f32 / total_bytes as f32 * 100.0;
  let level = if available_percent < CRITICAL_AVAILABLE_PERCENT {
    PressureLevel::Critical
  } else if available_percent < WARNING_AVAILABLE_PERCENT {
    PressureLevel::Warning
  } else {
    PressureLevel::Normal
  };
  MemoryPressure {
    level,
    source: PressureSource::Derived,
    available_percent: Some(available_percent),
  }
}

/// The numeric owner of `process`. On Windows `sysinfo` reports a SID, not a
/// `u32`; the owner is then unknown, which the safety rules treat as another
/// user (S7), so nothing is ever quit there by mistake.
#[cfg(unix)]
fn process_uid(process: &sysinfo::Process) -> Option<u32> {
  process.user_id().map(|uid| **uid)
}

#[cfg(not(unix))]
fn process_uid(_process: &sysinfo::Process) -> Option<u32> {
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pressure_levels_escalate_at_the_documented_shares() {
    let cases = [
      (50, 50.0, PressureLevel::Normal),
      (20, 20.0, PressureLevel::Normal),
      (19, 19.0, PressureLevel::Warning),
      (10, 10.0, PressureLevel::Warning),
      (9, 9.0, PressureLevel::Critical),
      (0, 0.0, PressureLevel::Critical),
    ];
    for (available, expected_percent, expected_level) in cases {
      let pressure = derive_pressure(100, available);
      assert_eq!(pressure.level, expected_level, "{available}% available");
      assert_eq!(pressure.source, PressureSource::Derived);
      let percent = pressure.available_percent.expect("a derived share");
      assert!(
        (percent - expected_percent).abs() < 0.001,
        "{available}% available became {percent}"
      );
    }
  }

  #[test]
  fn pressure_without_a_memory_total_is_unknown() {
    let pressure = derive_pressure(0, 0);
    assert_eq!(pressure.level, PressureLevel::Unknown);
    assert_eq!(pressure.source, PressureSource::Unavailable);
    assert_eq!(pressure.available_percent, None);
  }

  #[test]
  fn command_line_stops_before_the_environment() {
    let arguments = [
      "Raycast Backend".to_string(),
      "COMMAND_MODE=unix2003".to_string(),
      "HOME=/Users/hk".to_string(),
    ];
    assert_eq!(
      command_line_from_args(arguments.into_iter()).as_deref(),
      Some("Raycast Backend")
    );
    let arguments = ["node", "server.js", "--port", "3000"]
      .into_iter()
      .map(str::to_string);
    assert_eq!(
      command_line_from_args(arguments).as_deref(),
      Some("node server.js --port 3000")
    );
  }

  #[test]
  fn environment_assignment_detection_is_narrow() {
    assert!(is_environment_assignment("HOME=/Users/hk"));
    assert!(is_environment_assignment("_FOO=1"));
    assert!(!is_environment_assignment("--foo=bar"));
    assert!(!is_environment_assignment("PATH"));
    assert!(!is_environment_assignment("1FOO=bar"));
  }

  #[test]
  fn platform_matches_the_build_target() {
    let expected = if cfg!(target_os = "macos") {
      Platform::Macos
    } else if cfg!(target_os = "linux") {
      Platform::Linux
    } else if cfg!(target_os = "windows") {
      Platform::Windows
    } else {
      Platform::Unsupported
    };
    assert_eq!(platform(), expected);
    assert_eq!(PortableSampler::new().platform(), expected);
  }
}
