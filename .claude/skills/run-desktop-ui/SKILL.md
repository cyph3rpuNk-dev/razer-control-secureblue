---
name: run-desktop-ui
description: Launch and drive the razer-control-desktop GTK4 app (mock daemon, WSLg). Use when asked to run, screenshot, or visually verify the desktop UI, or to confirm a UI change works in the real app.
---

# Run the desktop UI

Verified on the maintainer's WSL2 (Fedora-based) dev shell with WSLg
(`WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`). The EC is unreachable here,
so always run against the in-process mock daemon.

## Launch

```bash
GDK_BACKEND=x11 RAZER_CONTROL_MOCK=1 cargo run -p razer-control-desktop
```

Run it in the background (first build takes a while). Two things about
that command line:

- `RAZER_CONTROL_MOCK=1` is the repo's documented mock-daemon mode
  (see CLAUDE.md) — the GUI gets live fake telemetry, no daemon or
  hardware needed.
- `GDK_BACKEND=x11` forces the window onto XWayland (`:0`) so it can be
  captured and driven with X11 tools. Without it GTK4 picks Wayland,
  where WSLg's Weston compositor supports neither `grim` nor
  wlr-screencopy — you can launch but not screenshot.

The app forces a dark scheme at startup (Synapse-style), so it looks the
same whatever the system theme; there is no light variant to check.

Expect a wall of `Vulkan`/`libEGL`/`ZINK` warnings at startup. They are
WSLg falling back to software rendering and are **benign** — do not
debug them. The app has launched when
`pgrep -f target/debug/razer-control-desktop` shows a PID.

## Capture and drive

One-time setup (already done if `import` and `xdotool` resolve):

```bash
sudo dnf install -y ImageMagick xdotool
```

Find the **main** window — `xdotool search --class razer` also matches
1x1 helper windows and menu popovers, so pick the one whose geometry is
the app's 920x700 default:

```bash
WID=$(for wid in $(xdotool search --class razer); do
  xdotool getwindowgeometry "$wid" 2>/dev/null | grep -q "920x700" && echo "$wid"
done | head -1)
import -display :0 -window "$WID" /path/to/shot.png
```

**Look at the screenshot** — a blank frame means the launch failed.
The Overview page should show the hero card (Blade 14 portrait, status
chips), a stat-tile grid, a Telemetry group, and a "Simulated session"
banner; values change between captures because the mock telemetry is live.

Navigation is five view-switcher pills in the header bar (Overview,
Performance, Display, Battery, Lighting). To click one, **re-read the
window geometry first** — WSLg moves and resizes windows between
activations, so cached coordinates go stale — and compute the pill's x
as a fraction of the window width (the switcher is centred):

```bash
eval $(xdotool getwindowgeometry --shell "$WID" | grep -E '^(X|Y|WIDTH)=')
xdotool windowactivate --sync "$WID"   # prints a XGetWindowProperty warning — ignore it
xdotool mousemove --sync $((X + WIDTH*31/100)) $((Y+20))   # "Performance" pill
xdotool click 1
```

Pill centres at window-relative y≈20: x/WIDTH ≈ 0.16 (Overview),
0.31 (Performance), 0.46 (Display), 0.60 (Battery), 0.75 (Lighting).
Below 620 px wide, the switcher moves to a bottom bar instead.

Diagnostics is a separate window behind the header menu. Clicking into
the menu popover is unreliable under WSLg (it is its own X window that
toggles closed easily); activate the action over D-Bus instead, then
capture the new 640x560 "Diagnostics" window:

```bash
gdbus call --session --dest dev.cyph3rpunk.razer-control \
  --object-path /dev/cyph3rpunk/razer_control \
  --method org.gtk.Actions.Activate diagnostics "[]" "{}"
```

Toasts appear bottom-centre; the Diagnostics window's request log is the
quickest way to confirm a control actually sent its IPC line.

## Cleanup

```bash
pkill -f target/debug/razer-control-desktop
```
