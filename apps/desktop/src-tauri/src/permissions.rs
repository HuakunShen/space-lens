//! Full Disk Access (macOS TCC `kTCCServiceSystemPolicyAllFiles`).
//!
//! There is no API to request it: the system offers no prompt, and an app can
//! only send the user to the right Settings pane. So this module does two
//! things: tell whether the grant is in effect, and open that pane. The probe
//! only lists a directory that the grant protects, so it reads no file
//! contents and cannot trigger a consent prompt of its own.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FullDiskAccess {
  Granted,
  Denied,
  /// The probe could not tell (the protected folder is missing, say).
  Unknown,
  /// Not macOS: there is no such gate.
  Unsupported,
}

/// Folders only readable with Full Disk Access. The first one that exists
/// decides; a user who never opened Safari still has the TCC database.
#[cfg(target_os = "macos")]
const PROBES: [&str; 3] = [
  "Library/Safari",
  "Library/Mail",
  "Library/Application Support/com.apple.TCC",
];

#[cfg(target_os = "macos")]
pub fn status() -> FullDiskAccess {
  let home = crate::dirs_home();
  for probe in PROBES {
    match std::fs::read_dir(home.join(probe)) {
      Ok(_) => return FullDiskAccess::Granted,
      Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return FullDiskAccess::Denied,
      Err(_) => continue,
    }
  }
  FullDiskAccess::Unknown
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> FullDiskAccess {
  FullDiskAccess::Unsupported
}

const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

#[tauri::command]
pub fn fda_status() -> FullDiskAccess {
  status()
}

/// Opens System Settings at Privacy & Security → Full Disk Access.
#[tauri::command]
pub fn fda_open_settings() -> Result<(), String> {
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("/usr/bin/open")
      .arg(SETTINGS_URL)
      .spawn()
      .map(|_| ())
      .map_err(|error| format!("could not open System Settings: {error}"))
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = SETTINGS_URL;
    Err("Full Disk Access is a macOS setting".to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn status_serializes_kebab_case() {
    assert_eq!(serde_json::to_string(&FullDiskAccess::Granted).unwrap(), "\"granted\"");
    assert_eq!(serde_json::to_string(&FullDiskAccess::Unsupported).unwrap(), "\"unsupported\"");
  }

  #[test]
  fn probe_returns_a_verdict_without_panicking() {
    let _ = status();
  }
}
