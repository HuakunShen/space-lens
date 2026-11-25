mod ai;
pub mod app_cleanup;
pub mod app_cleanup_policy;
pub mod commands;
pub mod engine;
pub mod events;
pub mod memory;
mod native_material;
pub mod permissions;
pub mod remote;
pub mod reveal;

use std::ffi::OsString;
use std::path::PathBuf;
use tauri::Manager;

/// A full-disk scan allocates and frees millions of small blocks; the macOS
/// system allocator kept those pages resident (and counted by Activity
/// Monitor) long after the scan, piling up with every rescan.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub fn run() {
  let roots = default_roots();

  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .manage(commands::AppState::new(roots))
    .setup(|app| {
      native_material::create_main_window(app)?;
      // `app` is already `&mut App`, so `new` takes the reborrow directly;
      // clippy rejects the redundant `&app`.
      app.manage(memory::MemoryState::new(app));
      Ok(())
    })
    .on_window_event(|window, event| {
      native_material::on_window_event(window, event);
      if matches!(event, tauri::WindowEvent::Destroyed) {
        let app = window.app_handle().clone();
        let label = window.label().to_string();
        // Cancellation may briefly wait for an engine lock; do not
        // make destruction of a window block the main event loop.
        tauri::async_runtime::spawn_blocking(move || {
          commands::disconnect_window(&label, &app.state::<commands::AppState>());
        });
      }
    })
    .invoke_handler(tauri::generate_handler![
      commands::sl_connect,
      commands::sl_disconnect,
      commands::sl_remote_ssh_hosts,
      commands::sl_remote_connect,
      commands::sl_remote_disconnect,
      commands::sl_remote_devices,
      commands::sl_read,
      commands::sl_submit,
      commands::sl_host_request,
      commands::sl_events_subscribe,
      commands::sl_events_unsubscribe,
      ai::ai_get_config,
      ai::ai_set_config,
      ai::ai_chat,
      ai::ai_chat_stream,
      ai::ai_chat_cancel,
      permissions::fda_status,
      permissions::fda_open_settings,
      permissions::fda_app_path,
    ])
    .run(tauri::generate_context!())
    .expect("error while running Space Lens desktop");
}

pub(crate) fn dirs_home() -> PathBuf {
  home_from(|name| std::env::var_os(name))
}

/// Windows sets USERPROFILE, not HOME: only shells such as Git Bash add HOME,
/// so an app started from Explorer has none.
fn home_from(env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
  let read = |name| env(name).filter(|value| !value.is_empty());
  read("HOME")
    .or_else(|| cfg!(windows).then(|| read("USERPROFILE")).flatten())
    .map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Desktop roots: the user's home directory plus the platform's mount roots,
/// so external drives are scannable — a disk-usage tool that cannot look at
/// mounted volumes only half works. The picker and the scan form decide what
/// actually gets scanned; containment stays lexical under these roots.
fn default_roots() -> Vec<PathBuf> {
  let mut roots = vec![dirs_home()];
  #[cfg(target_os = "macos")]
  {
    // The startup disk. `/` is the firmlink union of the read-only system
    // volume and the writable data volume; `/System/Volumes/Data` is that
    // writable half, which the protected walker treats as its own root (it
    // excludes nested roots from the parent traversal, so the disk is
    // measured once). Serving both is what lets the drive list offer
    // "Macintosh HD" — the same pair the browser host uses when it is
    // started with `--root /`.
    roots.push(PathBuf::from("/"));
    roots.push(PathBuf::from("/System/Volumes/Data"));
    roots.push(PathBuf::from("/Volumes"));
  }
  #[cfg(target_os = "linux")]
  {
    roots.push(PathBuf::from("/mnt"));
    roots.push(PathBuf::from("/media"));
    if let Ok(user) = std::env::var("USER") {
      roots.push(PathBuf::from(format!("/run/media/{user}")));
    }
  }
  #[cfg(target_os = "windows")]
  {
    for letter in b'A'..=b'Z' {
      let drive = PathBuf::from(format!("{}:\\", letter as char));
      if drive.is_dir() {
        roots.push(drive);
      }
    }
  }
  roots
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn home_falls_back_to_the_windows_profile_when_home_is_unset() {
    use std::ffi::OsString;
    let env = |pairs: &'static [(&str, &str)]| {
      move |name: &str| {
        pairs
          .iter()
          .find(|(key, _)| *key == name)
          .map(|(_, value)| OsString::from(value))
      }
    };
    let both = env(&[("HOME", "/h"), ("USERPROFILE", "/p")]);
    assert_eq!(home_from(both), PathBuf::from("/h"));
    // A Windows app started from Explorer has USERPROFILE and no HOME; "/"
    // there is not a folder anyone chose.
    let profile = env(&[("HOME", ""), ("USERPROFILE", "/p")]);
    let expected = if cfg!(windows) { "/p" } else { "/" };
    assert_eq!(home_from(profile), PathBuf::from(expected));
    assert_eq!(home_from(env(&[])), PathBuf::from("/"));
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn macos_default_roots_serve_home_and_mounted_volumes() {
    let roots = default_roots();
    assert!(roots.iter().any(|root| root == &dirs_home()));
    assert!(roots
      .iter()
      .any(|root| root == std::path::Path::new("/Volumes")));
    // The startup disk is scannable too: `/` and its writable data volume.
    assert!(roots.iter().any(|root| root == std::path::Path::new("/")));
    assert!(roots
      .iter()
      .any(|root| root == std::path::Path::new("/System/Volumes/Data")));
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn linux_default_roots_serve_home_and_mount_roots() {
    let roots = default_roots();
    assert!(roots.iter().any(|root| root == &dirs_home()));
    assert!(roots
      .iter()
      .any(|root| root == std::path::Path::new("/media")));
  }
}
