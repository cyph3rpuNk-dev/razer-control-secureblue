# Installing on Fedora Atomic / Secureblue

Current release scope: **dry-run only**. The daemon validates requests and
logs what it would send; no HID command reaches the EC yet. Installing now
gets you the full security plumbing (uaccess udev rule, per-user
socket-activated daemon, GUI) ahead of the protocol layer.

Supported hardware: Razer Blade 14 (2023), USB `1532:029d`
(see [DEVICES.md](DEVICES.md) for the evidence policy).

## Path A — release RPM, layered with rpm-ostree

1. Download the RPM and `SHA256SUMS` from the
   [latest release](https://github.com/cyph3rpuNk-dev/razer-control-secureblue/releases)
   and verify:

   ```bash
   sha256sum -c SHA256SUMS --ignore-missing
   ```

2. Layer it and reboot (the reboot also activates the udev rule):

   ```bash
   rpm-ostree install ./razer-control-secureblue-*.rpm
   systemctl reboot
   ```

3. Enable the per-user socket once per user:

   ```bash
   systemctl --user enable --now razer-control.socket
   razer-control ctl status
   ```

   The daemon starts on the first request (socket activation). The GUI
   appears in the app grid as "Razer Control".

## Path B — custom image (BlueBuild)

Image-based installs avoid layering entirely. In a BlueBuild recipe:

```yaml
modules:
  - type: rpm-ostree
    install:
      - https://github.com/cyph3rpuNk-dev/razer-control-secureblue/releases/download/v0.1.0/razer-control-secureblue-0.1.0-1.fc42.x86_64.rpm
```

Pin the exact release URL and record the SHA256 alongside your recipe.

## Path C — COPR (planned)

A COPR repository requires network-free builds, and shipping the Tauri app
made that harder rather than easier. Three things are fetched during the
build today: the crate sources, the npm packages the frontend is built from,
and — via `next/font/google` — Roboto Mono from Google's font CDN. Vendoring
the first two is routine (`cargo vendor`/rust2rpm, a pnpm store tarball); the
third wants the font self-hosted next to Selawik in `webui/public/fonts`.
Tracked for the packaging milestone; until then Path A is the source of truth.

## Verify the install

```bash
razer-control device 1532 029d          # capability table lookup
razer-control ctl status                # socket activation round trip
razer-control-desktop                   # the GUI, over the same socket
./scripts/blade-smoke-test.sh           # full on-device check (from a clone)
```

## Secureblue notes

- **USBGuard**: the Blade's EC is an internal USB device present at boot, so
  a policy generated on this machine already allows it. If you maintain a
  hand-tightened policy, allow `1532:029d`.
- **hardened_malloc**: the daemon and tray are plain Rust binaries and run
  under Secureblue's LD_PRELOAD hardening; if you hit an allocator abort,
  file it with the journal lines — that is a bug we want to know about.
- **WebKitGTK**: `razer-control-desktop` is a Tauri app, so it renders through
  the system WebKitGTK (`webkit2gtk4.1`, pulled in as a dependency) rather
  than being a self-contained binary. The webview holds no policy — every
  action it takes is one line of the daemon IPC protocol — but it is the one
  component with a browser engine in it, and on a hardened image it is the
  most likely thing to need a `journalctl --user` look if the window fails to
  come up. The daemon, the tray, and the CLI do not depend on it.
- The udev rule grants access via `TAG+="uaccess"` (active local session
  only). Nothing is world-writable and nothing runs as root.

## Uninstall / rollback

```bash
systemctl --user disable --now razer-control.socket
rpm-ostree uninstall razer-control-secureblue
systemctl reboot
```

`rpm-ostree rollback` returns to the previous deployment entirely if
anything misbehaves.
