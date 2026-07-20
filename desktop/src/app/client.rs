//! IPC client layer: one request line, one response line, never on the GTK
//! main thread.
//!
//! Talks to the real per-user daemon socket unless `RAZER_CONTROL_MOCK=1` is
//! set, in which case it drives an in-process copy of the identical daemon
//! core with the dry-run backend.  Either way the GUI holds zero policy: the
//! daemon validates every request and the reply says what happened.
//!
//! Every user-initiated request (not the background status/telemetry polls)
//! is recorded in a small ring buffer for the Diagnostics page.

use gtk::glib;
use razer_control_secureblue::BLADE_14_2023;
use razer_control_secureblue::backend::DryRunBackend;
use razer_control_secureblue::config::PersistedState;
use razer_control_secureblue::daemon::Daemon;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

/// True when the GUI is driving the in-process mock daemon instead of the
/// real socket.
pub fn is_mock() -> bool {
    std::env::var_os("RAZER_CONTROL_MOCK").is_some()
}

/// Blocking request/response.  Call from a worker thread (see [`send`]) or,
/// sparingly, during startup where one local round-trip is acceptable.
pub fn request_blocking(line: &str) -> Result<String, String> {
    if is_mock() {
        mock(line)
    } else {
        razer_control_secureblue::daemon_unix::send(line)
    }
}

fn mock(line: &str) -> Result<String, String> {
    static MOCK: OnceLock<Mutex<Daemon<DryRunBackend>>> = OnceLock::new();
    let daemon = MOCK.get_or_init(|| {
        // Experimental is on in the mock: the backend is a dry run, so the
        // profile/lighting UI can be exercised with zero hardware risk.  The
        // real daemon still defaults to locked.  Load the shipped factory
        // defaults so the UI shows the same out-of-box state a fresh install
        // would (BHO on, fan automation on, keyboard 40%/20%).
        let mut daemon =
            Daemon::new(BLADE_14_2023, DryRunBackend::default(), true).with_simulated_telemetry();
        daemon.load_state(PersistedState::factory());
        Mutex::new(daemon)
    });
    Ok(daemon
        .lock()
        .map_err(|error| error.to_string())?
        .handle_line(line))
}

/// One remembered request for the Diagnostics page.
#[derive(Clone)]
pub struct LogEntry {
    pub time: String,
    pub request: String,
    pub response: String,
}

const LOG_CAPACITY: usize = 100;

fn log() -> &'static Mutex<VecDeque<LogEntry>> {
    static LOG: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn record(request: &str, result: &Result<String, String>) {
    let time = glib::DateTime::now_local()
        .ok()
        .and_then(|now| now.format("%H:%M:%S").ok())
        .map_or_else(String::new, |formatted| formatted.to_string());
    let response = match result {
        Ok(reply) => reply.clone(),
        Err(error) => format!("err {error}"),
    };
    let mut entries = log()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if entries.len() == LOG_CAPACITY {
        entries.pop_front();
    }
    entries.push_back(LogEntry {
        time,
        request: request.to_owned(),
        response,
    });
}

/// Oldest-first copy of the request log.
pub fn log_entries() -> Vec<LogEntry> {
    log()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// Run blocking work on the gio worker pool, then deliver the result on the
/// main loop: a toast with the outcome, then `done`.  This is the one path
/// every button/row handler uses, so the main thread never blocks on I/O,
/// pkexec, or a slow external tool.
pub fn spawn_result(
    overlay: &adw::ToastOverlay,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
    done: impl FnOnce(&Result<String, String>) + 'static,
) {
    let overlay = overlay.clone();
    glib::spawn_future_local(async move {
        let result = gtk::gio::spawn_blocking(work)
            .await
            .unwrap_or_else(|_| Err("worker thread panicked".to_owned()));
        let text = match &result {
            Ok(reply) => reply.clone(),
            Err(error) => format!("Error: {error}"),
        };
        overlay.add_toast(adw::Toast::new(&text));
        done(&result);
    });
}

/// Send one daemon request off-thread, toast the daemon's reply (which is the
/// policy verdict), record it for Diagnostics, and hand the result to `done`.
pub fn send(
    overlay: &adw::ToastOverlay,
    line: impl Into<String>,
    done: impl FnOnce(&Result<String, String>) + 'static,
) {
    let line = line.into();
    spawn_result(
        overlay,
        move || {
            let result = request_blocking(&line);
            record(&line, &result);
            result
        },
        done,
    );
}

/// Parse an `ok key=value key=value …` reply into its fields.
pub fn parse_fields(reply: &str) -> HashMap<String, String> {
    reply
        .trim_start_matches("ok ")
        .split_whitespace()
        .filter_map(|token| {
            token
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// The `sysinfo` reply, whose values (model names) contain spaces and so are
/// tab-separated rather than space-separated.
pub fn request_sysinfo_blocking() -> HashMap<String, String> {
    let Ok(line) = request_blocking("sysinfo") else {
        return HashMap::new();
    };
    line.trim_start_matches("ok ")
        .split('\t')
        .filter_map(|field| {
            field
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
        })
        .collect()
}
