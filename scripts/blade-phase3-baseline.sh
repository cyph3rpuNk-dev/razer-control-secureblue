#!/usr/bin/env bash
# Phase 3, Steps 0-1a: the read-only hardware baseline for the Blade 14 (2023).
#
# Runs ONLY the query (0x8x) commands via `probe`.  It never writes to the EC
# and never starts the daemon.  It refuses to run anywhere the real device is
# not present (so it cannot be fooled into "passing" on WSL2 or a dev box),
# sanity-checks the read-back values, proves consistency across three reads,
# pins the response CRC window, and emits a ready-to-paste docs/DEVICES.md
# evidence block.  Stop at the first FAIL and file what you saw, per
# docs/PHASE3-PERF-VERIFICATION.md.
#
# Usage:
#   scripts/blade-phase3-baseline.sh              # run + print the evidence block
#   scripts/blade-phase3-baseline.sh --append     # also append the block to docs/DEVICES.md
set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${REPO_ROOT}/target/release/razer-control"
EXPECTED_VID_PID="1532:029d"
APPEND=0
[ "${1:-}" = "--append" ] && APPEND=1

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); printf 'PASS  %s\n' "$1"; }
bad()  { FAIL=$((FAIL + 1)); printf 'FAIL  %s\n' "$1"; }
note() { printf 'note  %s\n' "$1"; }
die()  { printf 'STOP  %s\n' "$1" >&2; exit 1; }

echo "== Phase 3 baseline (read-only) for Blade 14 (2023) =="

# 0. Refuse to run without the real hardware.  A missing hidraw node or the
#    absence of the exact VID:PID means the EC is not reachable here; the
#    whole point of this script is on-device evidence, so a dry environment
#    must hard-stop rather than emit misleading output.
if grep -qi microsoft /proc/version 2>/dev/null; then
    die "this looks like WSL2; the Razer EC is not exposed here. Boot the real Secureblue target."
fi
device_present=0
if command -v lsusb >/dev/null 2>&1; then
    lsusb | grep -qi "$EXPECTED_VID_PID" && device_present=1
elif grep -qsi '029d' /sys/bus/usb/devices/*/idProduct 2>/dev/null; then
    device_present=1
fi
[ "$device_present" -eq 1 ] || die "device ${EXPECTED_VID_PID} not found (lsusb/sysfs). Is this the Blade 14?"
ls /dev/hidraw* >/dev/null 2>&1 || die "no /dev/hidraw* nodes; the udev rule / uaccess may not have applied. Re-plug or re-login."
ok "hardware present: ${EXPECTED_VID_PID} with hidraw nodes"

# Step 0 — build with the hidraw backend, then the table sanity check.
if [ ! -x "$BIN" ] || ! grep -q hidraw "$BIN" 2>/dev/null; then
    command -v cargo >/dev/null 2>&1 || die "need cargo to build the hidraw binary (or prebuild it)."
    echo "building release binary with --features hidraw-backend..."
    (cd "$REPO_ROOT" && cargo build --release --locked --features hidraw-backend) \
        || die "cargo build --features hidraw-backend failed."
fi
ok "release binary with hidraw-backend available"

"$BIN" device 1532 029d | grep -q '^supported:' \
    && ok "capability table recognises the device" \
    || bad "device table did not report the Blade 14 as supported"

# Step 1 — probe three times.  Reads only.
echo "-- Step 1: probe x3 (query commands only, no writes) --"
probe1="$("$BIN" probe 2>&1)"; probe_rc=$?
if [ "$probe_rc" -ne 0 ]; then
    printf '%s\n' "$probe1"
    die "probe failed (rc=${probe_rc}). If it says 'unsupported' the read encoding is wrong for this model; no write step may run."
fi
printf '%s\n' "$probe1"
probe2="$("$BIN" probe 2>&1)"
probe3="$("$BIN" probe 2>&1)"

if [ "$probe1" = "$probe2" ] && [ "$probe2" = "$probe3" ]; then
    ok "three reads identical (consistency check)"
else
    bad "probe output differed across reads — file all three in DEVICES.md and stop:"
    printf '  run2:\n%s\n  run3:\n%s\n' "$probe2" "$probe3"
fi

# Sanity: mode must be 0..=4, boost 0..=3.  Garbage means the read encoding
# is wrong and, per the procedure, no write step below may run.
sane=1
while IFS= read -r line; do
    case "$line" in
        cpu:*|gpu:*)
            mode="$(printf '%s' "$line"  | sed -n 's/.*mode=\([0-9]*\).*/\1/p')"
            boost="$(printf '%s' "$line" | sed -n 's/.*boost=\([0-9]*\).*/\1/p')"
            { [ -n "$mode" ]  && [ "$mode" -ge 0 ]  && [ "$mode" -le 4 ]; }  || { sane=0; note "out-of-range mode in: $line"; }
            { [ -n "$boost" ] && [ "$boost" -ge 0 ] && [ "$boost" -le 3 ]; } || { sane=0; note "out-of-range boost in: $line"; }
            ;;
    esac
done <<EOF
$probe1
EOF
[ "$sane" -eq 1 ] && ok "read-back values within spec (mode 0-4, boost 0-3)" \
                  || bad "read-back values out of spec — read encoding suspect; DO NOT proceed to writes"

# Step 1a — the response CRC window.
crc="$(printf '%s\n' "$probe1" | sed -n 's/^crc_window=//p')"
case "$crc" in
    openrazer)      ok "CRC window = openrazer (expected; matches fang's on-hardware finding)" ;;
    lineage)        ok "CRC window = lineage (possible; this model answers with the lineage window)" ;;
    ""|mixed*|ambiguous)
        bad "CRC window = '${crc:-<none>}' — a first surprise; file it in DEVICES.md and stop before pinning" ;;
    *)              bad "unrecognised CRC window '${crc}'; file it and stop" ;;
esac

# Environment for the evidence record.
KERNEL="$(uname -r)"
BIOS="$(cat /sys/class/dmi/id/bios_version 2>/dev/null || echo unknown)"
BOARD="$(cat /sys/class/dmi/id/board_name 2>/dev/null || echo unknown)"
DATE="$(date '+%Y-%m-%d')"

# The paste-ready evidence block, in the DEVICES.md house style.
BLOCK="$(cat <<EOF

### Phase 3 perf-mode verification — baseline (Steps 0-1a)

Recorded ${DATE} on the maintainer's Blade 14 (2023) under Secureblue.
Kernel \`${KERNEL}\`; BIOS \`${BIOS}\`; board \`${BOARD}\`.
EC firmware is not exposed to the OS; BIOS/board stand in as the closest
identifiers.

Read-only \`probe\` output (identical across three consecutive reads):

\`\`\`
${probe1}
\`\`\`

CRC window observed: **${crc:-unknown}**. Follow-up: pin the validator to
this window and drop the dual acceptance (see PHASE3 Step 1a).

No EC writes were performed. Write steps (2+) remain pending.
EOF
)"

echo
echo "== result: ${PASS} passed, ${FAIL} failed =="
if [ "$FAIL" -ne 0 ]; then
    echo "Do not proceed to any write step. File the failures above in docs/DEVICES.md."
    exit 1
fi

echo "-- docs/DEVICES.md evidence block (paste under the Blade 14 section) --"
printf '%s\n' "$BLOCK"

if [ "$APPEND" -eq 1 ]; then
    printf '%s\n' "$BLOCK" >> "${REPO_ROOT}/docs/DEVICES.md"
    echo
    note "appended to docs/DEVICES.md — review, place it under the right heading, and commit."
fi
