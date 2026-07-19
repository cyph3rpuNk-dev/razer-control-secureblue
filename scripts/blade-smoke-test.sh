#!/usr/bin/env bash
# End-to-end smoke test for the dry-run daemon on the actual Blade under
# Fedora Secureblue/Atomic.  Sends no hardware commands; it exercises device
# detection, socket activation, IPC, policy enforcement, and the
# automatic-fan failsafe.  Everything is per-user; no root required.
set -u

PASS=0
FAIL=0
UNIT_DIR="${HOME}/.config/systemd/user"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${REPO_ROOT}/target/release/razer-control"
STARTED_AT="$(date '+%Y-%m-%d %H:%M:%S')"

ok()   { PASS=$((PASS + 1)); printf 'PASS  %s\n' "$1"; }
bad()  { FAIL=$((FAIL + 1)); printf 'FAIL  %s\n' "$1"; }
note() { printf 'note  %s\n' "$1"; }

echo "== razer-control-secureblue smoke test (dry-run backend) =="

# 1. Device identity: is this really a Blade 14 (2023)?
if command -v lsusb >/dev/null 2>&1; then
    if lsusb | grep -qi '1532:029d'; then
        ok "USB device 1532:029d (Blade 14 2023) is present"
    else
        bad "expected 1532:029d; Razer devices seen:"
        lsusb | grep -i '1532' || echo "  (none)"
        note "if the PID differs, the capability table must NOT be edited to match;"
        note "file the lsusb line as evidence for a new device entry instead"
    fi
else
    note "lsusb not found (usbutils); checking sysfs instead"
    if grep -qsi '029d' /sys/bus/usb/devices/*/idProduct; then
        ok "sysfs reports a device with product id 029d"
    else
        bad "no USB device with product id 029d found in sysfs"
    fi
fi

# 2. Build (host cargo or an existing release build).
if [ ! -x "$BIN" ]; then
    if command -v cargo >/dev/null 2>&1; then
        echo "building release binary..."
        (cd "$REPO_ROOT" && cargo build --release --locked) || {
            bad "cargo build failed"
            exit 1
        }
    else
        bad "no cargo and no prebuilt ${BIN}"
        note "build inside a matching-release container, e.g.:"
        note "  distrobox create --image registry.fedoraproject.org/fedora:\$(rpm -E %fedora) rust-build"
        note "  distrobox enter rust-build -- bash -c 'sudo dnf install -y cargo && cargo build --release --locked'"
        exit 1
    fi
fi
ok "release binary available"

# 3. Install user units pointing at the built binary.
mkdir -p "$UNIT_DIR"
sed "s|ExecStart=/usr/bin/razer-control|ExecStart=${BIN}|" \
    "${REPO_ROOT}/systemd/razer-control.service" > "${UNIT_DIR}/razer-control.service"
cp "${REPO_ROOT}/systemd/razer-control.socket" "${UNIT_DIR}/razer-control.socket"
systemctl --user daemon-reload
systemctl --user enable --now razer-control.socket && ok "socket unit active" || bad "socket unit failed"

# 4. Socket-activated IPC round trips.
run_ctl() { "$BIN" ctl "$@" 2>/dev/null; }

response="$(run_ctl status)"
case "$response" in
    ok*) ok "socket activation + status: ${response}" ;;
    *)   bad "status request failed: ${response:-no response}" ;;
esac

response="$(run_ctl fan manual 3000)"
case "$response" in
    ok*) ok "manual fan accepted by policy (dry-run): ${response}" ;;
    *)   bad "fan manual 3000 rejected unexpectedly: ${response}" ;;
esac

response="$(run_ctl fan manual 9000)"
case "$response" in
    err*) ok "out-of-range fan correctly rejected" ;;
    *)    bad "fan manual 9000 was NOT rejected: ${response}" ;;
esac

# A real experimental verb (the daemon here runs without --experimental), so
# rejection proves the opt-in gate — not merely that the verb is unknown.
response="$(run_ctl profile gaming)"
case "$response" in
    err*experimental*) ok "experimental profile correctly rejected without opt-in" ;;
    err*)              bad "profile gaming rejected, but not by the experimental gate: ${response}" ;;
    *)                 bad "experimental profile was NOT rejected: ${response}" ;;
esac

# 5. Failsafe: daemon is in manual mode from step 4; stopping the service
#    must log the revert-to-auto action.
systemctl --user stop razer-control.service
sleep 1
if journalctl --user -u razer-control.service --since "$STARTED_AT" 2>/dev/null | grep -qi 'failsafe'; then
    ok "failsafe fired on service stop (revert manual fan to automatic)"
else
    bad "no failsafe entry in the journal after stopping the service"
fi

# 6. Socket hygiene: never /tmp, private modes.
SOCK="${XDG_RUNTIME_DIR:-}/razer-control/daemon.sock"
if [ -S "$SOCK" ]; then
    ok "socket lives under XDG_RUNTIME_DIR: ${SOCK}"
    mode="$(stat -c '%a' "$SOCK")"
    [ "$mode" = "600" ] && ok "socket mode is 0600" || bad "socket mode is ${mode}, expected 600"
else
    note "socket file absent after stop (fine if systemd removed it); re-check while active"
fi
ls /tmp/razer* >/dev/null 2>&1 && bad "found razer artifacts in /tmp" || ok "nothing in /tmp"

echo
echo "== result: ${PASS} passed, ${FAIL} failed =="
echo "cleanup when done:  systemctl --user disable --now razer-control.socket"
[ "$FAIL" -eq 0 ]
