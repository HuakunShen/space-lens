//! Kill-safety rule cases from §5 of docs/memory-cleaner-plan.md. One test per
//! rule id S1–S8, plus the 64-hop cap on the ancestor walk. Everything runs on
//! synthetic [`RawProcess`] values and the fakes in `src/fake.rs`, so no real
//! machine is touched.

use spacelens_memory::fake::{FakeClock, FakeSignaller, FakeSleeper, Identity};
use spacelens_memory::group::{build_groups, group_id_for};
use spacelens_memory::raw::Signal;
use spacelens_memory::safety::{apply, classify, context, SafetyContext};
use spacelens_memory::sampler::{Clock, Signaller, Sleeper};
use spacelens_memory::{GroupId, ProcessGroup, Protection, RawProcess};
use std::path::PathBuf;
use std::sync::Arc;

/// Build one raw process with sensible defaults; the fields a test does not
/// care about stay `None` so a rule can only fire because of the field it sets.
fn process(
  pid: u32,
  ppid: Option<u32>,
  uid: Option<u32>,
  name: &str,
  exe: Option<&str>,
  zombie: bool,
) -> RawProcess {
  RawProcess {
    pid,
    ppid,
    start_time_us: 1_700_000_000_000_000 + u64::from(pid),
    uid,
    name: name.to_string(),
    exe: exe.map(PathBuf::from),
    command: Some("worker --job 42".to_string()),
    user: Some("hk".to_string()),
    state: spacelens_memory::ProcessState::Sleeping,
    threads: Some(2),
    listening_ports: vec![],
    cpu_percent: None,
    footprint_bytes: None,
    resident_bytes: None,
    virtual_bytes: None,
    zombie,
  }
}

/// A stable group id derived from a seed string.
fn group(seed: &str) -> GroupId {
  group_id_for(seed)
}

/// A context with hand-picked sets, for rules that do not depend on the walk.
fn ctx_for(own_pid: u32, ancestors: &[u32], descendants: &[u32]) -> SafetyContext {
  SafetyContext {
    own_pid,
    current_uid: Some(501),
    ancestors: ancestors.iter().copied().collect(),
    descendants: descendants.iter().copied().collect(),
    own_group: None,
  }
}

fn group_by_bundle<'a>(groups: &'a [ProcessGroup], path: &str) -> &'a ProcessGroup {
  groups
    .iter()
    .find(|group| group.bundle_path.as_deref() == Some(path))
    .unwrap_or_else(|| panic!("no group for bundle {path}"))
}

#[test]
fn s1_kernel_task_is_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  assert_eq!(
    classify(
      &process(0, None, Some(0), "kernel_task", None, false),
      &ctx,
      &g
    ),
    Some(Protection::Kernel)
  );
  // The name alone is enough, whatever the pid.
  assert_eq!(
    classify(
      &process(42, Some(1), Some(0), "kernel_task", None, false),
      &ctx,
      &g
    ),
    Some(Protection::Kernel)
  );
  // S1 outranks every later rule, even another user and a zombie.
  assert_eq!(
    classify(
      &process(43, Some(1), Some(999), "kernel_task", Some("/x"), true),
      &ctx,
      &g
    ),
    Some(Protection::Kernel)
  );
}

#[test]
fn s2_init_is_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  assert_eq!(
    classify(
      &process(1, Some(0), Some(0), "launchd", Some("/sbin/launchd"), false),
      &ctx,
      &g
    ),
    Some(Protection::Init)
  );
}

#[test]
fn s3_session_critical_is_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  for name in [
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
  ] {
    let p = process(500, Some(1), Some(501), name, Some("/usr/bin/x"), false);
    assert_eq!(
      classify(&p, &ctx, &g),
      Some(Protection::SessionCritical),
      "{name} should be session-critical"
    );
  }

  // The match is exact and case-sensitive: a lowercase lookalike with a normal
  // executable is not protected by S3.
  let lookalike = process(
    500,
    Some(1),
    Some(501),
    "windowserver",
    Some("/usr/bin/x"),
    false,
  );
  assert_eq!(classify(&lookalike, &ctx, &g), None);

  // A process with no name and no exe is Unidentified, not session-critical.
  let anonymous = process(500, Some(1), Some(501), "", None, false);
  assert_eq!(
    classify(&anonymous, &ctx, &g),
    Some(Protection::Unidentified)
  );
}

#[test]
fn s4_system_component_is_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  for exe in [
    "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder",
    "/usr/libexec/opendirectoryd",
    "/usr/sbin/syslogd",
    "/sbin/launchd",
    "/Library/Apple/System/Library/PrivateFrameworks/x",
  ] {
    let p = process(400, Some(1), Some(501), "Finder", Some(exe), false);
    assert_eq!(
      classify(&p, &ctx, &g),
      Some(Protection::SystemComponent),
      "{exe} should be a system component"
    );
  }
}

#[test]
fn s4_regular_apple_apps_under_system_applications_are_not_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  // Ordinary Apple apps live under /System but the user may quit them.
  for exe in [
    "/System/Applications/TextEdit.app/Contents/MacOS/TextEdit",
    "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
    "/System/Cryptexes/App/System/Applications/Safari.app/Contents/MacOS/Safari",
  ] {
    let p = process(400, Some(1), Some(501), "App", Some(exe), false);
    assert_eq!(classify(&p, &ctx, &g), None, "{exe} should be quittable");
  }

  // The system components elsewhere in /System stay protected.
  for exe in [
    "/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock",
    "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter",
    "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder",
  ] {
    let p = process(401, Some(1), Some(501), "Sys", Some(exe), false);
    assert_eq!(
      classify(&p, &ctx, &g),
      Some(Protection::SystemComponent),
      "{exe} should stay protected"
    );
  }
}

#[test]
fn s5_self_or_ancestor_is_protected() {
  // own pid 1001 is a helper inside Dia.app, so the whole Dia group is the
  // own group; the main process is its ancestor, and a sibling is neither an
  // ancestor nor a descendant but still shares the group.
  let processes = vec![
    process(
      1000,
      Some(1),
      Some(501),
      "Dia",
      Some("/Applications/Dia.app/Contents/MacOS/Dia"),
      false,
    ),
    process(
      1001,
      Some(1000),
      Some(501),
      "Dia Helper",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper.app/Contents/MacOS/Dia Helper"),
      false,
    ),
    process(
      1002,
      Some(1),
      Some(501),
      "Dia Helper (GPU)",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper (GPU).app/Contents/MacOS/Dia Helper (GPU)"),
      false,
    ),
  ];
  let (groups, _) = build_groups(&processes, &[], None, Some(501));
  let ctx = context(&processes, 1001, Some(501), &groups);

  let dia = group_by_bundle(&groups, "/Applications/Dia.app")
    .group_id
    .clone();
  assert_eq!(ctx.own_group, Some(dia.clone()));
  assert!(ctx.ancestors.contains(&1000));

  assert_eq!(
    classify(&processes[0], &ctx, &dia),
    Some(Protection::SelfOrAncestor),
    "the group main is an ancestor"
  );
  assert_eq!(
    classify(&processes[1], &ctx, &dia),
    Some(Protection::SelfOrAncestor),
    "the own pid is protected"
  );
  assert_eq!(
    classify(&processes[2], &ctx, &dia),
    Some(Protection::SelfOrAncestor),
    "a sibling in the own group is protected even without an ancestry link"
  );
}

#[test]
fn s6_self_child_is_protected() {
  let processes = vec![
    process(
      500,
      Some(1),
      Some(501),
      "spacelens",
      Some("/usr/local/bin/spacelens"),
      false,
    ),
    process(
      501,
      Some(500),
      Some(501),
      "spacelens-memory",
      Some("/usr/local/bin/spacelens"),
      false,
    ),
    process(
      502,
      Some(501),
      Some(501),
      "node",
      Some("/usr/local/bin/node"),
      false,
    ),
  ];
  let (groups, _) = build_groups(&processes, &[], None, Some(501));
  let ctx = context(&processes, 500, Some(501), &groups);

  assert_eq!(ctx.descendants, [501, 502].into_iter().collect());

  let g = group("x");
  assert_eq!(
    classify(&processes[1], &ctx, &g),
    Some(Protection::SelfChild)
  );
  assert_eq!(
    classify(&processes[2], &ctx, &g),
    Some(Protection::SelfChild)
  );
}

#[test]
fn s7_other_user_is_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  assert_eq!(
    classify(
      &process(
        600,
        Some(1),
        Some(502),
        "sshd",
        Some("/usr/bin/sshd"),
        false
      ),
      &ctx,
      &g
    ),
    Some(Protection::OtherUser)
  );
  // An unknown uid is treated as another user.
  assert_eq!(
    classify(
      &process(
        601,
        Some(1),
        None,
        "mystery",
        Some("/usr/bin/mystery"),
        false
      ),
      &ctx,
      &g
    ),
    Some(Protection::OtherUser)
  );
  // S7 outranks S4: a root daemon under a system prefix is another user.
  assert_eq!(
    classify(
      &process(
        602,
        Some(1),
        Some(0),
        "syslogd",
        Some("/usr/sbin/syslogd"),
        false
      ),
      &ctx,
      &g
    ),
    Some(Protection::OtherUser)
  );
}

#[test]
fn s8_zombie_and_unidentified_are_protected() {
  let ctx = ctx_for(9000, &[], &[]);
  let g = group("x");

  // S8 zombie is checked before the self rules.
  assert_eq!(
    classify(
      &process(700, Some(1), Some(501), "zombie_proc", None, true),
      &ctx,
      &g
    ),
    Some(Protection::Zombie)
  );
  // Zombie outranks S7 (another user).
  assert_eq!(
    classify(
      &process(701, Some(1), Some(502), "zombie_proc", Some("/x"), true),
      &ctx,
      &g
    ),
    Some(Protection::Zombie)
  );
  // No name and no exe is checked last.
  assert_eq!(
    classify(&process(702, Some(1), Some(501), "", None, false), &ctx, &g),
    Some(Protection::Unidentified)
  );
  // A process with a name is not unidentified, so it stays unprotected.
  assert_eq!(
    classify(
      &process(703, Some(1), Some(501), "named", None, false),
      &ctx,
      &g
    ),
    None
  );
}

#[test]
fn apply_fills_rows_and_protects_a_group_when_its_main_is_protected() {
  let processes = vec![
    process(1, Some(0), Some(0), "launchd", Some("/sbin/launchd"), false),
    process(
      400,
      Some(1),
      Some(501),
      "Finder",
      Some("/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder"),
      false,
    ),
    process(
      1000,
      Some(1),
      Some(501),
      "Dia",
      Some("/Applications/Dia.app/Contents/MacOS/Dia"),
      false,
    ),
    process(
      1001,
      Some(1000),
      Some(501),
      "Dia Helper",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper.app/Contents/MacOS/Dia Helper"),
      false,
    ),
    process(
      1002,
      Some(1000),
      Some(501),
      "Dia Helper (Renderer)",
      Some("/Applications/Dia.app/Contents/Frameworks/Dia Helper (Renderer).app/Contents/MacOS/Dia Helper (Renderer)"),
      false,
    ),
    process(5000, Some(1), Some(501), "sleep", Some("/bin/sleep"), false),
  ];
  let (mut groups, mut rows) = build_groups(&processes, &[], None, Some(501));

  // own pid is the Dia helper, so the whole Dia group is protected.
  let ctx = context(&processes, 1001, Some(501), &groups);
  apply(&mut groups, &mut rows, &processes, &ctx);

  let dia = group_by_bundle(&groups, "/Applications/Dia.app");
  assert_eq!(dia.protection, Some(Protection::SelfOrAncestor));
  for row in rows.iter().filter(|row| row.group_id == dia.group_id) {
    assert_eq!(
      row.protection,
      Some(Protection::SelfOrAncestor),
      "pid {} in the own group",
      row.pid
    );
  }

  let finder = group_by_bundle(&groups, "/System/Library/CoreServices/Finder.app");
  assert_eq!(finder.protection, Some(Protection::SystemComponent));

  let launchd = rows
    .iter()
    .find(|row| row.pid == 1)
    .and_then(|row| groups.iter().find(|group| group.group_id == row.group_id))
    .expect("launchd group");
  assert_eq!(launchd.protection, Some(Protection::Init));

  // A group whose main process is unprotected stays unprotected.
  let sleep = rows
    .iter()
    .find(|row| row.pid == 5000)
    .and_then(|row| groups.iter().find(|group| group.group_id == row.group_id))
    .expect("sleep group");
  assert_eq!(sleep.protection, None);
  assert_eq!(
    rows
      .iter()
      .find(|row| row.pid == 5000)
      .expect("sleep row")
      .protection,
    None
  );
}

#[test]
fn ancestor_walk_stops_at_64_hops_on_a_cycle() {
  // 10 → 11 → 12 → 10: the walk must terminate even though no parent is 1/0.
  let processes = vec![
    process(10, Some(11), Some(501), "a", Some("/a"), false),
    process(11, Some(12), Some(501), "b", Some("/b"), false),
    process(12, Some(10), Some(501), "c", Some("/c"), false),
  ];
  let ctx = context(&processes, 10, Some(501), &[]);

  assert!(ctx.ancestors.contains(&11));
  assert!(ctx.ancestors.contains(&12));
  assert_eq!(ctx.ancestors.len(), 3);
}

#[test]
fn ancestor_walk_is_capped_at_64_hops() {
  // A linear chain 1 → 2 → … → 200, own pid 200.
  let mut processes = Vec::new();
  for pid in 1..=200u32 {
    let ppid = if pid == 1 { None } else { Some(pid - 1) };
    processes.push(process(
      pid,
      ppid,
      Some(501),
      "proc",
      Some("/bin/proc"),
      false,
    ));
  }
  let ctx = context(&processes, 200, Some(501), &[]);

  for hop in 1..=64u32 {
    assert!(
      ctx.ancestors.contains(&(200 - hop)),
      "hop {hop} is an ancestor"
    );
  }
  // The 65th ancestor is beyond the cap.
  assert!(!ctx.ancestors.contains(&135));
}

#[test]
fn fakes_record_signals_and_advance_the_clock() {
  let clock = Arc::new(FakeClock::new(1_000));
  let sleeper = FakeSleeper::new(Arc::clone(&clock));
  let signaller = FakeSignaller::new(4242, Some(501));
  signaller.insert(500, Identity::new(111, Some(501), Some(1), false));

  assert_eq!(signaller.own_pid(), 4242);
  assert_eq!(signaller.current_uid(), Some(501));
  assert_eq!(
    signaller.identity(500),
    Some((111, Some(501), Some(1), false))
  );
  assert_eq!(signaller.identity(999), None);

  assert_eq!(signaller.send(500, Signal::Term), Ok(()));
  assert_eq!(signaller.recorded(), vec![(500, Signal::Term)]);

  assert_eq!(clock.now_ms(), 1_000);
  sleeper.sleep_ms(2_500);
  assert_eq!(clock.now_ms(), 3_500);
  assert_eq!(sleeper.slept_ms(), 2_500);
}
