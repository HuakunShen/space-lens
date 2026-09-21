pub mod commands;
pub mod engine;
pub mod events;

use std::path::PathBuf;

pub fn run() {
    // Desktop roots: the user's home directory by default — the picker and the
    // scan form decide what actually gets scanned.
    let roots = vec![dirs_home()];

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::new(roots))
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
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}
