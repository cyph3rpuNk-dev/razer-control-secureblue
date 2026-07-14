//! Tauri shell: bridges the web UI to the daemon.  One command, one IPC
//! line — the frontend holds no policy, exactly like the other clients.
//!
//! On Linux requests go to the per-user daemon socket.  On other platforms
//! (and under RAZER_CONTROL_MOCK=1) they drive the identical daemon core
//! in-process with the dry-run backend, which is how the UI is developed
//! on Windows without hardware risk.

use std::sync::Mutex;

use razer_control_secureblue::BLADE_14_2023;
use razer_control_secureblue::backend::DryRunBackend;
use razer_control_secureblue::daemon::Daemon;

struct MockDaemon(Mutex<Daemon<DryRunBackend>>);

#[tauri::command]
fn daemon_request(line: String, mock: tauri::State<'_, MockDaemon>) -> Result<String, String> {
    #[cfg(unix)]
    {
        if std::env::var_os("RAZER_CONTROL_MOCK").is_none() {
            return razer_control_secureblue::daemon_unix::send(&line);
        }
    }
    Ok(mock
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .handle_line(&line))
}

#[tauri::command]
fn transport_label() -> &'static str {
    #[cfg(unix)]
    {
        if std::env::var_os("RAZER_CONTROL_MOCK").is_none() {
            return "daemon socket";
        }
    }
    "in-process dry run"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(MockDaemon(Mutex::new(Daemon::new(
            BLADE_14_2023,
            DryRunBackend::default(),
            false,
        ))))
        .invoke_handler(tauri::generate_handler![daemon_request, transport_label])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
