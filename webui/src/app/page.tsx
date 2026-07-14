"use client";

// Synapse-style control surface. Pure IPC client: every action becomes one
// daemon protocol line via daemonRequest(); no policy lives here. Controls
// the daemon cannot drive yet render locked instead of pretending.

import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  BatteryCharging,
  Fan,
  Gauge,
  Lightbulb,
  Lock,
  RefreshCw,
  Settings,
  Waves,
} from "lucide-react";

import { Button } from "@/components/ui/button";
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
  transportLabel,
} from "@/lib/daemon";

type FanChoice = "auto" | "manual";
type PowerSource = "pluggedIn" | "onBattery";
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

  const send = async (line: string) => {
    setLastResponse(await daemonRequest(line));
    await refresh();
  };

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
                          className={`-mb-px px-5 py-2 text-sm transition-colors ${
                            power === value
                              ? "border border-border border-b-background bg-secondary text-primary"
                              : "border border-transparent text-muted-foreground hover:text-foreground"
                          }`}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                  </div>

                  {/* Mode tiles */}
                  <div className="grid grid-cols-2 gap-4">
                    <ModeTile
                      icon={<Gauge className="size-7" />}
                      label="Balanced"
                      selected
                    />
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
                  </div>

                  {/* Fan speed */}
                  <div className="space-y-4">
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

                  <Separator />

                  {/* Voltage optimizer — locked */}
                  <div className="space-y-2">
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
                    Profiles are stored per power source; automatic switching
                    on AC/battery events arrives with the diagnostics
                    milestone. Silent and Custom modes unlock with the HID
                    protocol import.
                  </p>
                </CardContent>
              </Card>
            )}

            {tab === "battery" && (
              <Card>
                <CardContent className="space-y-6 pt-6">
                  <div className="flex items-center gap-2">
                    <SectionTitle>Battery Health Optimizer</SectionTitle>
                    <BatteryCharging className="size-4 text-primary" />
                  </div>
                  <p className="text-sm text-foreground">
                    Battery will stop charging when it has reached the limit
                    (%).
                  </p>
                  <div className="flex items-end gap-4">
                    <BubbleSlider
                      min={BHO_MIN}
                      max={BHO_MAX}
                      step={1}
                      value={bho}
                      onChange={setBho}
                      onCommit={(value) => send(`bho ${value}`)}
                    />
                  </div>
                  <div className="flex justify-between text-xs text-muted-foreground">
                    <span>{BHO_MIN}</span>
                    <span>{BHO_MAX}</span>
                  </div>
                </CardContent>
              </Card>
            )}

            {tab === "lighting" && (
              <Card>
                <CardContent className="space-y-3 pt-6">
                  <div className="flex items-center gap-2">
                    <SectionTitle>Lighting</SectionTitle>
                    <Lightbulb className="size-4 text-muted-foreground" />
                  </div>
                  <p className="text-sm text-muted-foreground">
                    Keyboard brightness and Chroma effects arrive with the HID
                    protocol import (next milestone). No mock controls are
                    shown for hardware the daemon cannot drive yet.
                  </p>
                </CardContent>
              </Card>
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
  onChange,
  onCommit,
}: {
  min: number;
  max: number;
  step: number;
  value: number;
  marker?: number;
  gradient?: boolean;
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
