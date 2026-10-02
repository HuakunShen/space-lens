//! Human-readable formatting for the session layer.
//!
//! The scan engine reports **allocated** sizes — `metadata.blocks() * 512`
//! (kuntu `scanner.rs::allocated_size`), i.e. on-disk usage including block
//! slack, not apparent byte counts — so every size shown in the UI means
//! disk usage.

use std::path::{Path, PathBuf};

pub fn display_parent(path: &Path, roots: &[PathBuf]) -> String {
  let parent = path.parent().unwrap_or(path);
  for root in roots {
    if let Ok(relative) = parent.strip_prefix(root) {
      if relative.as_os_str().is_empty() {
        return root
          .file_name()
          .unwrap_or(root.as_os_str())
          .to_string_lossy()
          .to_string();
      }
      return relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join(" / ");
    }
  }
  parent.to_string_lossy().to_string()
}

/// Web-UI twin of `packages/web-ui/src/lib/format.ts::formatBytes`: binary
/// divisors (1024) with decimal unit labels, macOS Finder style; `0 B` for
/// zero, and 0/1/2 fractional digits by value magnitude.
pub fn format_bytes(bytes: u64) -> String {
  const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
  if bytes == 0 {
    return "0 B".to_string();
  }
  let mut value = bytes as f64;
  let mut unit = 0usize;
  while value >= 1024.0 && unit < UNITS.len() - 1 {
    value /= 1024.0;
    unit += 1;
  }
  let digits = if value >= 100.0 || unit == 0 {
    0
  } else if value >= 10.0 {
    1
  } else {
    2
  };
  format!("{:.prec$} {}", value, UNITS[unit], prec = digits)
}

/// Thousands-separated integer — the Rust twin of the `toLocaleString()`
/// calls used for item counts across the web workbench.
pub fn format_count(count: usize) -> String {
  let digits = count.to_string();
  let bytes = digits.as_bytes();
  let mut out = String::with_capacity(digits.len() + digits.len() / 3);
  for (index, byte) in bytes.iter().enumerate() {
    if index > 0 && (bytes.len() - index) % 3 == 0 {
      out.push(',');
    }
    out.push(char::from(*byte));
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn candidate_parents_are_relative_to_the_scanned_folder() {
    assert_eq!(
      display_parent(
        Path::new("/dev/project/apps/gpui/target"),
        &[PathBuf::from("/dev/project")]
      ),
      "apps / gpui"
    );
    assert_eq!(
      display_parent(
        Path::new("/dev/project/node_modules"),
        &[PathBuf::from("/dev/project")]
      ),
      "project"
    );
  }

  #[test]
  fn parent_labels_do_not_strip_a_sibling_with_a_shared_prefix() {
    assert_eq!(
      display_parent(
        Path::new("/dev/project-other/target"),
        &[PathBuf::from("/dev/project")]
      ),
      "/dev/project-other"
    );
  }

  #[test]
  fn zero_bytes_formats_as_plain_zero() {
    assert_eq!(format_bytes(0), "0 B");
  }

  #[test]
  fn sub_kib_keeps_whole_bytes() {
    assert_eq!(format_bytes(1), "1 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1023), "1023 B");
  }

  #[test]
  fn kib_boundary_uses_two_digits() {
    assert_eq!(format_bytes(1024), "1.00 KB");
    assert_eq!(format_bytes(1025), "1.00 KB");
  }

  #[test]
  fn digit_count_follows_magnitude() {
    assert_eq!(format_bytes(1500), "1.46 KB");
    assert_eq!(format_bytes(1536), "1.50 KB");
    assert_eq!(format_bytes(10 * 1024), "10.0 KB");
    assert_eq!(format_bytes(100 * 1024), "100 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
  }

  #[test]
  fn petabyte_is_the_top_unit() {
    assert_eq!(format_bytes(1024u64.pow(6)), "1024 PB");
  }

  #[test]
  fn count_groups_thousands() {
    assert_eq!(format_count(0), "0");
    assert_eq!(format_count(9), "9");
    assert_eq!(format_count(999), "999");
    assert_eq!(format_count(1000), "1,000");
    assert_eq!(format_count(1_234_567), "1,234,567");
  }
}
