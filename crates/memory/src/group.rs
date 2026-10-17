//! Bundle-root grouping (T3 of docs/memory-cleaner-plan.md). Folds the helpers
//! that share an outermost `.app` directory into one [`ProcessGroup`] and
//! derives the [`ProcessRow`]s the table shows, so every rule about "what is
//! one app" lives here and nowhere else. The functions are pure, which lets
//! `tests/group.rs` cover the macOS cases with a fixture and no real machine.
//!
//! A group is keyed by `app:{bundle_root}` for anything that lives inside a
//! `.app`, or by `proc:{pid}:{start_time_us}` otherwise, and the
//! [`ProcessGroup::group_id`] is a stable FNV-1a 64 hash of that key (§4.1).
//! [`ProcessGroup::protection`] is deliberately left `None` here; the safety
//! rules in T4 are the only thing allowed to fill it.

use crate::ids::GroupId;
use crate::model::{ProcessGroup, ProcessKind, ProcessRow};
use crate::raw::{AppInfo, RawProcess};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

/// Executable prefixes whose processes are system components (§5, S4): they run
/// as the user but relaunch, so the view shows them as `System`.
const SYSTEM_PREFIXES: &[&str] = &[
  "/System",
  "/usr/libexec",
  "/usr/sbin",
  "/sbin",
  "/Library/Apple",
];

/// The longest wire `name` (`String /* ≤ 256 bytes */` in §4.1).
const MAX_NAME_BYTES: usize = 256;

/// The outermost ancestor of `exe` whose file name ends in `.app`, if any.
///
/// "Outermost" means the first matching path component, so a helper inside a
/// framework's own `.app` folds into the enclosing application (for example
/// `WeChat.app/Contents/Frameworks/WeChatAppEx.app/...` → `WeChat.app`).
pub fn bundle_root(exe: &Path) -> Option<PathBuf> {
  let mut root = PathBuf::new();
  for component in exe.components() {
    root.push(component.as_os_str());
    if component.as_os_str().to_string_lossy().ends_with(".app") {
      return Some(root);
    }
  }
  None
}

/// The stable group identifier for `key`: `grp_` plus the 16 lowercase hex
/// digits of the FNV-1a 64 hash of the key bytes.
pub fn group_id_for(key: &str) -> GroupId {
  let candidate = format!("grp_{:016x}", fnv1a64(key));
  match GroupId::try_from(candidate) {
    Ok(group_id) => group_id,
    // `fnv1a64` always formats as exactly 16 lowercase hex digits, so the
    // pattern can never fail; there is no fallible path to return instead.
    Err(_) => unreachable!("a 16-digit lowercase hex body always matches GroupId"),
  }
}

/// Whether `exe` is the main executable of `bundle`
/// (`bundle/Contents/MacOS/<file>`, exactly one level below `MacOS`).
pub fn is_main_executable(bundle: &Path, exe: &Path) -> bool {
  let mac_os = bundle.join("Contents").join("MacOS");
  match exe.strip_prefix(&mac_os) {
    Ok(rest) => {
      let mut components = rest.components();
      let is_file = matches!(components.next(), Some(Component::Normal(_)));
      is_file && components.next().is_none()
    }
    Err(_) => false,
  }
}

/// The role of one process in the memory view.
///
/// Order matters: an executable under an S4 prefix is `System` even when it runs
/// as the current user, a process owned by another user is `Other`, a process
/// inside a bundle is `App`/`Agent` when it is the main executable and `Helper`
/// otherwise, and everything else is a `Cli`.
pub fn classify_kind(
  process: &RawProcess,
  bundle: Option<&Path>,
  app: Option<&AppInfo>,
  current_uid: Option<u32>,
) -> ProcessKind {
  if let Some(exe) = process.exe.as_deref() {
    if is_system_executable(exe) {
      return ProcessKind::System;
    }
  }
  if is_other_user(process, current_uid) {
    return ProcessKind::Other;
  }
  match bundle {
    Some(root) => match process.exe.as_deref() {
      Some(exe) if is_main_executable(root, exe) => match app {
        Some(info) if !info.regular => ProcessKind::Agent,
        _ => ProcessKind::App,
      },
      _ => ProcessKind::Helper,
    },
    None => ProcessKind::Cli,
  }
}

/// Group every process and derive the table rows for one sample.
///
/// The result is deterministic: groups are ordered by [`GroupId`], rows by pid,
/// and each [`ProcessGroup::pids`] is sorted ascending, so two calls with the
/// same input compare equal.
pub fn build_groups(
  processes: &[RawProcess],
  apps: &[AppInfo],
  frontmost: Option<u32>,
  current_uid: Option<u32>,
) -> (Vec<ProcessGroup>, Vec<ProcessRow>) {
  let apps_by_pid: HashMap<u32, &AppInfo> = apps.iter().map(|app| (app.pid, app)).collect();

  let mut bundles: Vec<Option<PathBuf>> = Vec::with_capacity(processes.len());
  let mut group_ids: Vec<GroupId> = Vec::with_capacity(processes.len());
  let mut buckets: BTreeMap<String, Vec<usize>> = BTreeMap::new();

  for (index, process) in processes.iter().enumerate() {
    let bundle = process.exe.as_deref().and_then(bundle_root);
    let key = group_key(process, bundle.as_deref());
    group_ids.push(group_id_for(&key));
    buckets.entry(key).or_default().push(index);
    bundles.push(bundle);
  }

  let mut groups: Vec<ProcessGroup> = Vec::with_capacity(buckets.len());
  for members in buckets.values() {
    let first = match members.first() {
      Some(index) => *index,
      None => continue,
    };
    let bundle = bundles[first].clone();

    let main_index = match main_executable_index(members, processes, &bundles) {
      Some(index) => index,
      None => continue,
    };
    let main_process = &processes[main_index];
    let main_app = apps_by_pid.get(&main_process.pid).copied();

    let mut pids: Vec<u32> = members.iter().map(|index| processes[*index].pid).collect();
    pids.sort_unstable();

    let mut footprint_bytes: u64 = 0;
    let mut resident_bytes: u64 = 0;
    let mut cpu_sum: f32 = 0.0;
    let mut cpu_seen = false;
    for index in members {
      let process = &processes[*index];
      footprint_bytes += match (process.footprint_bytes, process.resident_bytes) {
        (Some(footprint), _) => footprint,
        (None, Some(resident)) => resident,
        (None, None) => 0,
      };
      if let Some(resident) = process.resident_bytes {
        resident_bytes += resident;
      }
      if let Some(cpu) = process.cpu_percent {
        cpu_sum += cpu;
        cpu_seen = true;
      }
    }

    let (bundle_id, bundle_path) = match bundle.as_ref() {
      Some(root) => (
        main_app.and_then(|app| app.bundle_id.clone()),
        Some(root.to_string_lossy().into_owned()),
      ),
      None => (None, None),
    };

    let started_at = members
      .iter()
      .map(|index| processes[*index].start_time_us)
      .min()
      .map(iso_from_us)
      .unwrap_or_default();

    groups.push(ProcessGroup {
      group_id: group_ids[first].clone(),
      name: group_name(main_process, main_app, bundle.as_ref()),
      bundle_id,
      bundle_path,
      kind: classify_kind(main_process, bundle.as_deref(), main_app, current_uid),
      main_pid: main_process.pid,
      pids,
      cpu_percent: if cpu_seen { Some(cpu_sum) } else { None },
      footprint_bytes,
      resident_bytes,
      frontmost: match frontmost {
        Some(pid) => members.iter().any(|index| processes[*index].pid == pid),
        None => false,
      },
      last_frontmost_at: None,
      started_at,
      protection: None,
    });
  }
  groups.sort_by(|left, right| left.group_id.as_str().cmp(right.group_id.as_str()));

  let mut rows: Vec<ProcessRow> = processes
    .iter()
    .enumerate()
    .map(|(index, process)| ProcessRow {
      pid: process.pid,
      ppid: process.ppid,
      start_time_us: process.start_time_us,
      name: clamp_name(&process.name),
      owned_by_current_user: owns_process(process, current_uid),
      group_id: group_ids[index].clone(),
      kind: classify_kind(
        process,
        bundles[index].as_deref(),
        apps_by_pid.get(&process.pid).copied(),
        current_uid,
      ),
      cpu_percent: process.cpu_percent,
      footprint_bytes: process.footprint_bytes,
      resident_bytes: process.resident_bytes,
      virtual_bytes: process.virtual_bytes,
      restricted: process.footprint_bytes.is_none(),
      protection: None,
    })
    .collect();
  rows.sort_by_key(|row| row.pid);

  (groups, rows)
}

/// The index of the group's main process: the member whose executable is
/// `bundle/Contents/MacOS/<file>`, or failing that the earliest-started member.
fn main_executable_index(
  members: &[usize],
  processes: &[RawProcess],
  bundles: &[Option<PathBuf>],
) -> Option<usize> {
  let main = members
    .iter()
    .copied()
    .filter(
      |index| match (bundles[*index].as_deref(), processes[*index].exe.as_deref()) {
        (Some(bundle), Some(exe)) => is_main_executable(bundle, exe),
        _ => false,
      },
    )
    .min_by_key(|index| processes[*index].start_time_us);
  main.or_else(|| {
    members
      .iter()
      .copied()
      .min_by_key(|index| processes[*index].start_time_us)
  })
}

/// `app:{bundle_root}` or `proc:{pid}:{start_time_us}`.
fn group_key(process: &RawProcess, bundle: Option<&Path>) -> String {
  match bundle {
    Some(root) => format!("app:{}", root.to_string_lossy()),
    None => format!("proc:{}:{}", process.pid, process.start_time_us),
  }
}

/// AppInfo name of the main pid → bundle file stem → process name.
fn group_name(
  main_process: &RawProcess,
  main_app: Option<&AppInfo>,
  bundle: Option<&PathBuf>,
) -> String {
  let app_name = main_app
    .and_then(|app| app.name.as_deref())
    .filter(|name| !name.is_empty());
  let bundle_stem = bundle
    .and_then(|root| root.file_stem())
    .map(|stem| stem.to_string_lossy().into_owned());
  let name = match (app_name, bundle_stem) {
    (Some(app_name), _) => app_name.to_string(),
    (None, Some(stem)) => stem,
    (None, None) => main_process.name.clone(),
  };
  clamp_name(&name)
}

/// Truncate a name to at most [`MAX_NAME_BYTES`] bytes on a UTF-8 boundary.
fn clamp_name(name: &str) -> String {
  if name.len() <= MAX_NAME_BYTES {
    return name.to_string();
  }
  let mut end = MAX_NAME_BYTES;
  while end > 0 && !name.is_char_boundary(end) {
    end -= 1;
  }
  name[..end].to_string()
}

/// Whether the process belongs to the current user.
fn owns_process(process: &RawProcess, current_uid: Option<u32>) -> bool {
  matches!((process.uid, current_uid), (Some(owner), Some(current)) if owner == current)
}

/// Whether the process belongs to a different, known user.
fn is_other_user(process: &RawProcess, current_uid: Option<u32>) -> bool {
  matches!((process.uid, current_uid), (Some(owner), Some(current)) if owner != current)
}

/// Whether `exe` sits under an executable prefix listed in §5, S4.
fn is_system_executable(exe: &Path) -> bool {
  SYSTEM_PREFIXES
    .iter()
    .any(|prefix| exe.starts_with(*prefix))
}

/// FNV-1a 64 over the UTF-8 bytes of `key`.
fn fnv1a64(key: &str) -> u64 {
  const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
  const PRIME: u64 = 0x0000_0100_0000_01b3;
  let mut hash = OFFSET_BASIS;
  for byte in key.as_bytes() {
    hash ^= u64::from(*byte);
    hash = hash.wrapping_mul(PRIME);
  }
  hash
}

/// Render microseconds since the Unix epoch as an ISO-8601 UTC instant with
/// millisecond precision (`2026-10-18T09:00:00.000Z`), matching `z.iso.datetime`.
fn iso_from_us(start_time_us: u64) -> String {
  let total_seconds = (start_time_us / 1_000_000) as i64;
  let millis = ((start_time_us % 1_000_000) / 1_000) as u32;
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
