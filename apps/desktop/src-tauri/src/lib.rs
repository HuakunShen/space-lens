pub mod commands;
pub mod engine;
pub mod events;

use std::path::PathBuf;
use tauri::Manager;

pub fn run() {
  let roots = default_roots();

  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .manage(commands::AppState::new(roots))
    .on_window_event(|window, event| {
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
      commands::sl_read,
      commands::sl_submit,
      commands::sl_host_request,
      commands::sl_events_subscribe,
      commands::sl_events_unsubscribe,
    ])
    .run(tauri::generate_context!())
    .expect("error while running Space Lens desktop");
}

fn dirs_home() -> PathBuf {
  std::env::var("HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from("/"))
}

/// Desktop roots: the user's home directory plus the platform's mount roots,
/// so external drives are scannable — a disk-usage tool that cannot look at
/// mounted volumes only half works. The picker and the scan form decide what
/// actually gets scanned; containment stays lexical under these roots.
fn default_roots() -> Vec<PathBuf> {
  let mut roots = vec![dirs_home()];
  #[cfg(target_os = "macos")]
  roots.push(PathBuf::from("/Volumes"));
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

  #[cfg(target_os = "macos")]
  #[test]
  fn macos_default_roots_serve_home_and_mounted_volumes() {
    let roots = default_roots();
    assert!(roots.iter().any(|root| root == &dirs_home()));
    assert!(roots.iter().any(|root| root == std::path::Path::new("/Volumes")));
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn linux_default_roots_serve_home_and_mount_roots() {
    let roots = default_roots();
    assert!(roots.iter().any(|root| root == &dirs_home()));
    assert!(roots.iter().any(|root| root == std::path::Path::new("/media")));
  }
}
