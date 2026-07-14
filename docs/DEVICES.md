# Tested-device evidence

Every entry in the compiled-in capability table must be backed by evidence
recorded here. "Declared" values come from documentation or upstream code and
have not been confirmed against hardware by this project; they must not be
treated as verified.

## Razer Blade 14 (2023) — `1532:029d`

### Device identity

| Check | Status | Evidence |
| --- | --- | --- |
| USB enumeration as `1532:029d` | **Confirmed** (2026-07-13) | Windows 11 PnP enumeration on the maintainer's Blade 14: composite device `USB\VID_1532&PID_029D`, friendly name "Razer Blade 14". Interfaces: MI_00 keyboard + vendor control collections, MI_01 keyboard/system-controller/consumer collections, MI_02 mouse collection. |
| `lsusb` on Fedora Secureblue | Pending | Run `scripts/blade-smoke-test.sh` on the Secureblue boot and paste the `lsusb` line here. |

### Capability values

| Capability | Status | Notes |
| --- | --- | --- |
| Fan manual range 2000–5400 RPM | Declared (vendor UI) | Read off the Razer Synapse fan slider on the maintainer's own Blade 14 (2026-07-14): low 2000, high 5400, default marker at 3800. Supersedes the earlier 2200–5000 assumption. Still pending EC read-back verification before the write layer is enabled. |
| Battery Health Optimizer 50–80 % | Declared | Matches the range Razer Synapse offers for this model; on-device confirmation pending. |
| CPU/GPU boost modes | Declared, experimental | Gated behind `--experimental`; stays that way until fan and BHO have real-world mileage. |

### Dry-run daemon on Secureblue

| Check | Status |
| --- | --- |
| Socket activation via `razer-control.socket` | Pending |
| Policy rejections over IPC (out-of-range fan, gpu-tdp without opt-in) | Pending |
| Auto-fan failsafe logged on service stop | Pending |
| Socket is 0600 under `XDG_RUNTIME_DIR`, nothing in `/tmp` | Pending |

Update this file in the same commit as any capability-table change; a new
device entry without an evidence section here should not pass review.
