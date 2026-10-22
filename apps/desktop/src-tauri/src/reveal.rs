//! Reveal a scanned path in the desktop shell's own file manager.
//!
//! The path is data, never a command line: it is resolved against the roots
//! this shell serves and handed to the platform launcher as one argument (no
//! shell, no interpolation). Anything outside those roots is refused before a
//! process is spawned.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

/// The desktop shell's own operating system. The frontend uses it to name the
/// right file manager ("Open in Finder" / "Open in Explorer" / …).
pub fn host_platform() -> &'static str {
  if cfg!(target_os = "macos") {
    "macos"
  } else if cfg!(target_os = "windows") {
    "windows"
  } else if cfg!(target_os = "linux") {
    "linux"
  } else {
    "unknown"
  }
}

fn problem_value(code: &str, message: impl std::fmt::Display) -> Value {
  json!({ "problem": { "code": code, "message": message.to_string(), "retryable": false } })
}

/// What "show me this path" means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevealAction {
  /// Show the item in its parent folder, selected/highlighted.
  Reveal,
  /// Open the item itself in the file manager. Folders only.
  Open,
}

/// The platform's "show me this" launcher.
///
/// `Reveal` shows the item in its parent folder and selects it where the
/// platform can select; `Open` opens a folder in the file manager itself.
pub fn file_manager_target(
  path: &Path,
  is_directory: bool,
  action: RevealAction,
) -> Result<(String, Vec<String>), String> {
  if action == RevealAction::Open && !is_directory {
    return Err("only a folder can be opened in the file manager".to_string());
  }
  if cfg!(target_os = "macos") {
    if action == RevealAction::Open {
      Ok(("open".to_string(), vec![path.to_string_lossy().to_string()]))
    } else {
      Ok((
        "open".to_string(),
        vec!["-R".to_string(), path.to_string_lossy().to_string()],
      ))
    }
  } else if cfg!(target_os = "windows") {
    if action == RevealAction::Open {
      Ok((
        "explorer.exe".to_string(),
        vec![path.to_string_lossy().to_string()],
      ))
    } else {
      Ok((
        "explorer.exe".to_string(),
        vec![format!("/select,{}", path.to_string_lossy())],
      ))
    }
  } else if cfg!(target_os = "linux") {
    let target = if action == RevealAction::Open {
      path.to_path_buf()
    } else {
      // No portable way to select a row on Linux: open the parent folder.
      path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
    };
    Ok((
      "xdg-open".to_string(),
      vec![target.to_string_lossy().to_string()],
    ))
  } else {
    Err("this platform has no file manager launcher".to_string())
  }
}

/// Open `path` in the shell's file manager.
///
/// `roots` are the paths this shell is allowed to serve; a path outside them
/// is refused, never opened. The path itself is revealed as the scan reported
/// it — a symlinked entry is never resolved to its target.
pub fn reveal_scan_path(
  roots: &[PathBuf],
  path: &str,
  action: RevealAction,
) -> Result<Value, Value> {
  // Synthetic app identifiers name no real path and must never reach the
  // filesystem or the file manager.
  if crate::app_cleanup_policy::is_virtual_app_str(path) {
    return Err(problem_value(
      "Forbidden",
      "this path is outside the roots this host serves",
    ));
  }
  let candidate = Path::new(path);
  if !candidate.is_absolute() {
    return Err(problem_value("InvalidRequest", "the path must be absolute"));
  }
  // `Path::starts_with` compares whole components, so `/a/bc` is never inside
  // `/a/b`.
  if !roots.iter().any(|root| candidate.starts_with(root)) {
    return Err(problem_value(
      "Forbidden",
      "this path is outside the roots this host serves",
    ));
  }
  let metadata = std::fs::symlink_metadata(candidate)
    .map_err(|_| problem_value("NotFound", format!("no such path: {path}")))?;
  let is_directory = metadata.is_dir();
  if action == RevealAction::Open && !is_directory {
    return Err(problem_value(
      "InvalidRequest",
      "only a folder can be opened in the file manager",
    ));
  }
  let (command, args) = file_manager_target(candidate, is_directory, action)
    .map_err(|error| problem_value("Unavailable", error))?;
  open_in_file_manager(&command, &args, candidate)
    .map_err(|error| problem_value("Unavailable", error))?;
  Ok(json!({ "revealed": true }))
}

#[cfg(test)]
fn open_in_file_manager(_command: &str, _args: &[String], path: &Path) -> Result<(), String> {
  REVEALED_TEST_HOOK.lock().unwrap().push(path.to_path_buf());
  Ok(())
}

#[cfg(not(test))]
fn open_in_file_manager(command: &str, args: &[String], _path: &Path) -> Result<(), String> {
  // Spawn and ignore output; never a shell, never interpolated.
  std::process::Command::new(command)
    .args(args)
    .spawn()
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[cfg(test)]
static REVEALED_TEST_HOOK: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Test hook: paths the cfg(test) reveal runner was asked to open.
#[cfg(test)]
pub fn revealed_for_test() -> Vec<PathBuf> {
  REVEALED_TEST_HOOK.lock().unwrap().clone()
}

/// Test hook: clear recorded reveal invocations.
#[cfg(test)]
pub fn clear_revealed_for_test() {
  REVEALED_TEST_HOOK.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("spacelens-reveal-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp root");
    root
  }

  #[test]
  fn reveal_opens_a_served_folder() {
    let root = temp_root("served");
    let folder = root.join("folder");
    std::fs::create_dir_all(&folder).expect("folder");
    clear_revealed_for_test();
    let outcome = reveal_scan_path(
      std::slice::from_ref(&root),
      folder.to_str().expect("path"),
      RevealAction::Reveal,
    );
    assert!(outcome.is_ok(), "{outcome:?}");
    let revealed = revealed_for_test();
    assert_eq!(revealed.len(), 1);
    assert_eq!(revealed[0], folder);
    clear_revealed_for_test();
  }

  #[test]
  fn reveal_refuses_paths_outside_the_served_roots() {
    let root = temp_root("outside");
    let outside = temp_root("outside-target");
    let problem = reveal_scan_path(
      &[root],
      outside.to_str().expect("path"),
      RevealAction::Reveal,
    )
    .expect_err("refused");
    assert_eq!(problem["problem"]["code"], "Forbidden");
  }

  #[test]
  fn reveal_refuses_sibling_paths_with_a_shared_prefix() {
    let root = temp_root("prefix");
    let sibling = temp_root("prefix-sibling");
    let problem = reveal_scan_path(
      &[root],
      sibling.to_str().expect("path"),
      RevealAction::Reveal,
    )
    .expect_err("refused");
    assert_eq!(problem["problem"]["code"], "Forbidden");
  }

  #[test]
  fn reveal_refuses_missing_paths() {
    let root = temp_root("missing");
    let missing = root.join("nope");
    let problem = reveal_scan_path(
      &[root],
      missing.to_str().expect("path"),
      RevealAction::Reveal,
    )
    .expect_err("refused");
    assert_eq!(problem["problem"]["code"], "NotFound");
  }

  #[test]
  fn reveal_refuses_relative_and_synthetic_paths() {
    let root = temp_root("relative");
    let problem = reveal_scan_path(
      std::slice::from_ref(&root),
      "relative/path",
      RevealAction::Reveal,
    )
    .expect_err("refused");
    assert_eq!(problem["problem"]["code"], "InvalidRequest");
    let problem =
      reveal_scan_path(&[root], "app-storage:abc", RevealAction::Reveal).expect_err("refused");
    assert_eq!(problem["problem"]["code"], "Forbidden");
  }

  #[test]
  fn opening_a_file_in_the_file_manager_is_refused() {
    let root = temp_root("open-file");
    let file = root.join("note.txt");
    std::fs::write(&file, b"hello").expect("file");
    let problem = reveal_scan_path(&[root], file.to_str().expect("path"), RevealAction::Open)
      .expect_err("refused");
    assert_eq!(problem["problem"]["code"], "InvalidRequest");
  }

  #[test]
  fn the_platform_names_its_own_file_manager() {
    assert!(["macos", "windows", "linux", "unknown"].contains(&host_platform()));
  }
}
