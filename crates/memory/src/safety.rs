//! Kill-safety rules (§5 of docs/memory-cleaner-plan.md): the single place that
//! decides whether a process may be terminated. [`classify`] applies the rules
//! in the frozen order S1, S2, S3, S8-zombie, S5, S6, S7, S4, S8-unidentified
//! (first match wins) and is used both when a snapshot is displayed and, again,
//! when a plan is built and executed against a fresh identity read. [`apply`]
//! fills the `protection` field of a snapshot's rows and groups, where a group
//! is protected exactly when its main process is.
//!
//! Nothing here touches the operating system: it works on [`RawProcess`] values
//! and the [`SafetyContext`] gathered once per sample, so the rules are unit
//! tested with synthetic processes.

use crate::ids::GroupId;
use crate::model::{ProcessGroup, ProcessRow, Protection};
use crate::raw::RawProcess;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// How far the ancestor walk may climb before giving up (§5, S5). The bound also
/// makes the walk terminate on a `ppid` cycle.
pub const MAX_ANCESTOR_HOPS: usize = 64;

/// Process names that must never be terminated because the session ends with
/// them (§5, S3). macOS and Linux names are both listed; the match is exact and
/// case-sensitive.
const SESSION_CRITICAL_NAMES: &[&str] = &[
  "WindowServer",
  "loginwindow",
  "launchd",
  "logind",
  "systemd",
  "init",
  "Xorg",
  "Xwayland",
  "gnome-shell",
  "kwin_wayland",
  "kwin_x11",
  "plasmashell",
  "gdm",
  "gdm-session-worker",
  "sddm",
  "dbus-daemon",
  "dbus-broker",
];

/// Executable prefixes whose processes are system components (§5, S4).
const SYSTEM_PREFIXES: &[&str] = &[
  "/System",
  "/usr/libexec",
  "/usr/sbin",
  "/sbin",
  "/Library/Apple",
];

/// Regular Apple apps (Notes, TextEdit, Calendar, Safari, ...) live under
/// `/System/`, but they are ordinary apps the user may quit; only the system
/// components elsewhere in `/System` (Dock, Finder, Control Center, ...) stay
/// protected.
const USER_APP_PREFIXES: &[&str] = &[
  "/System/Applications",
  "/System/Cryptexes/App/System/Applications",
];

/// The facts a single sample establishes about the running service (§5,
/// S5–S7): its own pid and uid, its ancestors and descendants, and the group it
/// belongs to.
#[derive(Clone, Debug)]
pub struct SafetyContext {
  /// The pid of the process running the memory service.
  pub own_pid: u32,
  /// The uid of the current user, if it can be determined.
  pub current_uid: Option<u32>,
  /// Every ancestor of `own_pid`, up to [`MAX_ANCESTOR_HOPS`] hops.
  pub ancestors: HashSet<u32>,
  /// Every descendant of `own_pid`.
  pub descendants: HashSet<u32>,
  /// The group that contains `own_pid`, if it is in the sample.
  pub own_group: Option<GroupId>,
}

/// Build the safety context for one sample.
///
/// Ancestors are found by walking `ppid` from `own_pid`, at most
/// [`MAX_ANCESTOR_HOPS`] hops, so a `ppid` cycle terminates instead of hanging.
/// Descendants are found by following parent links down from `own_pid`, and the
/// own group is the group whose members contain `own_pid`.
pub fn context(
  processes: &[RawProcess],
  own_pid: u32,
  current_uid: Option<u32>,
  groups: &[ProcessGroup],
) -> SafetyContext {
  let parents: HashMap<u32, Option<u32>> = processes.iter().map(|p| (p.pid, p.ppid)).collect();

  let mut ancestors = HashSet::new();
  let mut cursor = own_pid;
  for _ in 0..MAX_ANCESTOR_HOPS {
    let parent = match parents.get(&cursor).copied().flatten() {
      Some(parent) if parent != 0 => parent,
      _ => break,
    };
    ancestors.insert(parent);
    if parent == 1 {
      break;
    }
    cursor = parent;
  }

  let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
  for process in processes {
    if let Some(ppid) = process.ppid {
      children.entry(ppid).or_default().push(process.pid);
    }
  }
  let mut descendants = HashSet::new();
  let mut stack = vec![own_pid];
  while let Some(pid) = stack.pop() {
    if let Some(kids) = children.get(&pid) {
      for &child in kids {
        if descendants.insert(child) {
          stack.push(child);
        }
      }
    }
  }

  let own_group = groups
    .iter()
    .find(|group| group.pids.contains(&own_pid))
    .map(|group| group.group_id.clone());

  SafetyContext {
    own_pid,
    current_uid,
    ancestors,
    descendants,
    own_group,
  }
}

/// Decide whether `p` may be terminated, applying S1…S8 in order and returning
/// the first matching [`Protection`]. `None` means the process is unprotected.
pub fn classify(p: &RawProcess, ctx: &SafetyContext, group: &GroupId) -> Option<Protection> {
  // S1: the kernel, or anything that calls itself `kernel_task`.
  if p.pid == 0 || p.name.as_str() == "kernel_task" {
    return Some(Protection::Kernel);
  }
  // S2: init / launchd is the root of the process tree.
  if p.pid == 1 {
    return Some(Protection::Init);
  }
  // S3: names whose loss would end or break the session.
  if SESSION_CRITICAL_NAMES.contains(&p.name.as_str()) {
    return Some(Protection::SessionCritical);
  }
  // S8, first half: a zombie holds no memory and is already gone.
  if p.zombie {
    return Some(Protection::Zombie);
  }
  // S5: the service itself, its ancestors, or anything in its group.
  if p.pid == ctx.own_pid || ctx.ancestors.contains(&p.pid) || ctx.own_group.as_ref() == Some(group)
  {
    return Some(Protection::SelfOrAncestor);
  }
  // S6: a descendant of the service (the stdio child, the scan CLI).
  if ctx.descendants.contains(&p.pid) {
    return Some(Protection::SelfChild);
  }
  // S7: another user, or an unknown owner; there is no elevation path in v1.
  if !owns_process(p.uid, ctx.current_uid) {
    return Some(Protection::OtherUser);
  }
  // S4: a system component that would only relaunch.
  if let Some(exe) = p.exe.as_deref() {
    if is_system_executable(exe) {
      return Some(Protection::SystemComponent);
    }
  }
  // S8, second half: nothing to name it, nothing to show it.
  if p.name.is_empty() && p.exe.is_none() {
    return Some(Protection::Unidentified);
  }
  None
}

/// Fill `protection` on the rows and groups of one snapshot.
///
/// Each row's protection comes from [`classify`]; each group is protected
/// exactly when its main process is, so the group's state can never disagree
/// with the row it points at.
pub fn apply(
  groups: &mut [ProcessGroup],
  rows: &mut [ProcessRow],
  processes: &[RawProcess],
  ctx: &SafetyContext,
) {
  let group_by_pid: HashMap<u32, GroupId> = rows
    .iter()
    .map(|row| (row.pid, row.group_id.clone()))
    .collect();

  let mut protections: HashMap<u32, Option<Protection>> = HashMap::with_capacity(processes.len());
  for process in processes {
    let protection = group_by_pid
      .get(&process.pid)
      .and_then(|group| classify(process, ctx, group));
    protections.insert(process.pid, protection);
  }

  for row in rows.iter_mut() {
    row.protection = protections.get(&row.pid).copied().flatten();
  }
  for group in groups.iter_mut() {
    group.protection = protections.get(&group.main_pid).copied().flatten();
  }
}

/// Whether the process belongs to the current user; an unknown uid on either
/// side fails the check, so it is treated as another user.
fn owns_process(uid: Option<u32>, current_uid: Option<u32>) -> bool {
  matches!((uid, current_uid), (Some(owner), Some(current)) if owner == current)
}

/// Whether `exe` sits under a system-component prefix listed in §5, S4.
fn is_system_executable(exe: &Path) -> bool {
  if USER_APP_PREFIXES
    .iter()
    .any(|prefix| exe.starts_with(*prefix))
  {
    return false;
  }
  SYSTEM_PREFIXES
    .iter()
    .any(|prefix| exe.starts_with(*prefix))
}
