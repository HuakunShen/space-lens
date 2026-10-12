//! Platform enumeration of installed applications.
//!
//! macOS: enumerate `.app` bundles via the `applications` crate (mdfind plus
//! configurable search paths) and read each bundle's identity from its
//! `Contents/Info.plist`. Every failure mode is fail-soft: enumeration errors
//! yield an empty list and apps without a readable `Info.plist` are skipped,
//! so the audit still returns a report built from the catalog layer.
//!
//! Other platforms: enumeration returns an empty list; the catalog CLI
//! entries are still measured against the audit home.

use std::path::{Path, PathBuf};

use super::DiscoveredApp;

/// Read the display name and bundle id for one `.app` bundle.
///
/// Returns `None` when `Contents/Info.plist` is missing, in which case the
/// caller skips the app. A present-but-unparseable plist falls back to the
/// enumerated display name with no bundle id.
#[cfg(target_os = "macos")]
pub(crate) fn read_bundle_identity(
  app_bundle: &Path,
  fallback_name: &str,
) -> Option<(String, Option<String>)> {
  let info_plist = app_bundle.join("Contents/Info.plist");
  if std::fs::symlink_metadata(&info_plist).is_err() {
    return None;
  }
  let mut name = fallback_name.to_string();
  let mut bundle_id = None;
  if let Ok(value) = plist::Value::from_file(&info_plist) {
    if let Some(dictionary) = value.as_dictionary() {
      if let Some(identifier) = dictionary
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_string())
      {
        bundle_id = Some(identifier.to_owned());
      }
      if let Some(bundle_name) = dictionary
        .get("CFBundleName")
        .and_then(|value| value.as_string())
      {
        name = bundle_name.to_owned();
      }
    }
  }
  Some((name, bundle_id))
}

/// Enumerate installed `.app` bundles on macOS.
#[cfg(target_os = "macos")]
pub(crate) fn enumerate_installed_apps(home: &Path) -> Vec<DiscoveredApp> {
  use applications::common::SearchPath;
  use applications::{AppInfo, AppInfoContext};

  let extra_search_paths = vec![
    SearchPath::new(PathBuf::from("/Applications"), 2),
    SearchPath::new(home.join("Applications"), 2),
    SearchPath::new(PathBuf::from("/System/Applications"), 2),
    SearchPath::new(PathBuf::from("/System/Library/CoreServices"), 2),
    SearchPath::new(PathBuf::from("/System/Library/ExtensionKit/Extensions"), 1),
    SearchPath::new(PathBuf::from("/Library/PreferencePanes"), 1),
    SearchPath::new(home.join("Library/PreferencePanes"), 1),
  ];
  let mut context = AppInfoContext::new(extra_search_paths);
  // Fail-soft: when the crate call errors, catalog CLI entries are still audited.
  let apps = match context.refresh_apps() {
    Ok(()) => context.get_all_apps(),
    Err(_) => Vec::new(),
  };
  let mut discovered = Vec::with_capacity(apps.len());
  for app in apps {
    if let Some((name, bundle_id)) = read_bundle_identity(&app.app_desktop_path, &app.name) {
      discovered.push(DiscoveredApp {
        name,
        bundle_id,
        app_path: app.app_desktop_path,
      });
    }
  }
  discovered
}

/// Non-macOS platforms have no enumeration yet; the merge pipeline still runs.
#[cfg(not(target_os = "macos"))]
pub(crate) fn enumerate_installed_apps(_home: &Path) -> Vec<DiscoveredApp> {
  Vec::new()
}
