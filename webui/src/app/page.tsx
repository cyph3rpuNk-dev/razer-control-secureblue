"use client";

// Synapse-style control surface. Pure IPC client: every action becomes one
// daemon protocol line via daemonRequest(); no policy lives here. Controls
// the daemon cannot drive yet render locked instead of pretending.

import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  Fan,
  Gauge,
  Link2,
  Lock,
  RefreshCw,
  Settings,
  Unlink,
  Waves,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Card, CardContent } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  BHO_MAX,
  BHO_MIN,
  DEVICE_ID,
  DEVICE_NAME,
  FAN_DEFAULT_RPM,
  FAN_MAX_RPM,
  FAN_MIN_RPM,
  daemonRequest,
  detectPowerSource,
  transportLabel,
} from "@/lib/daemon";

type FanChoice = "auto" | "manual";
type PowerSource = "pluggedIn" | "onBattery";
// Lighting cards can link both power sources to one shared profile.
type LightScope = PowerSource | "linked";
type FanProfile = { choice: FanChoice; rpm: number };

const DEFAULT_RPM = FAN_DEFAULT_RPM;

const fade = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.18, ease: "easeOut" as const },
};

export default function Home() {
  const [tab, setTab] = useState("performance");
  const [power, setPower] = useState<PowerSource>("pluggedIn");
  const [profiles, setProfiles] = useState<Record<PowerSource, FanProfile>>({
    pluggedIn: { choice: "auto", rpm: DEFAULT_RPM },
    onBattery: { choice: "auto", rpm: DEFAULT_RPM },
  });
  const [bho, setBho] = useState(80);
  // Synapse default: BHO on, and it stays on across AC transitions until
  // the user turns it off themselves.
  const [bhoEnabled, setBhoEnabled] = useState(true);

  // Lighting state is a design preview: fully interactive, but nothing is
  // sent — the daemon gains lighting operations with the protocol import.
  // Each card keeps separate AC/battery profiles with Synapse's defaults
  // (dimmer, quicker-to-switch-off on battery).
  const [sysLightPower, setSysLightPower] = useState<PowerSource>("pluggedIn");
  const [offLightPower, setOffLightPower] = useState<PowerSource>("pluggedIn");
  const [sysLinked, setSysLinked] = useState(false);
  const [offLinked, setOffLinked] = useState(false);
  const [sysLight, setSysLight] = useState<
    Record<LightScope, { on: boolean; brightness: number; logo: string }>
  >({
    pluggedIn: { on: true, brightness: 40, logo: "Static" },
    onBattery: { on: true, brightness: 20, logo: "Static" },
    linked: { on: true, brightness: 100, logo: "Static" },
  });
  const [switchOff, setSwitchOff] = useState<
    Record<
      LightScope,
      {
        displayOff: boolean;
        idle: boolean;
        idleMinutes: number;
        batteryLevel: boolean;
        batteryPercent: number;
      }
    >
  >({
    pluggedIn: {
      displayOff: false,
      idle: false,
      idleMinutes: 30,
      batteryLevel: false,
      batteryPercent: 20,
    },
    onBattery: {
      displayOff: false,
      idle: false,
      idleMinutes: 10,
      batteryLevel: false,
      batteryPercent: 20,
    },
    linked: {
      displayOff: false,
      idle: false,
      idleMinutes: 10,
      batteryLevel: false,
      batteryPercent: 20,
    },
  });
  const [effectsMode, setEffectsMode] = useState<"quick" | "advanced">(
    "quick",
  );
  const [quickEffect, setQuickEffect] = useState("Spectrum Cycling");

  const sysScope: LightScope = sysLinked ? "linked" : sysLightPower;
  const sys = sysLight[sysScope];
  const patchSys = (patch: Partial<(typeof sysLight)["pluggedIn"]>) =>
    setSysLight((current) => ({
      ...current,
      [sysScope]: { ...current[sysScope], ...patch },
    }));
  const offScope: LightScope = offLinked ? "linked" : offLightPower;
  const off = switchOff[offScope];
  const patchOff = (patch: Partial<(typeof switchOff)["pluggedIn"]>) =>
    setSwitchOff((current) => ({
      ...current,
      [offScope]: { ...current[offScope], ...patch },
    }));
  const [status, setStatus] = useState("");
  const [lastResponse, setLastResponse] = useState("");
  const [transport, setTransport] = useState("…");

  const profile = profiles[power];

  const refresh = useCallback(async () => {
    setStatus(await daemonRequest("status"));
  }, []);

  useEffect(() => {
    refresh();
    transportLabel().then(setTransport);
  }, [refresh]);

  // Follow AC/battery transitions like Synapse: when the detected source
  // changes, switch the visible profile tab. Manual tab clicks still work
  // between transitions.
  const lastDetected = useRef<string>("");
  useEffect(() => {
    let cancelled = false;
    const check = async () => {
      const detected = await detectPowerSource();
      if (cancelled || detected === "unknown") return;
      if (detected !== lastDetected.current) {
        lastDetected.current = detected;
        setPower(detected);
      }
    };
    check();
    const timer = setInterval(check, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const send = async (line: string) => {
    setLastResponse(await daemonRequest(line));
    await refresh();
  };

  const lightingPreview = () =>
    setLastResponse(
      "preview only — lighting commands arrive with the HID protocol import",
    );

  const applyFan = (next: FanProfile) =>
    send(next.choice === "auto" ? "fan auto" : `fan manual ${next.rpm}`);

  const updateProfile = (patch: Partial<FanProfile>, apply: boolean) => {
    const next = { ...profile, ...patch };
    setProfiles((current) => ({ ...current, [power]: next }));
    if (apply) applyFan(next);
  };

  return (
    <div className="flex min-h-screen flex-col bg-background text-foreground">
      {/* Header */}
      <header className="border-b border-border bg-black/40">
        <p className="pt-4 text-center text-sm font-medium tracking-[0.2em] text-foreground">
          RAZER BLADE 14
        </p>
        <Tabs value={tab} onValueChange={setTab} className="items-center">
          <TabsList className="mb-3 mt-2 gap-1 bg-transparent">
            {["performance", "battery", "lighting"].map((value) => (
              <TabsTrigger
                key={value}
                value={value}
                className="rounded-full px-5 text-xs uppercase tracking-wider text-muted-foreground transition-colors data-active:bg-primary data-active:text-primary-foreground dark:data-active:border-transparent dark:data-active:bg-primary dark:data-active:text-primary-foreground"
              >
                {value}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </header>

      {/* Body */}
      <main className="mx-auto w-full max-w-3xl flex-1 px-6 py-8">
        <AnimatePresence mode="wait">
          <motion.div key={tab} {...fade}>
            {tab === "performance" && (
              <Card>
                <CardContent className="space-y-6 pt-6">
                  {/* Title + hardware shortcut */}
                  <div className="flex items-center">
                    <SectionTitle>Performance Modes</SectionTitle>
                    <div className="ml-auto flex items-center gap-2">
                      <KeyChip>FN</KeyChip>
                      <span className="text-sm text-muted-foreground">+</span>
                      <KeyChip>P</KeyChip>
                    </div>
                  </div>

                  {/* Power source tabs */}
                  <div>
                    <div className="flex border-b border-border">
                      {(
                        [
                          ["pluggedIn", "Plugged In"],
                          ["onBattery", "On Battery"],
                        ] as const
                      ).map(([value, label]) => (
                        <button
                          key={value}
                          onClick={() => setPower(value)}
                          className={`-mb-px px-5 py-2.5 text-[15px] transition-colors ${
                            power === value
                              ? "border border-border border-b-card bg-card text-primary"
                              : "border border-transparent bg-secondary text-foreground/90 hover:text-foreground"
                          }`}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                  </div>

                  {/* Mode tiles — on battery Synapse offers Balanced only,
                      which matches the daemon: no manual fan unsupervised
                      on battery. */}
                  <div className="grid grid-cols-2 gap-4">
                    <ModeTile
                      icon={<Gauge className="size-7" />}
                      label="Balanced"
                      selected
                    />
                    {power === "pluggedIn" && (
                      <>
                        <ModeTile
                          icon={<Fan className="size-7" />}
                          label="Silent"
                          locked
                        />
                        <ModeTile
                          icon={<Settings className="size-7" />}
                          label="Custom"
                          locked
                        />
                      </>
                    )}
                  </div>

                  {/* Fan speed — plugged in only, like Synapse */}
                  <div
                    className={power === "pluggedIn" ? "space-y-4" : "hidden"}
                  >
                    <p className="text-[15px] text-foreground">Fan Speed</p>
                    <RadioRow
                      selected={profile.choice === "auto"}
                      title="Auto (Default)"
                      caption="The system automatically adjusts the fan speed"
                      onSelect={() => updateProfile({ choice: "auto" }, true)}
                    />
                    <RadioRow
                      selected={profile.choice === "manual"}
                      title="Manual"
                      caption="Manually maintain the fan speed at the selected rpm"
                      onSelect={() => updateProfile({ choice: "manual" }, true)}
                    />
                    <div
                      className={`flex items-end gap-4 pl-9 transition-opacity ${
                        profile.choice === "manual"
                          ? ""
                          : "pointer-events-none opacity-40"
                      }`}
                    >
                      <SliderEnd icon={<Fan className="size-5 text-orange-800" />}>
                        Low
                      </SliderEnd>
                      <BubbleSlider
                        min={FAN_MIN_RPM}
                        max={FAN_MAX_RPM}
                        step={100}
                        value={profile.rpm}
                        marker={FAN_DEFAULT_RPM}
                        gradient
                        onChange={(rpm) => updateProfile({ rpm }, false)}
                        onCommit={(rpm) => updateProfile({ rpm }, true)}
                      />
                      <SliderEnd
                        icon={
                          <span className="flex items-center text-sky-800">
                            <Fan className="size-5" />
                            <Waves className="size-4" />
                          </span>
                        }
                      >
                        High
                      </SliderEnd>
                    </div>
                  </div>

                  {power === "pluggedIn" && <Separator />}

                  {/* Voltage optimizer — locked, plugged in only */}
                  <div
                    className={power === "pluggedIn" ? "space-y-2" : "hidden"}
                  >
                    <div className="flex items-center gap-3">
                      <h3 className="text-[15px] uppercase tracking-[0.12em] text-foreground">
                        CPU Voltage Optimizer
                      </h3>
                      <Switch disabled checked={false} />
                      <Lock className="size-3.5 text-muted-foreground" />
                    </div>
                    <p className="text-sm text-muted-foreground">
                      Adjusting voltage may increase the efficiency of the CPU
                      by setting the optimal minimum voltage without causing
                      performance losses. Locked until the safe controls have
                      on-device mileage — the daemon rejects it without an
                      explicit opt-in.
                    </p>
                  </div>

                  <p className="text-xs text-muted-foreground">
                    The view follows the detected power source (unplug the
                    charger and the tab switches). The daemon applying stored
                    profiles on AC/battery transitions arrives with the
                    diagnostics milestone; Silent and Custom modes unlock
                    with the HID protocol import.
                  </p>
                </CardContent>
              </Card>
            )}

            {tab === "battery" && (
              <Card>
                <CardContent className="space-y-5 pt-6">
                  <div className="flex items-center gap-3">
                    <SectionTitle>Battery Health Optimizer</SectionTitle>
                    <Switch
                      checked={bhoEnabled}
                      onCheckedChange={(checked) => {
                        setBhoEnabled(checked);
                        send(checked ? `bho ${bho}` : "bho off");
                      }}
                    />
                  </div>
                  <p className="text-[15px] text-foreground">
                    Battery will stop charging when it has reached the limit
                    (%).
                  </p>
                  <div
                    className={`space-y-6 ${
                      bhoEnabled ? "" : "pointer-events-none opacity-40"
                    }`}
                  >
                    <div className="flex items-center gap-3">
                      <select
                        value={bho}
                        onChange={(event) => {
                          const value = Number(event.target.value);
                          setBho(value);
                          send(`bho ${value}`);
                        }}
                        className="h-10 w-20 rounded-none border border-border bg-secondary px-2 text-[15px] text-foreground focus:border-primary focus:outline-none"
                      >
                        {[50, 55, 60, 65, 70, 75, 80].map((value) => (
                          <option key={value} value={value}>
                            {value}
                          </option>
                        ))}
                      </select>
                      <span className="text-[15px] text-foreground">%</span>
                    </div>
                    <BhoSlider
                      value={bho}
                      onChange={setBho}
                      onCommit={(value) => send(`bho ${value}`)}
                    />
                  </div>
                </CardContent>
              </Card>
            )}

            {tab === "lighting" && (
              <div className="space-y-6">
                <Card>
                  <CardContent className="space-y-5 pt-6">
                    <SectionTitle>System Lighting</SectionTitle>
                    <LightingPowerTabs
                      value={sysLightPower}
                      linked={sysLinked}
                      onChange={setSysLightPower}
                      onToggleLinked={() => {
                        setSysLinked((current) => !current);
                        lightingPreview();
                      }}
                    />
                    <div className="flex items-center gap-3">
                      <h3 className="text-[15px] uppercase tracking-[0.12em] text-primary">
                        Brightness
                      </h3>
                      <Switch
                        checked={sys.on}
                        onCheckedChange={(checked) => {
                          patchSys({ on: checked });
                          lightingPreview();
                        }}
                      />
                    </div>
                    <div
                      className={sys.on ? "" : "pointer-events-none opacity-40"}
                    >
                      <BubbleSlider
                        min={0}
                        max={100}
                        step={1}
                        value={sys.brightness}
                        bubbleSuffix={sysLinked ? "*" : ""}
                        onChange={(value) => patchSys({ brightness: value })}
                        onCommit={lightingPreview}
                      />
                      <div className="flex justify-between text-[15px] text-foreground">
                        <span>OFF</span>
                        <span>BRIGHT</span>
                      </div>
                      {sysLinked && (
                        <p className="text-right text-[14px] text-red-500">
                          *This setting may reduce battery life
                        </p>
                      )}
                    </div>
                    <div className="space-y-2 pt-1">
                      <p className="text-[15px] uppercase tracking-[0.08em] text-foreground">
                        Logo
                      </p>
                      <select
                        value={sys.logo}
                        onChange={(event) => {
                          patchSys({ logo: event.target.value });
                          lightingPreview();
                        }}
                        className="h-10 w-48 rounded-none border border-border bg-secondary px-2 text-[15px] text-foreground focus:border-primary focus:outline-none"
                      >
                        {["Off", "Static", "Breathing"].map((mode) => (
                          <option key={mode} value={mode}>
                            {mode}
                          </option>
                        ))}
                      </select>
                    </div>
                  </CardContent>
                </Card>

                <Card>
                  <CardContent className="space-y-5 pt-6">
                    <SectionTitle>Switch Off Lighting</SectionTitle>
                    <LightingPowerTabs
                      value={offLightPower}
                      linked={offLinked}
                      onChange={setOffLightPower}
                      onToggleLinked={() => {
                        setOffLinked((current) => !current);
                        lightingPreview();
                      }}
                    />
                    <label className="flex items-center gap-3 text-[15px] text-foreground">
                      <Checkbox
                        checked={off.displayOff}
                        onCheckedChange={(checked) => {
                          patchOff({ displayOff: checked === true });
                          lightingPreview();
                        }}
                        className="size-5"
                      />
                      When display is turned Off
                    </label>
                    <label className="flex items-center gap-3 text-[15px] text-foreground">
                      <Checkbox
                        checked={off.idle}
                        onCheckedChange={(checked) => {
                          patchOff({ idle: checked === true });
                          lightingPreview();
                        }}
                        className="size-5"
                      />
                      When idle for (minutes)
                    </label>
                    <div
                      className={`px-8 ${
                        off.idle ? "" : "pointer-events-none opacity-40"
                      }`}
                    >
                      <BubbleSlider
                        min={1}
                        max={60}
                        step={1}
                        value={off.idleMinutes}
                        onChange={(value) => patchOff({ idleMinutes: value })}
                        onCommit={lightingPreview}
                      />
                      <div className="flex justify-between text-[15px] text-muted-foreground">
                        <span>1</span>
                        <span>60</span>
                      </div>
                    </div>
                    {(offLinked || offLightPower === "onBattery") && (
                      <>
                        <label className="flex items-center gap-3 text-[15px] text-foreground">
                          <Checkbox
                            checked={off.batteryLevel}
                            onCheckedChange={(checked) => {
                              patchOff({ batteryLevel: checked === true });
                              lightingPreview();
                            }}
                            className="size-5"
                          />
                          {offLinked
                            ? "When battery level falls below (%) - On Battery only:"
                            : "When battery level falls below (%):"}
                        </label>
                        <div
                          className={`px-8 ${
                            off.batteryLevel
                              ? ""
                              : "pointer-events-none opacity-40"
                          }`}
                        >
                          <BubbleSlider
                            min={10}
                            max={50}
                            step={1}
                            value={off.batteryPercent}
                            onChange={(value) =>
                              patchOff({ batteryPercent: value })
                            }
                            onCommit={lightingPreview}
                          />
                          <div className="flex justify-between text-[15px] text-muted-foreground">
                            <span>10</span>
                            <span>50</span>
                          </div>
                        </div>
                      </>
                    )}
                  </CardContent>
                </Card>

                <Card>
                  <CardContent className="space-y-5 pt-6">
                    <SectionTitle>Effects</SectionTitle>
                    <div className="inline-flex rounded-full border border-border p-1">
                      {(
                        [
                          ["quick", "Quick Effects"],
                          ["advanced", "Advanced Effects"],
                        ] as const
                      ).map(([value, label]) => (
                        <button
                          key={value}
                          onClick={() => {
                            setEffectsMode(value);
                            lightingPreview();
                          }}
                          className={`rounded-full px-5 py-1.5 text-[15px] transition-colors ${
                            effectsMode === value
                              ? "bg-primary text-primary-foreground"
                              : "text-foreground hover:text-primary"
                          }`}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                    {effectsMode === "quick" ? (
                      <>
                        <p className="text-[15px] text-foreground">
                          Quick effects are presets that can be saved to a
                          device&apos;s profile and synced with other supported
                          Razer Chroma-enabled devices.
                        </p>
                        <div className="flex items-center gap-4">
                          <select
                            value={quickEffect}
                            onChange={(event) => {
                              setQuickEffect(event.target.value);
                              lightingPreview();
                            }}
                            className="h-10 w-48 rounded-none border border-border bg-secondary px-2 text-[15px] text-foreground focus:border-primary focus:outline-none"
                          >
                            {[
                              "Spectrum Cycling",
                              "Static",
                              "Breathing",
                              "Wave",
                              "Reactive",
                            ].map((effect) => (
                              <option key={effect} value={effect}>
                                {effect}
                              </option>
                            ))}
                          </select>
                          <span className="size-6 shrink-0 rounded-full bg-[conic-gradient(#f00,#ff0,#0f0,#0ff,#00f,#f0f,#f00)]" />
                          <button
                            onClick={lightingPreview}
                            className="text-[15px] text-foreground underline underline-offset-4 hover:text-primary"
                          >
                            Apply to other Chroma-enabled devices
                          </button>
                        </div>
                      </>
                    ) : (
                      <p className="text-[15px] text-muted-foreground">
                        Advanced Chroma layering arrives with the HID protocol
                        import.
                      </p>
                    )}
                  </CardContent>
                </Card>

                <p className="text-xs text-muted-foreground">
                  Design preview: these controls do not send hardware commands
                  yet — keyboard lighting joins the daemon protocol with the
                  HID import milestone.
                </p>
              </div>
            )}
          </motion.div>
        </AnimatePresence>
      </main>

      {/* Footer */}
      <footer className="flex items-center gap-4 border-t border-border px-6 py-3 text-xs">
        <span className="text-muted-foreground">
          {DEVICE_NAME} ({DEVICE_ID}) via {transport}
        </span>
        <span className="ml-auto truncate font-mono text-foreground">
          {lastResponse || status}
        </span>
        <Button
          variant="outline"
          size="sm"
          className="h-7 gap-1.5 rounded text-xs text-muted-foreground"
          onClick={refresh}
        >
          <RefreshCw className="size-3" />
          Refresh
        </Button>
      </footer>
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-[15px] font-medium uppercase tracking-[0.12em] text-primary">
      {children}
    </h2>
  );
}

function KeyChip({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded border border-border px-4 py-1.5 text-sm text-foreground">
      {children}
    </span>
  );
}

function ModeTile({
  icon,
  label,
  selected = false,
  locked = false,
}: {
  icon: React.ReactNode;
  label: string;
  selected?: boolean;
  locked?: boolean;
}) {
  return (
    <motion.button
      whileTap={locked ? undefined : { scale: 0.99 }}
      disabled={locked}
      className={`relative flex h-36 flex-col items-center justify-center gap-3 rounded-md border bg-secondary/60 transition-colors ${
        selected
          ? "border-primary"
          : "border-border"
      } ${locked ? "cursor-not-allowed opacity-50" : ""}`}
    >
      {locked && (
        <Lock className="absolute right-3 top-3 size-3.5 text-muted-foreground" />
      )}
      <span
        className={`flex size-14 items-center justify-center rounded-full border-2 ${
          selected
            ? "border-primary text-primary"
            : "border-foreground/60 text-foreground/80"
        }`}
      >
        {icon}
      </span>
      <span className="text-lg text-foreground">{label}</span>
    </motion.button>
  );
}

function RadioRow({
  selected,
  title,
  caption,
  onSelect,
}: {
  selected: boolean;
  title: string;
  caption: string;
  onSelect: () => void;
}) {
  return (
    <button
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className="flex w-full items-start gap-4 text-left"
    >
      <span
        className={`mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border-2 ${
          selected ? "border-muted-foreground" : "border-muted-foreground"
        }`}
      >
        {selected && <span className="size-2.5 rounded-full bg-primary" />}
      </span>
      <span>
        <span className="block text-[15px] text-foreground">{title}</span>
        <span className="block text-sm text-muted-foreground">{caption}</span>
      </span>
    </button>
  );
}

function SliderEnd({
  icon,
  children,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <span className="flex flex-col items-center gap-1">
      {icon}
      <span className="text-[11px] uppercase tracking-wider text-muted-foreground">
        {children}
      </span>
    </span>
  );
}

function BubbleSlider({
  min,
  max,
  step,
  value,
  marker,
  gradient = false,
  bubbleSuffix = "",
  onChange,
  onCommit,
}: {
  min: number;
  max: number;
  step: number;
  value: number;
  marker?: number;
  gradient?: boolean;
  bubbleSuffix?: string;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  const markerPct =
    marker === undefined ? undefined : ((marker - min) / (max - min)) * 100;
  return (
    <div className="relative w-full pb-4 pt-9">
      <div
        className="pointer-events-none absolute top-0 flex flex-col items-center"
        style={{ left: `${pct}%`, transform: "translateX(-50%)" }}
      >
        <span className="rounded bg-primary px-2 py-0.5 font-mono text-xs font-semibold text-primary-foreground">
          {value}
          {bubbleSuffix}
        </span>
        <span className="h-0 w-0 border-x-4 border-t-4 border-x-transparent border-t-primary" />
      </div>
      <Slider
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={([next]) => onChange(next)}
        onValueCommit={([next]) => onCommit(next)}
        className={
          gradient
            ? // Synapse fan track: heat gradient (red = low airflow temp
              // headroom, blue = high), uniform across the whole track,
              // with the green thumb riding on top.
              "[&_[data-slot=slider-track]]:bg-[linear-gradient(to_right,#b23c17,#2b58c8)] [&_[data-slot=slider-range]]:bg-transparent [&_[data-slot=slider-thumb]]:size-4 [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-thumb]]:ring-primary/40"
            : undefined
        }
      />
      {markerPct !== undefined && (
        <span
          className="pointer-events-none absolute bottom-0 h-0 w-0 border-x-4 border-b-4 border-x-transparent border-b-primary"
          style={{ left: `${markerPct}%`, transform: "translateX(-50%)" }}
        />
      )}
    </div>
  );
}

// Synapse's BHO slider: the track spans 0-100 so the user sees where the
// limit sits on the whole charge scale, the green fill runs from 50 to the
// thumb, but only 50-80 in steps of 5 is selectable — the daemon rejects
// anything else regardless.
function BhoSlider({
  value,
  onChange,
  onCommit,
}: {
  value: number;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const valueFromPointer = (clientX: number) => {
    const track = trackRef.current;
    if (!track) return value;
    const rect = track.getBoundingClientRect();
    const pct = Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1);
    const snapped = Math.round((pct * 100) / 5) * 5;
    return Math.min(BHO_MAX, Math.max(BHO_MIN, snapped));
  };

  return (
    <div className="select-none">
      <div
        ref={trackRef}
        className="relative h-6 cursor-pointer touch-none"
        onPointerDown={(event) => {
          dragging.current = true;
          event.currentTarget.setPointerCapture(event.pointerId);
          onChange(valueFromPointer(event.clientX));
        }}
        onPointerMove={(event) => {
          if (dragging.current) onChange(valueFromPointer(event.clientX));
        }}
        onPointerUp={(event) => {
          dragging.current = false;
          onCommit(valueFromPointer(event.clientX));
        }}
      >
        <div className="absolute top-1/2 h-1.5 w-full -translate-y-1/2 rounded-full bg-muted" />
        <div
          className="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-primary"
          style={{ left: "50%", width: `${value - 50}%` }}
        />
        <div
          className="absolute top-1/2 size-4 -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary"
          style={{ left: `${value}%` }}
        />
      </div>
      <div className="relative h-6 text-[15px] text-foreground">
        <span className="absolute left-0">0</span>
        {value > 58 && (
          <span
            className="absolute -translate-x-1/2"
            style={{ left: "50%" }}
          >
            50
          </span>
        )}
        <span
          className="absolute -translate-x-1/2"
          style={{ left: `${value}%` }}
        >
          {value}
        </span>
        <span className="absolute right-0">100</span>
      </div>
    </div>
  );
}

// Per-card AC/battery sub-profile tabs used by the lighting cards, with the
// Synapse link/unlink toggle. Linked = one shared profile for both power
// sources, shown as a single merged tab.
function LightingPowerTabs({
  value,
  linked,
  onChange,
  onToggleLinked,
}: {
  value: PowerSource;
  linked: boolean;
  onChange: (value: PowerSource) => void;
  onToggleLinked: () => void;
}) {
  return (
    <div className="flex items-center border-b border-border">
      {linked ? (
        <span className="-mb-px border border-border border-b-card bg-card px-5 py-2.5 text-[15px] text-primary">
          Plugged In&ensp;/ On Battery
        </span>
      ) : (
        (
          [
            ["pluggedIn", "Plugged In"],
            ["onBattery", "On Battery"],
          ] as const
        ).map(([tabValue, label]) => (
          <button
            key={tabValue}
            onClick={() => onChange(tabValue)}
            className={`-mb-px px-5 py-2.5 text-[15px] transition-colors ${
              value === tabValue
                ? "border border-border border-b-card bg-card text-primary"
                : "border border-transparent bg-secondary text-foreground/90 hover:text-foreground"
            }`}
          >
            {label}
          </button>
        ))
      )}
      <button
        onClick={onToggleLinked}
        title={
          linked
            ? "Profiles linked across power sources"
            : "Separate profiles per power source"
        }
        className="ml-3 pb-1 text-foreground/80 hover:text-primary"
      >
        {linked ? <Link2 className="size-5" /> : <Unlink className="size-5" />}
      </button>
    </div>
  );
}
