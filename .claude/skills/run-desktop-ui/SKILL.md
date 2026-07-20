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

To check dark mode, add `ADW_DEBUG_COLOR_SCHEME=prefer-dark`.

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
The Overview page should show the Blade 14 device card (`1532:029d`), a
Telemetry group (CPU/GPU/Fan/Memory rows), and a "Simulated session"
banner; values change between captures because the mock telemetry is live.

To click (e.g. sidebar navigation), **re-read the window position first**
— WSLg moves windows between activations, so cached coordinates go stale:

```bash
eval $(xdotool getwindowgeometry --shell "$WID" | grep -E '^(X|Y)=')
xdotool windowactivate --sync "$WID"   # prints a XGetWindowProperty warning — ignore it
xdotool mousemove --sync $((X+115)) $((Y+107))   # e.g. "Performance" nav item
xdotool click 1
```

Sidebar rows at the default 920x700 size sit at window-relative x≈115,
y≈69 (Overview), 107 (Performance), 145 (Cooling), 183 (Battery),
221 (Lighting), 259 (Automation), 297 (Display & GPU), 335 (Diagnostics).
Toasts appear bottom-centre; the Diagnostics page's request log is the
quickest way to confirm a control actually sent its IPC line.

## Cleanup

```bash
pkill -f target/debug/razer-control-desktop
```
