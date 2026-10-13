use serde_json::Value;
use std::process::Command;

fn temp_home(tag: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("spacelens-cli-audit-{tag}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).unwrap();
  root.canonicalize().unwrap()
}

#[test]
fn app_audit_empty_home_yields_valid_report_with_audit_id() {
  let home = temp_home("empty");
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args([
      "app-audit",
      "--json",
      "--home",
      home.to_str().unwrap(),
    ])
    .output()
    .unwrap();
  let _ = std::fs::remove_dir_all(&home);
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  // Nothing else on stdout: exactly one JSON document.
  let stdout = String::from_utf8(output.stdout).unwrap();
  let value: Value = serde_json::from_str(stdout.trim()).unwrap();
  assert!(value.get("platform").and_then(|v| v.as_str()).is_some());
  assert!(value.get("generatedAt").and_then(|v| v.as_str()).is_some());
  let audit_id = value
    .get("auditId")
    .and_then(|v| v.as_str())
    .expect("report carries top-level auditId");
  assert!(audit_id.starts_with("audit_"));
  let suffix = &audit_id["audit_".len()..];
  assert!((8..=64).contains(&suffix.len()));
  assert!(suffix.chars().all(|c| c.is_ascii_alphanumeric()));
  // macOS enumerates real /Applications bundles regardless of HOME, so the
  // app list is not necessarily empty; it must be a valid array consistent
  // with totalApps/truncated.
  let apps = value.get("apps").and_then(|v| v.as_array()).expect("apps array");
  let total = value.get("totalApps").and_then(|v| v.as_u64()).expect("totalApps");
  assert_eq!(total as usize, {
    // totalApps counts before limit truncation; with the default limit 500
    // and a temp home it equals the returned length unless truncated.
    if value.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false) {
      total as usize
    } else {
      apps.len()
    }
  });
}

#[test]
fn app_audit_rejects_bad_limit_without_stdout_json() {
  let home = temp_home("bad-limit");
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args([
      "app-audit",
      "--json",
      "--limit",
      "0",
      "--home",
      home.to_str().unwrap(),
    ])
    .output()
    .unwrap();
  let _ = std::fs::remove_dir_all(&home);
  assert!(!output.status.success());
  assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
}

#[test]
fn app_icon_missing_path_fails_closed() {
  let missing = std::env::temp_dir().join(format!(
    "spacelens-cli-icon-missing-{}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&missing);
  let output = Command::new(env!("CARGO_BIN_EXE_spacelens"))
    .args(["app-icon", "--path", missing.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
}
