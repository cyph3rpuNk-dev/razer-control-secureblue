# Razer Control Secureblue

An Atomic-Linux-oriented rebuild of Razer Blade control foundations.

Licensed **GPL-2.0-only**. The project plans to derive its HID protocol layer
from [razer-control-revived](https://github.com/cyph3rpuNk-dev/razer-control-revived)
(GPL-2.0), which in turn builds on the razer-laptop-control lineage; adopting
its license up front keeps that import clean. The code currently in this
repository is original to this project.

This initial milestone deliberately does **not** send HID commands to hardware. It establishes the safety boundary required before that work: a verified device capability table, profile validation, a secure runtime-socket location, and an udev rule which grants access only to the active local user.

## Blade 14 (2023)

The Razer Blade 14 (2023) is recognised as USB `1532:029d`. Its declared capabilities are:

- automatic or 2200–5000 RPM manual fan control;
- Battery Health Optimizer charge limit of 50–80%;
- CPU/GPU boost modes, disabled unless explicitly opted into as experimental.

## Security model

- No kernel module or DKMS component.
- No world-writable `hidraw` node: the supplied udev rule uses `TAG+="uaccess"` only.
- The user daemon socket belongs in `$XDG_RUNTIME_DIR/razer-control/`, never `/tmp`.
- Hardware writes will remain unavailable until protocol implementations have model-specific integration tests.

## Try the current foundation

```bash
cargo test
cargo run -- device 1532 029d
cargo run -- validate bho 80
cargo run -- validate fan manual 3000
cargo run -- udev-rule
```

`boost` and `gpu-tdp` are intentionally rejected unless `--experimental` is supplied. A valid policy decision is not yet a hardware write.

## Atomic/Secureblue packaging direction

This project will ship a signed RPM and an OCI/custom-image recipe. A host service is necessary because the laptop controller is a local HID device; a Flatpak alone cannot safely provide this feature.
