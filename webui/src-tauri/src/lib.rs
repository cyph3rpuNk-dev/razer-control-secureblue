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

/// Current power source, so the UI can follow AC/battery transitions the
/// way Synapse does. Read-only; profile auto-application on transitions is
/// daemon work (diagnostics milestone), not GUI work.
#[tauri::command]
fn power_source() -> &'static str {
    let Ok(manager) = starship_battery::Manager::new() else {
        return "unknown";
    };
    let Ok(batteries) = manager.batteries() else {
        return "unknown";
    };
    for battery in batteries.flatten() {
        use starship_battery::State;
        return match battery.state() {
            State::Discharging | State::Empty => "onBattery",
            State::Charging | State::Full => "pluggedIn",
            _ => "unknown",
        };
    }
    "unknown"
}

/// Launch Polychromatic if the user installed it (OpenRazer frontend).
/// Deliberately optional: OpenRazer needs DKMS kernel modules, which this
/// project neither ships nor recommends on Secureblue.
#[tauri::command]
fn open_polychromatic() -> Result<String, String> {
    #[cfg(unix)]
    {
        return match std::process::Command::new("polychromatic-controller").spawn() {
            Ok(_) => Ok("launched Polychromatic".to_owned()),
            Err(error) => Err(format!(
                "Polychromatic not found ({error}); see polychromatic.app"
            )),
        };
    }
    #[cfg(not(unix))]
    Err("Polychromatic runs on Linux only (OpenRazer)".to_owned())
}

/// Switch the internal panel's refresh rate via kscreen-doctor (KDE).
/// Display configuration is compositor territory, not daemon/EC work, so
/// this lives in the GUI shell and needs no privileges.
#[tauri::command]
fn set_refresh_rate(hz: u32) -> Result<String, String> {
    #[cfg(unix)]
    {
        let listing = std::process::Command::new("kscreen-doctor")
            .arg("-j")
            .output()
            .map_err(|error| format!("kscreen-doctor not available (KDE only): {error}"))?;
        let config: serde_json::Value = serde_json::from_slice(&listing.stdout)
            .map_err(|error| format!("unexpected kscreen-doctor output: {error}"))?;
        let outputs = config["outputs"]
            .as_array()
            .ok_or("unexpected kscreen-doctor output: no outputs array")?;
        for output in outputs {
            let name = output["name"].as_str().unwrap_or("");
            if !name.starts_with("eDP") {
                continue;
            }
            let current_mode = output["currentModeId"].as_str().unwrap_or("");
            let size = output["modes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|mode| mode["id"].as_str() == Some(current_mode))
                .map(|mode| (&mode["size"]["width"], &mode["size"]["height"]))
                .and_then(|(w, h)| Some((w.as_i64()?, h.as_i64()?)))
                .ok_or("could not determine the current panel resolution")?;
            let mode_argument = format!("output.{name}.mode.{}x{}@{hz}", size.0, size.1);
            let status = std::process::Command::new("kscreen-doctor")
                .arg(&mode_argument)
                .status()
                .map_err(|error| format!("cannot run kscreen-doctor: {error}"))?;
            return if status.success() {
                Ok(format!("applied {mode_argument}"))
            } else {
                Err(format!("kscreen-doctor rejected {mode_argument}"))
            };
        }
        return Err("no internal (eDP) display found".to_owned());
    }
    #[cfg(not(unix))]
    {
        let _ = hz;
        Err("refresh-rate switching targets KDE; use Windows display settings here".to_owned())
    }
}

/// Open a KDE System Settings module. Whitelisted so the webview cannot
/// spawn arbitrary programs.
#[tauri::command]
fn open_kde_settings(module: String) -> Result<String, String> {
    const ALLOWED: [&str; 2] = ["kcm_kscreen", "kcm_colors"];
    if !ALLOWED.contains(&module.as_str()) {
        return Err(format!("module {module:?} is not on the allowlist"));
    }
    #[cfg(unix)]
    {
        return match std::process::Command::new("systemsettings")
            .arg(&module)
            .spawn()
        {
            Ok(_) => Ok(format!("opened KDE System Settings ({module})")),
            Err(error) => Err(format!("cannot open systemsettings: {error}")),
        };
    }
    #[cfg(not(unix))]
    Err("KDE System Settings is Linux-only".to_owned())
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
        .invoke_handler(tauri::generate_handler![
            daemon_request,
            transport_label,
            power_source,
            open_polychromatic,
            set_refresh_rate,
            open_kde_settings
        ])
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
