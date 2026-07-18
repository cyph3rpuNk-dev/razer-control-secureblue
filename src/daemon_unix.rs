//! Unix socket transport for the daemon: systemd socket activation with a
//! fallback to a private directory under `$XDG_RUNTIME_DIR`.  Never `/tmp`.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::backend::{Backend, BackendChoice, DryRunBackend};
use crate::daemon::Daemon;
use crate::{BLADE_14_2023, runtime_directory};

const SOCKET_FILE: &str = "daemon.sock";

pub fn run(allow_experimental: bool, backend: BackendChoice) -> Result<(), String> {
    match backend {
        BackendChoice::DryRun => run_with(
            Daemon::new(BLADE_14_2023, DryRunBackend::default(), allow_experimental),
            allow_experimental,
        ),
        #[cfg(feature = "hidraw-backend")]
        BackendChoice::Hidraw => {
            let backend = crate::backend_hidraw::HidrawBackend::open(BLADE_14_2023.id)?;
            run_with(
                Daemon::new(BLADE_14_2023, backend, allow_experimental),
                allow_experimental,
            )
        }
        #[cfg(not(feature = "hidraw-backend"))]
        BackendChoice::Hidraw => Err(
            "this build has no hidraw backend; rebuild with --features hidraw-backend".to_owned(),
        ),
    }
}

fn run_with<B: Backend>(mut daemon: Daemon<B>, allow_experimental: bool) -> Result<(), String> {
    let shutdown = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&shutdown))
            .map_err(|error| format!("cannot register signal handler: {error}"))?;
    }

    let listener = acquire_listener()?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot make listener non-blocking: {error}"))?;

    // Restore the persisted settings through the normal validation path,
    // then keep the file in sync after every accepted mutation.  A
    // brand-new install (no state file yet) starts from the shipped
    // factory defaults, which reapply then persists on the first save.
    let state = load_state_file().unwrap_or_else(crate::config::PersistedState::factory);
    daemon.load_state(state);
    daemon.reapply_persisted();
    daemon.take_dirty();

    eprintln!(
        "razer-control daemon: serving {} ({} backend, experimental={})",
        BLADE_14_2023.name,
        daemon.backend().name(),
        allow_experimental
    );

    let mut last_power_poll = std::time::Instant::now();
    let mut on_ac = crate::telemetry::on_ac();

    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = serve_connection(&mut daemon, stream, &shutdown) {
                    eprintln!("connection error: {error}");
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                daemon.shutdown();
                return Err(format!("accept failed: {error}"));
            }
        }

        if last_power_poll.elapsed() >= Duration::from_secs(5) {
            last_power_poll = std::time::Instant::now();
            let current = crate::telemetry::on_ac();
            if let Some(now_on_ac) = current
                && on_ac != current
                && let Some(action) = daemon.on_power_change(now_on_ac)
            {
                eprintln!("automation: {action}");
            }
            on_ac = current;
        }

        if daemon.take_dirty() {
            save_state_file(daemon.persisted());
        }
    }

    eprintln!("razer-control daemon: shutting down");
    daemon.shutdown();
    Ok(())
}

/// State file under `$XDG_CONFIG_HOME` (or `~/.config`): the daemon is
/// per-user, so its persisted settings are too.
fn state_file_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("razer-control").join("state"))
}

fn load_state_file() -> Option<crate::config::PersistedState> {
    let text = std::fs::read_to_string(state_file_path()?).ok()?;
    Some(crate::config::PersistedState::parse(&text))
}

fn save_state_file(state: &crate::config::PersistedState) {
    let Some(path) = state_file_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("cannot create {}: {error}", parent.display());
        return;
    }
    if let Err(error) = std::fs::write(&path, state.render()) {
        eprintln!("cannot save {}: {error}", path.display());
    }
}

/// Prefer a listener passed by systemd socket activation; otherwise bind a
/// 0600 socket inside a 0700 directory under `$XDG_RUNTIME_DIR`.
fn acquire_listener() -> Result<UnixListener, String> {
    let mut activation_fds = listenfd::ListenFd::from_env();
    if activation_fds.len() > 0 {
        if let Some(listener) = activation_fds
            .take_unix_listener(0)
            .map_err(|error| format!("systemd passed an unusable socket: {error}"))?
        {
            return Ok(listener);
        }
        return Err("systemd passed a socket that is not a unix stream listener".to_owned());
    }

    let directory = private_runtime_directory()?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot restrict {}: {error}", directory.display()))?;

    let socket_path = directory.join(SOCKET_FILE);
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)
            .map_err(|error| format!("cannot remove stale socket: {error}"))?;
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("cannot bind {}: {error}", socket_path.display()))?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("cannot restrict {}: {error}", socket_path.display()))?;
    Ok(listener)
}

fn private_runtime_directory() -> Result<PathBuf, String> {
    let xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();
    runtime_directory(xdg_runtime_dir.as_deref())
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set; refusing to fall back to /tmp".to_owned())
}

fn serve_connection<B: Backend>(
    daemon: &mut Daemon<B>,
    stream: UnixStream,
    shutdown: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut writer = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let line = match line {
            Ok(line) => line,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break;
            }
            Err(error) => return Err(error),
        };
        writer.write_all(daemon.handle_line(&line).as_bytes())?;
        writer.write_all(b"\n")?;
    }
    Ok(())
}

/// Send one request line to a running (or socket-activated) daemon and
/// return its response line.
pub fn send(command: &str) -> Result<String, String> {
    let socket_path = private_runtime_directory()?.join(SOCKET_FILE);
    let mut stream = UnixStream::connect(&socket_path).map_err(|error| {
        format!(
            "cannot connect to {}: {error}; is razer-control.socket enabled?",
            socket_path.display()
        )
    })?;
    stream
        .write_all(format!("{command}\n").as_bytes())
        .map_err(|error| format!("cannot send request: {error}"))?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| format!("cannot read response: {error}"))?;
    Ok(response.trim_end().to_owned())
}
