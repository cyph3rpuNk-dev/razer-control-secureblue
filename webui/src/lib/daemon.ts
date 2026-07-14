// Client for the daemon IPC line protocol.
//
// Inside Tauri, every request goes through the `daemon_request` command
// (socket on Linux, in-process dry-run core elsewhere). In a plain browser
// (`pnpm dev` without Tauri) a TypeScript mock mirrors the daemon's policy
// responses so the UI is previewable — the mock is presentation-only and
// never a source of truth: the real daemon re-validates everything.

// Mirrors the Blade 14 (2023) capability table (src/lib.rs).
export const DEVICE_NAME = "Razer Blade 14 (2023)";
export const DEVICE_ID = "1532:029d";
export const FAN_MIN_RPM = 2000;
export const FAN_MAX_RPM = 5400;
// Synapse marks its default/reset position on the slider track.
export const FAN_DEFAULT_RPM = 3800;
export const BHO_MIN = 50;
export const BHO_MAX = 80;

function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// --- browser-only mock -----------------------------------------------------

let mockFan = "auto";

function mockRequest(line: string): string {
  const tokens = line.trim().split(/\s+/);
  if (tokens[0] === "ping") return "ok pong";
  if (tokens[0] === "status")
    return `ok device=1532:029d backend=browser-mock fan=${mockFan} experimental=false`;
  if (tokens[0] === "fan" && tokens[1] === "auto") {
    mockFan = "auto";
    return "ok applied Fan(Auto) via browser-mock backend";
  }
  if (tokens[0] === "fan" && tokens[1] === "manual") {
    const rpm = Number(tokens[2]);
    if (!Number.isInteger(rpm)) return "err fan RPM must be an integer";
    if (rpm < FAN_MIN_RPM || rpm > FAN_MAX_RPM)
      return `err manual fan speed ${rpm} RPM is outside the verified ${FAN_MIN_RPM}–${FAN_MAX_RPM} RPM range`;
    mockFan = `manual:${rpm}`;
    return `ok applied Fan(Manual(${rpm})) via browser-mock backend`;
  }
  if (tokens[0] === "bho" && tokens[1] === "off")
    return "ok applied BatteryHealthOff via browser-mock backend";
  if (tokens[0] === "bho") {
    const limit = Number(tokens[1]);
    if (!Number.isInteger(limit) || limit < BHO_MIN || limit > BHO_MAX)
      return `err battery health limit ${tokens[1]}% is invalid; choose a value from 50% through 80%`;
    return `ok applied BatteryHealthLimit(${limit}) via browser-mock backend`;
  }
  if (tokens[0] === "boost" || tokens[0] === "gpu-tdp")
    return "err this control is experimental and requires explicit opt-in";
  return `err unknown request "${line}"`;
}

// --- public API --------------------------------------------------------------

export async function daemonRequest(line: string): Promise<string> {
  if (hasTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<string>("daemon_request", { line });
    } catch (error) {
      return `err ${String(error)}`;
    }
  }
  return mockRequest(line);
}

export type DetectedPowerSource = "pluggedIn" | "onBattery" | "unknown";

export async function detectPowerSource(): Promise<DetectedPowerSource> {
  if (hasTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<DetectedPowerSource>("power_source");
    } catch {
      return "unknown";
    }
  }
  // Plain browser preview: Battery Status API where available (Chromium).
  try {
    const nav = navigator as Navigator & {
      getBattery?: () => Promise<{ charging: boolean }>;
    };
    if (nav.getBattery) {
      const batteryInfo = await nav.getBattery();
      return batteryInfo.charging ? "pluggedIn" : "onBattery";
    }
  } catch {
    // fall through
  }
  return "unknown";
}

export async function transportLabel(): Promise<string> {
  if (hasTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<string>("transport_label");
    } catch {
      return "tauri (unavailable)";
    }
  }
  return "browser mock";
}
