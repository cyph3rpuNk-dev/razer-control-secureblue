//! How the GUI reaches a daemon.  On Linux the default is the per-user
//! socket (systemd socket activation starts the daemon on first request).
//! The in-process transport drives the identical daemon core with the
//! dry-run backend, which is what makes the GUI fully testable on any
//! platform without hardware.

use razer_control_secureblue::BLADE_14_2023;
use razer_control_secureblue::backend::DryRunBackend;
use razer_control_secureblue::daemon::Daemon;

pub trait Transport {
    fn label(&self) -> &'static str;
    fn request(&mut self, line: &str) -> Result<String, String>;
}

pub struct InProcessDryRun {
    daemon: Daemon<DryRunBackend>,
}

impl InProcessDryRun {
    pub fn new() -> Self {
        Self {
            daemon: Daemon::new(BLADE_14_2023, DryRunBackend::default(), false),
        }
    }
}

impl Transport for InProcessDryRun {
    fn label(&self) -> &'static str {
        "in-process dry run"
    }

    fn request(&mut self, line: &str) -> Result<String, String> {
        Ok(self.daemon.handle_line(line))
    }
}

#[cfg(unix)]
pub struct DaemonSocket;

#[cfg(unix)]
impl Transport for DaemonSocket {
    fn label(&self) -> &'static str {
        "daemon socket"
    }

    fn request(&mut self, line: &str) -> Result<String, String> {
        razer_control_secureblue::daemon_unix::send(line)
    }
}

/// Socket on Linux unless `--mock` is passed; always in-process elsewhere.
pub fn choose(force_mock: bool) -> Box<dyn Transport> {
    #[cfg(unix)]
    if !force_mock {
        return Box::new(DaemonSocket);
    }
    let _ = force_mock;
    Box::new(InProcessDryRun::new())
}
