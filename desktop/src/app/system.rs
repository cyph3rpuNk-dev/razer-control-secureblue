//! Desktop-integration helpers with no widgets in them: GPU-mode switching,
//! display modes via kscreen-doctor, panel backlight via logind, external
//! monitors via DDC/CI, the session idle timer, and the GUI-only settings
//! file.  Everything is simulated under `RAZER_CONTROL_MOCK=1` so the whole
//! UI can be exercised with zero system effect.

use super::client;

/// GPU-mode switching: delegate to whichever supported tool is installed.
/// supergfxctl talks to its own privileged daemon; prime-select and
/// envycontrol need root, which the app requests per switch through pkexec
/// (polkit) — this codebase never runs a root daemon of its own.
pub mod gpu {
    use std::process::Command;
    use std::sync::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Mode {
        Integrated,
        Hybrid,
        Dedicated,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Tool {
        SuperGfx,
        PrimeSelect,
        EnvyControl,
        Mock,
    }

    static MOCK_MODE: Mutex<Mode> = Mutex::new(Mode::Hybrid);

    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn parse_mode(text: &str) -> Option<Mode> {
        let text = text.to_lowercase();
        if text.contains("integrated") || text.contains("intel") {
            Some(Mode::Integrated)
        } else if text.contains("hybrid") || text.contains("on-demand") {
            Some(Mode::Hybrid)
        } else if text.contains("nvidia") || text.contains("dgpu") {
            Some(Mode::Dedicated)
        } else {
            None
        }
    }

    /// First installed tool and the mode it reports.
    pub fn detect() -> Option<(Tool, Mode)> {
        if super::client::is_mock() {
            return Some((Tool::Mock, *MOCK_MODE.lock().unwrap()));
        }
        if let Some(mode) = run("supergfxctl", &["-g"]).as_deref().and_then(parse_mode) {
            return Some((Tool::SuperGfx, mode));
        }
        if let Some(mode) = run("prime-select", &["query"])
            .as_deref()
            .and_then(parse_mode)
        {
            return Some((Tool::PrimeSelect, mode));
        }
        if let Some(mode) = run("envycontrol", &["--query"])
            .as_deref()
            .and_then(parse_mode)
        {
            return Some((Tool::EnvyControl, mode));
        }
        None
    }

    /// supergfxctl exposes a plain dGPU mode only on ASUS MUX hardware, so
    /// the dGPU option stays locked under it.
    pub fn supports_dedicated(tool: Tool) -> bool {
        !matches!(tool, Tool::SuperGfx)
    }

    /// Blocking; run off the main loop.  pkexec pops the polkit dialog for
    /// the tools that need root.
    pub fn switch(tool: Tool, mode: Mode) -> Result<String, String> {
        let run_checked = |program: &str, args: &[&str]| {
            let output = Command::new(program)
                .args(args)
                .output()
                .map_err(|error| format!("cannot run {program}: {error}"))?;
            if output.status.success() {
                Ok(format!(
                    "GPU mode set — log out or reboot to apply ({program})"
                ))
            } else {
                Err(format!(
                    "{program} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        };
        match tool {
            Tool::Mock => {
                *MOCK_MODE.lock().unwrap() = mode;
                Ok("GPU mode set (simulated) — log out or reboot to apply".to_owned())
            }
            Tool::SuperGfx => {
                let name = match mode {
                    Mode::Integrated => "Integrated",
                    Mode::Hybrid => "Hybrid",
                    Mode::Dedicated => {
                        return Err("dGPU mode is not supported by supergfxctl here".to_owned());
                    }
                };
                run_checked("supergfxctl", &["-m", name])
            }
            Tool::PrimeSelect => {
                let name = match mode {
                    Mode::Integrated => "intel",
                    Mode::Hybrid => "on-demand",
                    Mode::Dedicated => "nvidia",
                };
                run_checked("pkexec", &["prime-select", name])
            }
            Tool::EnvyControl => {
                let name = match mode {
                    Mode::Integrated => "integrated",
                    Mode::Hybrid => "hybrid",
                    Mode::Dedicated => "nvidia",
                };
                run_checked("pkexec", &["envycontrol", "-s", name])
            }
        }
    }
}

pub struct DisplayMode {
    pub id: String,
    pub label: String,
    pub current: bool,
}

/// All enabled outputs and their modes: kscreen-doctor normally, or a
/// simulated laptop panel plus one external monitor under the mock so every
/// row can be exercised where kscreen does not exist (WSLg) — always
/// labelled simulated, like the daemon mock.
pub fn display_outputs() -> Vec<(String, Vec<DisplayMode>)> {
    if client::is_mock() {
        let simulated = |resolution: &str, rates: &[&str], current: &str| -> Vec<DisplayMode> {
            rates
                .iter()
                .map(|hz| DisplayMode {
                    id: format!("{resolution}@{hz}"),
                    label: format!("{resolution}@{hz}"),
                    current: hz == &current,
                })
                .collect()
        };
        return vec![
            (
                "eDP-1 (simulated)".to_owned(),
                simulated("2560x1600", &["60", "120", "240"], "240"),
            ),
            (
                "HDMI-1 (simulated)".to_owned(),
                simulated("1920x1080", &["60", "144"], "144"),
            ),
        ];
    }
    kscreen_outputs()
}

/// `kscreen-doctor -o` for every enabled output: mode tokens look like
/// `id:WxH@Hz` with `*` marking the current mode and `!` the preferred one.
fn kscreen_outputs() -> Vec<(String, Vec<DisplayMode>)> {
    let Some(output) = std::process::Command::new("kscreen-doctor")
        .arg("-o")
        .output()
        .ok()
        .filter(|output| output.status.success())
    else {
        return Vec::new();
    };
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let mut outputs = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.first() != Some(&"Output:") || !line.contains("enabled") {
            continue;
        }
        let Some(name) = tokens.get(2) else {
            continue;
        };
        let mut modes = Vec::new();
        let mut in_modes = false;
        for token in &tokens {
            if *token == "Modes:" {
                in_modes = true;
                continue;
            }
            if in_modes {
                if !token.contains('@') || !token.contains(':') {
                    break;
                }
                let current = token.contains('*');
                let cleaned = token.replace(['*', '!'], "");
                let Some((id, label)) = cleaned.split_once(':') else {
                    continue;
                };
                modes.push(DisplayMode {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    current,
                });
            }
        }
        if !modes.is_empty() {
            outputs.push(((*name).to_owned(), modes));
        }
    }
    outputs
}

fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            if character.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if character == '\u{1b}' {
            in_escape = true;
        } else {
            result.push(character);
        }
    }
    result
}

/// `WxH@Hz` → (`WxH`, `Hz` rounded for display).
pub fn split_mode_label(label: &str) -> Option<(&str, String)> {
    let (resolution, hz) = label.split_once('@')?;
    let display = hz
        .parse::<f64>()
        .map_or_else(|_| hz.to_owned(), |value| format!("{value:.0}"));
    Some((resolution, display))
}

/// Sets one output's mode via kscreen-doctor; simulated outputs succeed
/// without touching anything, always saying so.  Blocking; run off-thread.
pub fn apply_display_mode(output_name: &str, mode_id: &str) -> Result<String, String> {
    if output_name.contains("(simulated)") {
        return Ok("Refresh rate applied (simulated)".to_owned());
    }
    let result = std::process::Command::new("kscreen-doctor")
        .arg(format!("output.{output_name}.mode.{mode_id}"))
        .status();
    match result {
        Ok(status) if status.success() => Ok("Refresh rate applied".to_owned()),
        Ok(status) => Err(format!("kscreen-doctor exited with {status}")),
        Err(error) => Err(format!("cannot run kscreen-doctor: {error}")),
    }
}

/// One `(hz, mode id)` candidate per distinct rate at the output's current
/// resolution, ascending, plus the index of the active one.
pub fn rate_candidates(modes: &[DisplayMode]) -> (Vec<(String, String)>, usize) {
    let current_index = modes.iter().position(|mode| mode.current).unwrap_or(0);
    let resolution = split_mode_label(&modes[current_index].label)
        .map(|(resolution, _)| resolution.to_owned())
        .unwrap_or_default();
    let mut candidates: Vec<(String, String, bool)> = Vec::new();
    for mode in modes {
        let Some((mode_resolution, hz)) = split_mode_label(&mode.label) else {
            continue;
        };
        if mode_resolution == resolution
            && !candidates.iter().any(|(existing, _, _)| *existing == hz)
        {
            candidates.push((hz, mode.id.clone(), mode.current));
        }
    }
    candidates.sort_by(|a, b| {
        a.0.parse::<f64>()
            .unwrap_or(0.0)
            .total_cmp(&b.0.parse::<f64>().unwrap_or(0.0))
    });
    let initial = candidates
        .iter()
        .position(|(_, _, current)| *current)
        .unwrap_or(0);
    (
        candidates.into_iter().map(|(hz, id, _)| (hz, id)).collect(),
        initial,
    )
}

/// First backlight device: (name, current, max).  Simulated under the mock
/// so the row renders where no backlight exists.
pub fn backlight_device() -> Option<(String, u32, u32)> {
    if client::is_mock() {
        return Some(("intel_backlight (simulated)".to_owned(), 80, 100));
    }
    let entry = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()?;
    let name = entry.file_name().to_string_lossy().into_owned();
    let read_u32 = |file: &str| -> Option<u32> {
        std::fs::read_to_string(entry.path().join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some((name, read_u32("brightness")?, read_u32("max_brightness")?))
}

/// Set the panel backlight through logind's SetBrightness D-Bus call — the
/// seat owner may write it without any privilege escalation.  Blocking (D-Bus
/// round-trip with a 1 s timeout); run off-thread.
pub fn set_backlight(device: &str, value: u32) -> Result<(), String> {
    use gtk::prelude::*;
    if device.contains("(simulated)") {
        return Ok(());
    }
    let connection = gtk::gio::bus_get_sync(gtk::gio::BusType::System, gtk::gio::Cancellable::NONE)
        .map_err(|error| format!("system bus unavailable: {error}"))?;
    connection
        .call_sync(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            Some(&("backlight", device, value).to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            1000,
            gtk::gio::Cancellable::NONE,
        )
        .map(|_| ())
        .map_err(|error| format!("SetBrightness failed: {error}"))
}

/// True when `ddcutil` sees at least one DDC/CI display (or always, under
/// the mock).  Blocking probe; call once at page build.
pub fn ddc_available() -> bool {
    if client::is_mock() {
        return true;
    }
    std::process::Command::new("ddcutil")
        .args(["detect", "--terse"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains("Display"))
}

/// One `ddcutil setvcp` invocation, or a simulated success.  Blocking.
pub fn ddc_set(feature: &str, value: &str) -> Result<String, String> {
    if client::is_mock() {
        return Ok(format!(
            "Monitor updated: setvcp {feature}={value} (simulated)"
        ));
    }
    let result = std::process::Command::new("ddcutil")
        .args(["setvcp", feature, value])
        .output();
    match result {
        Ok(output) if output.status.success() => Ok("Monitor updated".to_owned()),
        Ok(output) => Err(format!(
            "ddcutil failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("cannot run ddcutil: {error}")),
    }
}

/// Session idle time in seconds via org.freedesktop.ScreenSaver (KDE
/// implements GetSessionIdleTime); None where the bus or method is absent.
pub fn session_idle_seconds() -> Option<u64> {
    let connection =
        gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE).ok()?;
    let reply = connection
        .call_sync(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            "GetSessionIdleTime",
            None,
            None,
            gtk::gio::DBusCallFlags::NONE,
            1000,
            gtk::gio::Cancellable::NONE,
        )
        .ok()?;
    reply.child_value(0).get::<u32>().map(u64::from)
}

/// GUI-only settings file (the daemon's state file stays daemon-owned).
/// The name predates the Lighting page; it now holds every GUI-side rule.
fn gui_config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("razer-control").join("display.conf"))
}

pub fn gui_config_get(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(gui_config_path()?).ok()?;
    text.lines().find_map(|line| {
        line.trim()
            .split_once('=')
            .filter(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value.to_owned())
    })
}

/// Read-modify-write so each setting keeps the others intact.
pub fn gui_config_set(key: &str, value: &str) {
    let Some(path) = gui_config_path() else {
        return;
    };
    let mut entries: Vec<(String, String)> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            line.trim()
                .split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .filter(|(k, _)| k != key)
        .collect();
    entries.push((key.to_owned(), value.to_owned()));
    entries.sort();
    let text = entries
        .iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect::<String>();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, text) {
        eprintln!("cannot save GUI settings: {error}");
    }
}
