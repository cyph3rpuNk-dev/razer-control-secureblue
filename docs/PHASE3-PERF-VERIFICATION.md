# Phase 3: verifying the performance-profile EC commands on hardware

The `profile` operation (EC command `0x0d/0x02` mode byte, `0x0d/0x07`
boost levels) is implemented and locked behind `--experimental`.  The
bytes are cross-checked against two independent GPL-2.0 sources that run
them on real Blades (razer-control-revived `device.rs`, fang-protocol
`packet.rs`), but **nothing unlocks for normal use until this procedure
has been completed on the maintainer's Blade 14 (2023) under Secureblue.**

Do the steps in order.  Stop at the first surprise and file what you saw
in `docs/DEVICES.md`.

## Safety notes before starting

- A full power cycle (shutdown, not reboot) resets the EC.  That is the
  recovery path if anything looks wrong.
- Plug into AC.  Do not run the first write tests on battery.
- Keep temperatures visible the whole time (the app dashboard, or
  `watch cat /sys/class/hwmon/*/temp1_input`).
- The known thermal envelope: Synapse's own profiles drive these same
  bytes on Windows, so the EC's internal limits stay in force.  We are
  verifying our encoding matches, not exploring new territory.

## Step 0 — build and baseline (no EC writes)

```sh
cargo build --release --features hidraw-backend
./target/release/razer-control device 1532 029d   # table sanity check
```

## Step 1 — reads only

`probe` sends only the `0x8x` query commands (get power mode, get boost,
get fan setpoint, get BHO).  It writes nothing.

```sh
./target/release/razer-control probe
```

Expected: `mode=0` (or whatever Synapse last set from Windows — record
it), `manual_fan=0`, plausible boost values `0..=3`, BHO state matching
what you configured.  **If `probe` errors with "unsupported" or returns
garbage values (mode > 4, boost > 3), stop: the read encoding is wrong
for this model and no write step below may run.**

Run `probe` twice more.  Identical output each time is the consistency
check.

## Step 2 — no-op write

Set the profile to the exact mode Step 1 read back (if it read mode 0,
that is `balanced`).  This exercises the write path without changing
state.

```sh
./target/release/razer-control daemon --experimental --backend hidraw &
./target/release/razer-control ctl profile balanced   # or what Step 1 read
./target/release/razer-control probe                  # after stopping the daemon
```

Expected: `ok`, and probe reads the same mode as before.  Fans should not
change audibly.

## Step 3 — Balanced ↔ Silent

Silent is custom mode (4) with both boosts low — the gentlest real
transition (it can only reduce power).

```sh
./target/release/razer-control ctl profile silent
./target/release/razer-control ctl status             # profile=silent
# listen + watch temps for 2–3 minutes under light load
./target/release/razer-control ctl profile balanced
```

Expected: mode reads back 4 then 0; fans/thermals calm under Silent; no
instability.  Watch for the lineage quirk: revived force-reverts fans to
auto when entering custom mode — note whether a manual fan setting
survives `profile silent`, and record it.

## Step 4 — Gaming

```sh
./target/release/razer-control ctl profile gaming
# run a short CPU load (e.g. stress-ng --cpu 8 --timeout 120)
# temps and fan ramp should be *higher* than the same load under balanced
./target/release/razer-control ctl profile balanced
```

## Step 5 — Custom boost levels

One level at a time, reading back after each:

```sh
./target/release/razer-control ctl profile custom cpu low gpu low
./target/release/razer-control ctl profile custom cpu medium gpu medium
./target/release/razer-control ctl profile custom cpu high gpu high
./target/release/razer-control ctl profile custom cpu boost gpu high
```

After each: `probe` (or `ctl status`) and confirm the boost read-back
matches what was set.  The last line is the only one that uses CPU boost
level 3.

## Step 6 — interaction with fan control

The mode byte and the fan flag share one EC command; this step proves
neither clobbers the other.

```sh
./target/release/razer-control ctl profile gaming
./target/release/razer-control ctl fan manual 3000
./target/release/razer-control ctl status    # fan=manual:3000 AND profile=gaming
./target/release/razer-control probe         # mode=1, manual_fan=1
./target/release/razer-control ctl fan auto
./target/release/razer-control probe         # mode=1, manual_fan=0  ← mode preserved
./target/release/razer-control ctl profile balanced
```

## Step 7 — persistence and failsafe

- Restart the daemon: the persisted profile must be re-applied (check
  `ctl status`).
- Kill the daemon (SIGTERM) while fans are manual: the failsafe must
  revert fans to auto *without* resetting the profile mode byte
  (`probe`: `manual_fan=0`, mode unchanged).

## Step 8 — keyboard lighting (cosmetic, still read-first)

The lighting commands (`0x03` class: brightness `0x03/0x03`, matrix
effects `0x03/0x0a`, logo LED `0x03/0x00`+`0x03/0x02`) are the one place
our two lineage sources agree but OpenRazer has an alternative:
OpenRazer's blade-misc brightness (`0x0e/0x04`) with the same argument
triple.  These writes are cosmetic and instantly reversible — far lower
risk than fan or perf writes — but do the read first anyway:

```sh
# read-back only (extend probe or use ctl once implemented):
# get_keyboard_brightness = 0x03/0x83 — response args[2] is 0-255.
./target/release/razer-control ctl kbd brightness 50    # no-op-ish mid value
./target/release/razer-control ctl kbd brightness 100
./target/release/razer-control ctl kbd brightness 0
./target/release/razer-control ctl kbd effect static 44d62c
./target/release/razer-control ctl kbd effect spectrum
./target/release/razer-control ctl kbd effect wave
./target/release/razer-control ctl kbd effect off
./target/release/razer-control ctl logo static
./target/release/razer-control ctl logo breathing
./target/release/razer-control ctl logo off
```

Expected: each command visibly changes the keyboard/logo immediately.
**If brightness commands return NotSupported**, the model wants
OpenRazer's `0x0e/0x04` variant instead — record that in DEVICES.md and
switch the encoding before anything else.  Also verify per-power-source
automation: set different `kbd-automation ac/battery` values, pull the
plug, and watch the backlight follow.

## Recording results

Every step's `probe` output goes into `docs/DEVICES.md` under a new
"perf-mode verification" section, dated, with the kernel and firmware
versions.  Only after all steps pass may the experimental gate be
reconsidered for profiles on this model.
