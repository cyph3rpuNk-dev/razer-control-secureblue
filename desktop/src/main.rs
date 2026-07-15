//! Native GTK4/libadwaita desktop app for razer-control-secureblue.
//!
//! Like the tray and the retired Tauri shell, this is a thin client: every
//! control sends one line of the daemon IPC protocol and holds no policy.
//! The daemon (or the in-process dry-run core under RAZER_CONTROL_MOCK=1) makes
//! every safety decision.
//!
//! GTK4/libadwaita are Linux-only, so the whole UI is gated on `target_os =
//! "linux"`. On other platforms the binary is a stub that explains itself,
//! which keeps a workspace build green on the maintainer's Windows machine.

#[cfg(target_os = "linux")]
mod app;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    app::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "razer-control-desktop requires Linux (GTK4/libadwaita). \
         Develop the core on Windows; run the GUI on the Linux/secureblue side."
    );
    std::process::ExitCode::FAILURE
}
