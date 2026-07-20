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

Expect a wall of `Vulkan`/`libEGL`/`ZINK` warnings at startup. They are
WSLg falling back to software rendering and are **benign** — do not
debug them. The app has launched when
`pgrep -f target/debug/razer-control-desktop` shows a PID.

## Capture and drive

One-time setup (already done if `import` and `xdotool` resolve):

```bash
sudo dnf install -y ImageMagick xdotool
```

Find the window and screenshot it:

```bash
WID=$(xdotool search --onlyvisible --class razer | head -1)
import -display :0 -window "$WID" /path/to/shot.png
```

**Look at the screenshot** — a blank frame means the launch failed.
The Dashboard should show the Blade 14 device card (`1532:029d`), two
temperature gauges, and a fan RPM tile; values change between captures
because the mock telemetry is live.

To click (e.g. sidebar navigation), convert window-relative coords to
absolute using `xdotool getwindowgeometry "$WID"` (Position + offset).
A bare `mousemove … click` can silently miss; the reliable sequence is:

```bash
xdotool windowactivate --sync "$WID"   # prints a XGetWindowProperty warning — ignore it
xdotool mousemove --sync $((WIN_X+88)) $((WIN_Y+121))   # e.g. "Performance" nav item
xdotool click 1
```

Sidebar items at the default 1280x800 size sit at window-relative
x≈88, y≈83 (Dashboard), 121 (Performance), 159 (Display & GPU),
197 (Battery), 235 (Lighting).

## Cleanup

```bash
pkill -f target/debug/razer-control-desktop
```
