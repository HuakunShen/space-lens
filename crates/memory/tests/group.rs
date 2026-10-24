//! Grouping cases from §9 of docs/memory-cleaner-plan.md, driven by the
//! `mac-processes.json` fixture: Dia helpers fold into one group, a nested
//! `WeChatAppEx Helper.app` folds into `WeChat.app`, `zsh` under Warp stays its
//! own `Cli` group, group ids are stable, and root daemons are `System`.

use serde::Deserialize;
use spacelens_memory::group::{build_groups, bundle_root, group_id_for, is_main_executable};
use spacelens_memory::{AppInfo, ProcessGroup, ProcessKind, ProcessRow, RawProcess};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
  current_uid: Option<u32>,
  frontmost: Option<u32>,
  apps: Vec<AppInfo>,
  processes: Vec<RawProcess>,
}

fn fixture() -> Fixture {
  let path = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mac-processes.json"
  );
  let raw = std::fs::read_to_string(path).expect("read mac-processes.json");
  serde_json::from_str(&raw).expect("parse mac-processes.json")
}

fn group_with_bundle<'a>(groups: &'a [ProcessGroup], path: &str) -> &'a ProcessGroup {
  groups
    .iter()
    .find(|group| group.bundle_path.as_deref() == Some(path))
    .unwrap_or_else(|| panic!("no group for bundle {path}"))
}

fn row(rows: &[ProcessRow], pid: u32) -> &ProcessRow {
  rows
    .iter()
    .find(|row| row.pid == pid)
    .unwrap_or_else(|| panic!("no row for pid {pid}"))
}

fn group_of<'a>(rows: &'a [ProcessRow], groups: &'a [ProcessGroup], pid: u32) -> &'a ProcessGroup {
  let group_id = row(rows, pid).group_id.clone();
  groups
    .iter()
    .find(|group| group.group_id == group_id)
    .expect("every row's group exists")
}

#[test]
fn dia_helpers_fold_into_one_group() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  let dia = group_with_bundle(&groups, "/Applications/Dia.app");
  assert_eq!(dia.pids, vec![1000, 1001, 1002, 1003]);
  assert_eq!(dia.main_pid, 1000);
  assert_eq!(dia.kind, ProcessKind::App);
  assert_eq!(dia.name, "Dia");
  assert_eq!(dia.bundle_id.as_deref(), Some("company.thebrowser.dia"));
  assert!(dia.frontmost, "Dia is the frontmost app in the fixture");
  assert_eq!(dia.footprint_bytes, 1_027_604_480);
  assert_eq!(dia.resident_bytes, 929_038_336);
  assert!(matches!(dia.cpu_percent, Some(cpu) if (cpu - 4.8).abs() < 1e-4));
  assert_eq!(dia.started_at, "2024-10-17T16:00:00.000Z");

  assert_eq!(row(&rows, 1000).kind, ProcessKind::App);
  assert_eq!(row(&rows, 1001).kind, ProcessKind::Helper);
  assert_eq!(row(&rows, 1002).kind, ProcessKind::Helper);
  assert_eq!(row(&rows, 1003).kind, ProcessKind::Helper);

  // The group id is the FNV-1a 64 hash of the bundle-root key (§4.1).
  assert_eq!(dia.group_id.as_str(), "grp_4a91a3e615f3d2fc");
  assert_eq!(row(&rows, 1001).group_id, dia.group_id);
}

#[test]
fn nested_we_chat_app_ex_helper_folds_into_we_chat() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  let wechat = group_with_bundle(&groups, "/Applications/WeChat.app");
  assert_eq!(wechat.pids, vec![2000, 2001]);
  assert_eq!(wechat.main_pid, 2000);
  assert_eq!(wechat.kind, ProcessKind::App);
  assert_eq!(wechat.name, "WeChat");
  assert_eq!(wechat.footprint_bytes, 503_316_480);

  assert_eq!(row(&rows, 2000).kind, ProcessKind::App);
  assert_eq!(row(&rows, 2001).kind, ProcessKind::Helper);
  assert_eq!(row(&rows, 2001).group_id.as_str(), "grp_84be8bb1c31f444a");
}

#[test]
fn zsh_under_warp_is_its_own_cli_group() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  let zsh = row(&rows, 4001);
  assert_eq!(zsh.kind, ProcessKind::Cli);
  let zsh_group = group_of(&rows, &groups, 4001);
  assert_eq!(zsh_group.pids, vec![4001]);
  assert_eq!(zsh_group.kind, ProcessKind::Cli);
  assert_eq!(zsh_group.name, "zsh");
  assert!(zsh_group.bundle_path.is_none());

  let warp = group_with_bundle(&groups, "/Applications/Warp.app");
  assert_eq!(warp.pids, vec![4000]);
  assert_eq!(warp.name, "Warp");
  assert_ne!(zsh_group.group_id, warp.group_id);

  // The node child of zsh is a separate Cli group too, never folded into Warp.
  let node = row(&rows, 4002);
  assert_eq!(node.kind, ProcessKind::Cli);
  assert_ne!(node.group_id, zsh_group.group_id);
  assert_ne!(node.group_id, warp.group_id);
}

#[test]
fn group_ids_are_stable_across_calls() {
  let f = fixture();
  let first = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);
  let second = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);
  assert_eq!(first.0, second.0, "groups are deterministic");
  assert_eq!(first.1, second.1, "rows are deterministic");
}

#[test]
fn root_daemons_are_system() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  for pid in [1, 200, 300] {
    let process = row(&rows, pid);
    assert_eq!(
      process.kind,
      ProcessKind::System,
      "pid {pid} is a system daemon"
    );
    assert_eq!(
      group_of(&rows, &groups, pid).kind,
      ProcessKind::System,
      "the group of pid {pid} is a system group"
    );
  }
  assert_eq!(group_of(&rows, &groups, 1).name, "launchd");
}

#[test]
fn system_path_process_is_system_even_for_the_current_user() {
  let f = fixture();
  let (_, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  // Finder runs as the user but its executable sits under an S4 prefix.
  assert_eq!(row(&rows, 400).kind, ProcessKind::System);
  assert_eq!(row(&rows, 140).kind, ProcessKind::System);
}

#[test]
fn bundle_main_without_app_info_is_an_app() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  assert_eq!(row(&rows, 900).kind, ProcessKind::App);
  let group = group_of(&rows, &groups, 900);
  assert_eq!(group.kind, ProcessKind::App);
  assert_eq!(group.main_pid, 900);
  assert_eq!(group.bundle_id, None);
  // The executable is named `spark_launcher`, so the name comes from the
  // bundle stem, not the process name.
  assert_eq!(group.name, "Spark");
}

#[test]
fn non_regular_agent_is_an_agent() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  assert_eq!(row(&rows, 3000).kind, ProcessKind::Agent);
  let group = group_of(&rows, &groups, 3000);
  assert_eq!(group.kind, ProcessKind::Agent);
  assert_eq!(group.name, "Raycast");
}

#[test]
fn other_user_process_is_other() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  assert_eq!(row(&rows, 500).kind, ProcessKind::Other);
  assert_eq!(group_of(&rows, &groups, 500).kind, ProcessKind::Other);
  assert_eq!(row(&rows, 501).kind, ProcessKind::Other);
  assert!(!row(&rows, 500).owned_by_current_user);
  assert!(row(&rows, 500).restricted);
}

#[test]
fn bundle_root_finds_the_outermost_app() {
  let nested = Path::new(
    "/Applications/Dia.app/Contents/Frameworks/Dia Helper (Renderer).app/Contents/MacOS/Dia Helper (Renderer)",
  );
  assert_eq!(
    bundle_root(nested),
    Some(PathBuf::from("/Applications/Dia.app"))
  );
  assert_eq!(
    bundle_root(Path::new("/Applications/Spark.app")),
    Some(PathBuf::from("/Applications/Spark.app"))
  );
  assert_eq!(bundle_root(Path::new("/bin/zsh")), None);
  assert_eq!(bundle_root(Path::new("/usr/local/bin/node")), None);
}

#[test]
fn is_main_executable_only_for_a_direct_contents_mac_os_child() {
  let bundle = Path::new("/Applications/Dia.app");
  assert!(is_main_executable(
    bundle,
    Path::new("/Applications/Dia.app/Contents/MacOS/Dia")
  ));
  assert!(!is_main_executable(
    bundle,
    Path::new("/Applications/Dia.app/Contents/Frameworks/Dia Helper.app/Contents/MacOS/Dia Helper")
  ));
  assert!(!is_main_executable(
    bundle,
    Path::new("/Applications/Dia.app/Contents/MacOS/sub/Dia")
  ));
  assert!(!is_main_executable(
    bundle,
    Path::new("/Applications/Dia.app/Contents/MacOS")
  ));
  assert!(!is_main_executable(
    bundle,
    Path::new("/Applications/Spark.app/Contents/MacOS/spark_launcher")
  ));
}

#[test]
fn group_id_for_hashes_the_key_with_fnv1a64() {
  assert_eq!(
    group_id_for("app:/Applications/Dia.app").as_str(),
    "grp_4a91a3e615f3d2fc"
  );
  assert_eq!(
    group_id_for("app:/Applications/WeChat.app").as_str(),
    "grp_84be8bb1c31f444a"
  );
  assert_eq!(
    group_id_for("proc:4001:1729181000000000").as_str(),
    "grp_9057befd7eb64841"
  );
  assert_ne!(
    group_id_for("app:/Applications/Dia.app"),
    group_id_for("app:/Applications/WeChat.app")
  );
}

#[test]
fn every_process_has_a_row_and_its_group_lists_it() {
  let f = fixture();
  let (groups, rows) = build_groups(&f.processes, &f.apps, f.frontmost, f.current_uid);

  assert_eq!(rows.len(), f.processes.len());
  assert_eq!(groups.len(), 23, "one group per bundle root or process key");

  for process in &rows {
    let group = groups
      .iter()
      .find(|group| group.group_id == process.group_id)
      .expect("every row's group exists");
    assert!(group.pids.contains(&process.pid));
  }

  for group in &groups {
    let mut sorted = group.pids.clone();
    sorted.sort_unstable();
    assert_eq!(group.pids, sorted, "group pids are sorted ascending");
    assert!(group.pids.contains(&group.main_pid));
  }
}

#[test]
fn another_users_command_line_is_never_exposed() {
  let process = RawProcess {
    pid: 7001,
    ppid: Some(1),
    start_time_us: 1_700_000_000_000_000,
    uid: Some(502),
    name: "secret-agent".to_string(),
    exe: Some("/usr/local/bin/secret-agent".into()),
    command: Some("/usr/local/bin/secret-agent --token hunter2".to_string()),
    user: Some("other".to_string()),
    state: spacelens_memory::ProcessState::Sleeping,
    threads: Some(4),
    listening_ports: vec![],
    cpu_percent: None,
    footprint_bytes: None,
    resident_bytes: None,
    virtual_bytes: None,
    zombie: false,
  };
  let (_, rows) = build_groups(&[process], &[], None, Some(501));
  assert_eq!(rows.len(), 1);
  assert_eq!(
    rows[0].command, None,
    "another account's argv stays private"
  );
  assert_eq!(rows[0].user.as_deref(), Some("other"));
}
