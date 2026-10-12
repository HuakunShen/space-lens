//! Downscaled PNG export of a macOS `.app` bundle icon.
//!
//! The icon pipeline serves the App Storage Audit view: after an audit, the
//! desktop shell may serve per-app icons over its `appIcon` read, gated by an
//! allow-list of audited paths. Like the rest of the crate this helper is
//! read-only: it loads, scales, and encodes, never writing anything.

use std::path::Path;

/// Load the icon for the `.app` bundle at `app_path`, scaled to fit within
/// 128x128 (aspect ratio preserved), encoded as PNG bytes.
///
/// Returns `None` when the icon is missing or cannot be read or encoded.
/// The resize already bounds the output, so even an enormous source still
/// returns a small PNG. Non-macOS builds always return `None`: enumeration
/// (and its `applications`/`plist` dependency trees) is macOS-only in v1.
#[cfg(target_os = "macos")]
pub fn app_icon_png(app_path: &Path) -> Option<Vec<u8>> {
  use applications::utils::image::RustImage;

  let icon = applications::load_icon(app_path).ok()?;
  // `thumbnail` scales down to fit within the bounds, preserving aspect.
  let scaled = icon.thumbnail(128, 128).ok()?;
  Some(scaled.to_png().ok()?.get_bytes().to_vec())
}

/// Non-macOS platforms have no icon loading yet; callers treat `None` as
/// "no icon available for this app".
#[cfg(not(target_os = "macos"))]
pub fn app_icon_png(_app_path: &Path) -> Option<Vec<u8>> {
  None
}

#[cfg(test)]
mod tests {
  use super::app_icon_png;

  fn unique_root(tag: &str) -> std::path::PathBuf {
    let root =
      std::env::temp_dir().join(format!("spacelens-app-icon-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
  }

  #[test]
  fn nonexistent_path_returns_none() {
    let missing = unique_root("missing").join("No Such App.app");
    assert!(
      app_icon_png(&missing).is_none(),
      "a nonexistent bundle path must yield no icon"
    );
    let _ = std::fs::remove_dir_all(unique_root("missing"));
  }

  #[test]
  fn app_bundle_without_icon_returns_none() {
    // A real `.app` bundle shape with a parseable `Info.plist` but no icon
    // file: there is nothing to load, so the helper fails soft to `None`
    // without depending on any app installed on the test machine.
    let root = unique_root("no-icon");
    let bundle = root.join("Iconless.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    std::fs::write(
      contents.join("Info.plist"),
      r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.example.Iconless</string>
  <key>CFBundleName</key>
  <string>Iconless</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    assert!(
      app_icon_png(&bundle).is_none(),
      "a bundle without an icon must yield no icon"
    );
    let _ = std::fs::remove_dir_all(&root);
  }
}
