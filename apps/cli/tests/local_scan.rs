use serde_json::Value;
use std::process::Command;

#[test]
fn protected_cli_streams_progress_and_one_final_complete_report() {
  let root = std::env::temp_dir().join(format!("spacelens-cli-local-{}", std::process::id()));
  std::fs::create_dir_all(&root).unwrap();
  std::fs::write(root.join("file"), vec![b'x'; 8192]).unwrap();
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args([
      "scan",
      root.to_str().unwrap(),
      "--local-only",
      "--json",
      "--progress-json",
      "--respect-gitignore",
      "false",
    ])
    .output()
    .unwrap();
  std::fs::remove_dir_all(root).unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let lines: Vec<Value> = String::from_utf8(output.stdout)
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();
  assert!(lines.len() >= 2);
  assert!(lines[..lines.len() - 1]
    .iter()
    .all(|line| line["type"] == "progress"));
  assert_eq!(lines.last().unwrap()["type"], "done");
  assert_eq!(
    lines.last().unwrap()["report"]["coverage"]["mode"],
    "local-only"
  );
  assert_eq!(
    lines.last().unwrap()["report"]["coverage"]["logicalBytes"],
    8192
  );
  assert_eq!(
    lines.last().unwrap()["report"]["nodes"][0]["children"][0]["logicalSize"],
    8192
  );
  assert_eq!(
    lines.last().unwrap()["report"]["nodes"][0]["children"][0]["scanState"],
    "complete"
  );
  assert!(lines[0]["progress"]["entriesScanned"].is_u64());
}

#[test]
fn local_cli_rejects_symlink_following_instead_of_falling_back_to_legacy_scan() {
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args([
      "scan",
      "/definitely-not-a-real-fixture",
      "--local-only",
      "--json",
      "--progress-json",
      "--follow-symlinks",
    ])
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert!(!String::from_utf8_lossy(&output.stdout).contains("\"type\":\"done\""));
  assert!(String::from_utf8_lossy(&output.stderr).contains("never follow"));
}

#[test]
fn streamed_nodes_are_preorder_complete_and_terminal_report_has_no_tree() {
  let root = std::env::temp_dir().join(format!("spacelens-cli-stream-{}", std::process::id()));
  std::fs::create_dir_all(root.join("nested")).unwrap();
  std::fs::write(root.join("nested/file"), vec![b'x'; 8192]).unwrap();
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args([
      "scan",
      root.to_str().unwrap(),
      "--local-only",
      "--json",
      "--progress-json",
      "--stream-nodes",
      "--respect-gitignore",
      "false",
    ])
    .output()
    .unwrap();
  std::fs::remove_dir_all(&root).unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let frames: Vec<Value> = String::from_utf8(output.stdout)
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();
  let nodes: Vec<&Value> = frames
    .iter()
    .filter(|frame| frame["type"] == "node")
    .map(|frame| &frame["node"])
    .collect();
  assert_eq!(nodes.len(), 3);
  assert!(nodes
    .iter()
    .all(|node| node["children"].as_array().unwrap().is_empty()));
  assert_eq!(
    nodes[0]["name"],
    root.file_name().unwrap().to_str().unwrap()
  );
  assert_eq!(nodes[1]["name"], "nested");
  assert_eq!(nodes[2]["name"], "file");
  assert_eq!(nodes[2]["logicalSize"], 8192);
  let done = frames.last().unwrap();
  assert_eq!(done["type"], "done");
  assert!(done["report"]["nodes"].as_array().unwrap().is_empty());
  assert_eq!(done["report"]["coverage"]["logicalBytes"], 8192);
  assert_eq!(
    frames
      .iter()
      .filter(|frame| frame["type"] == "done")
      .count(),
    1
  );
  assert!(!done["report"]["volumes"].as_array().unwrap().is_empty());
}

#[test]
fn streamed_nodes_require_the_protected_progress_protocol() {
  for args in [
    vec!["scan", "--stream-nodes"],
    vec!["scan", "--local-only", "--json", "--stream-nodes"],
  ] {
    let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
      .args(args)
      .output()
      .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
  }
}
