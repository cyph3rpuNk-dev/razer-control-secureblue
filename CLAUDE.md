# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A safety-first Razer Blade laptop controller for Atomic/Secureblue Linux, currently supporting only the **Razer Blade 14 (2023)** (`1532:029d`). The governing design constraint: **no HID write reaches hardware unless it is (a) for a recognised device, (b) validated against the compiled-in capability table, and (c) explicitly opted into.** The dry-run backend that only logs is the default; real hardware writes are double-gated behind a build feature *and* a runtime flag (see below).

License is **GPL-2.0-only** because `src/protocol.rs` is derived from GPL upstreams (razer-control-revived, fang-razer-linux). Keep new files GPL-2.0-only.

## Common commands

```bash
./scripts/check.sh                                                       # fmt + clippy + both test invocations (the full local gate)
cargo test --locked --workspace                                          # all crates (default = dry-run only)
cargo test --locked -p razer-control-secureblue --features hidraw-backend # core incl. the real backend
cargo test <name>                                                        # single test by name substring
cargo fmt --all --check                                                  # CI enforces formatting
cargo run -- device 1532 029d                                            # capability lookup
cargo run -- validate fan manual 3000                                    # run a value through policy, no write
cargo run -- validate profile gaming --experimental                      # experimental ops need the flag
RAZER_CONTROL_MOCK=1 cargo run -p razer-control-desktop                  # GUI against in-process mock daemon
```

CI (`.github/workflows/ci.yml`) runs the two test invocations above plus `cargo fmt --check` and `cargo clippy … -Dwarnings`, then in a separate Fedora job builds and installs the RPM and asserts the on-disk layout (no world-writable udev node, units in the right paths). `./scripts/check.sh` runs the first four locally in one pass; match it before assuming a change is green. `./scripts/clean.sh` does a safe `cargo clean` (`--deep` adds a confirmed, destructive `git clean -fdx`).

## Architecture

Three-layer split, all safety in the core, thin clients on top.

- **Policy core** ([src/lib.rs](src/lib.rs)) — pure functions and data. `DeviceCapabilities` table (only `BLADE_14_2023`), `RequestedOperation`, and `validate_operation()`, which is the single chokepoint every operation passes through. No I/O. This is where "is this device/value/feature allowed" is decided.
- **Wire protocol** ([src/protocol.rs](src/protocol.rs)) — pure EC packet construction, no I/O. Every packet the encoder can emit is pinned by model-specific **golden-byte tests**; if you change encoding, update the golden bytes deliberately, not to make a test pass. `REPORT_LEN`/offsets/CRC live here.
- **Daemon core** ([src/daemon.rs](src/daemon.rs)) — portable `Daemon<B: Backend>`: parses an IPC line, re-validates through `validate_operation`, hands accepted ops to a `Backend`. Holds live EC state (`EcContext`) because fan and profile packets each re-assert part of the other's state — see the `EcContext` doc comment in lib.rs. Reverts manual fan to auto on SIGTERM/SIGINT (the failsafe).
- **Backends** ([src/backend.rs](src/backend.rs)) — `Backend` trait. `DryRunBackend` (default, logs only) and `HidrawBackend` ([src/backend_hidraw.rs](src/backend_hidraw.rs), behind `hidraw-backend` feature). `select_hid_candidate()` guarantees a plugged-in Razer peripheral can never be chosen in place of the laptop EC.
- **Transport** ([src/daemon_unix.rs](src/daemon_unix.rs), unix-only) — systemd socket activation on `%t/razer-control/daemon.sock` (0600 in a 0700 dir), fallback under `$XDG_RUNTIME_DIR`, **never `/tmp`**. Also polls power source and drives AC/battery automation.
- **IPC** ([src/ipc.rs](src/ipc.rs)) — the line protocol (`ping`, `status`, `telemetry`, `fan manual <rpm>`, `automation …`, etc.). One request per line; response is one line starting `ok` or `err`. Shared by the daemon, the `ctl` client, `validate`, and all GUI/tray clients.
- **Clients** — [desktop/](desktop/) (GTK4/libadwaita, Linux-only real UI / stub elsewhere) and [tray/](tray/) (ksni StatusNotifierItem, KDE). Both are **pure IPC clients holding zero policy** — every action is one IPC line to the daemon.
- **Config** ([src/config.rs](src/config.rs)) — `PersistedState`, saved as IPC-shaped lines under the user's XDG config dir; restored through the normal validation path at daemon start.

## Hardware-write gating (do not weaken)

1. Compile: `HidrawBackend` only exists with `--features hidraw-backend`.
2. Runtime: even then, the daemon uses it only with the explicit `--backend hidraw` flag; default stays dry-run. Hardware access is opt-in per invocation, never ambient.
3. Policy: `boost`, `gpu-tdp`, profiles, and all lighting/logo ops are **experimental** and rejected unless `--experimental` is passed.
4. `#![forbid(unsafe_code)]` is set on every crate.

On-device verification of the Blade 14 write path is still pending; the `probe` subcommand (read-only EC dump, hidraw builds only) and [docs/PHASE3-PERF-VERIFICATION.md](docs/PHASE3-PERF-VERIFICATION.md) are the procedure for it.

## Conventions specific to this repo

- **Windows/Linux split**: core + daemon logic must build on the maintainer's Windows box. GTK4/ksni deps are `cfg`-gated to Linux; desktop/tray build as stubs elsewhere. Keep platform-specific code behind `cfg(unix)` / `cfg(target_os = "linux")` and the daemon transport unix-gated, so a `--workspace` build still succeeds cross-platform.
- **Capability changes require evidence**: any edit to the `DeviceCapabilities` table must land in the same commit as an evidence entry in [docs/DEVICES.md](docs/DEVICES.md). Values labelled "Declared" (from upstream/vendor UI, not confirmed on hardware) must not be presented as verified.
- The RPM/packaging spec is [packaging/razer-control-secureblue.spec](packaging/razer-control-secureblue.spec); CI derives the tarball version from it.
- Cross-session Claude memory for this project lives in a separate private repo: [cyph3rpuNk-dev/razer-control-memory](https://github.com/cyph3rpuNk-dev/razer-control-memory) (durable facts not derivable from the code — environment quirks, milestone gates, working preferences); its [README](https://github.com/cyph3rpuNk-dev/razer-control-memory/blob/master/README.md) documents the layout and conventions.
