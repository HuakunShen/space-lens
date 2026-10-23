//! Hygiene guard for the memory crate (T10 of docs/memory-cleaner-plan.md).
//! It scans every `.rs` file under `src/` and holds three boundaries that only a
//! review would otherwise catch: `unsafe` may appear only inside `src/platform/`,
//! `serde_json::Value` only inside the stdio protocol module, and the core rule
//! modules never call `unwrap()`. The scan reads the real tree through
//! `CARGO_MANIFEST_DIR`, so it fails as soon as a boundary slips.

use std::fs;
use std::path::{Path, PathBuf};

/// The source files under `src/`, as `(path relative to src, contents)` pairs.
fn rust_sources() -> Vec<(PathBuf, String)> {
  let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
  let mut files = Vec::new();
  collect(&src, Path::new(""), &mut files);
  files
}

/// Walk `root`, recording every `.rs` file with its path relative to the src
/// root.
fn collect(root: &Path, relative: &Path, out: &mut Vec<(PathBuf, String)>) {
  for entry in fs::read_dir(root).expect("the src directory is readable") {
    let entry = entry.expect("a readable directory entry");
    let relative = relative.join(entry.file_name());
    let path = entry.path();
    if path.is_dir() {
      collect(&path, &relative, out);
    } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
      let text = fs::read_to_string(&path).expect("a readable source file");
      out.push((relative, text));
    }
  }
}

/// The contents of the source identified by its path relative to `src/`.
fn source<'a>(files: &'a [(PathBuf, String)], relative: &str) -> &'a str {
  files
    .iter()
    .find(|(path, _)| path == Path::new(relative))
    .unwrap_or_else(|| panic!("src/{relative} exists"))
    .1
    .as_str()
}

/// Whether the byte may be part of an identifier, so `unsafe_code` does not
/// count as the `unsafe` keyword.
fn is_word_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The byte offsets in `line` where the bare `keyword` appears.
fn keyword_offsets(line: &str, keyword: &str) -> Vec<usize> {
  let bytes = line.as_bytes();
  let mut offsets = Vec::new();
  let mut start = 0;
  while let Some(found) = line[start..].find(keyword) {
    let index = start + found;
    let before = index == 0 || !is_word_byte(bytes[index - 1]);
    let after_index = index + keyword.len();
    let after = after_index >= bytes.len() || !is_word_byte(bytes[after_index]);
    if before && after {
      offsets.push(index);
    }
    start = index + keyword.len();
  }
  offsets
}

#[test]
fn unsafe_lives_only_in_the_platform_module() {
  let files = rust_sources();
  assert!(!files.is_empty(), "the scan found the source tree");

  for (relative, text) in &files {
    let first = relative.components().next().map(|part| part.as_os_str());
    if first == Some(std::ffi::OsStr::new("platform")) {
      continue;
    }
    for (number, line) in text.lines().enumerate() {
      assert!(
        keyword_offsets(line, "unsafe").is_empty(),
        "{}:{} uses `unsafe` outside src/platform: {}",
        relative.display(),
        number + 1,
        line.trim()
      );
    }
  }
}

#[test]
fn serde_json_value_is_confined_to_the_protocol_module() {
  let files = rust_sources();
  // The guard is only meaningful while the protocol module really holds one.
  assert!(
    source(&files, "protocol.rs").contains("serde_json::Value"),
    "the protocol module keeps the only serde_json::Value"
  );

  for (relative, text) in &files {
    if relative == Path::new("protocol.rs") {
      continue;
    }
    assert!(
      !text.contains("serde_json::Value"),
      "{} touches serde_json::Value outside src/protocol.rs",
      relative.display()
    );
  }
}

#[test]
fn the_core_rule_modules_never_unwrap() {
  let files = rust_sources();
  for name in ["plan.rs", "safety.rs", "candidates.rs"] {
    assert!(
      !source(&files, name).contains(".unwrap()"),
      "src/{name} must not call unwrap()"
    );
  }
}
