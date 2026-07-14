"use client";

// Synapse-style control surface. Pure IPC client: every action becomes one
// daemon protocol line via daemonRequest(); no policy lives here.

import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  BatteryCharging,
  Fan,
  Gauge,
  Lightbulb,
  Lock,
  RefreshCw,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Slider } from "@/components/ui/slider";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  BHO_MAX,
  BHO_MIN,
  DEVICE_ID,
  DEVICE_NAME,
  FAN_MAX_RPM,
  FAN_MIN_RPM,
  daemonRequest,
  transportLabel,
} from "@/lib/daemon";

type FanChoice = "auto" | "manual";

const fade = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.18, ease: "easeOut" as const },
};

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-[13px] font-semibold uppercase tracking-[0.15em] text-primary">
      {children}
    </h2>
  );
}

export default function Home() {
  const [tab, setTab] = useState("performance");
  const [fanChoice, setFanChoice] = useState<FanChoice>("auto");
  const [fanRpm, setFanRpm] = useState(
    Math.round((FAN_MIN_RPM + FAN_MAX_RPM) / 2 / 100) * 100,
  );
  const [bho, setBho] = useState(80);
  const [status, setStatus] = useState("");
  const [lastResponse, setLastResponse] = useState("");
  const [transport, setTransport] = useState("…");

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

  const applyFan = () =>
    send(fanChoice === "auto" ? "fan auto" : `fan manual ${fanRpm}`);

  return (
    <div className="flex min-h-screen flex-col bg-background text-foreground">
      {/* Header */}
      <header className="border-b border-border">
        <p className="pt-4 text-center text-sm font-medium tracking-[0.2em] text-foreground">
          RAZER BLADE 14
        </p>
        <Tabs value={tab} onValueChange={setTab} className="items-center">
          <TabsList className="mb-3 mt-2 gap-1 bg-transparent">
            {["performance", "battery", "lighting"].map((value) => (
              <TabsTrigger
                key={value}
                value={value}
                className="rounded-full px-5 text-xs uppercase tracking-wider text-muted-foreground transition-colors data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
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
              <div className="space-y-6">
                <Card>
                  <CardContent className="space-y-6 pt-6">
                    <SectionTitle>Fan</SectionTitle>
                    <div className="grid grid-cols-2 gap-4">
                      <ModeTile
                        icon={<Gauge className="size-6" />}
                        label="Automatic"
                        caption="EC-managed cooling"
                        selected={fanChoice === "auto"}
                        onClick={() => setFanChoice("auto")}
                      />
                      <ModeTile
                        icon={<Fan className="size-6" />}
                        label="Manual"
                        caption="Fixed RPM, failsafe protected"
                        selected={fanChoice === "manual"}
                        onClick={() => setFanChoice("manual")}
                      />
                    </div>
                    <div
                      className={`flex items-center gap-4 transition-opacity ${
                        fanChoice === "manual"
                          ? ""
                          : "pointer-events-none opacity-40"
                      }`}
                    >
                      <Slider
                        value={[fanRpm]}
                        min={FAN_MIN_RPM}
                        max={FAN_MAX_RPM}
                        step={100}
                        onValueChange={([value]) => setFanRpm(value)}
                      />
                      <Badge className="shrink-0 rounded bg-primary font-mono text-primary-foreground">
                        {fanRpm} RPM
                      </Badge>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Range {FAN_MIN_RPM}–{FAN_MAX_RPM} RPM. The daemon rejects
                      anything outside it; manual mode reverts to automatic if
                      the daemon stops.
                    </p>
                    <ApplyButton onClick={applyFan} />
                  </CardContent>
                </Card>

                <Card>
                  <CardContent className="space-y-3 pt-6">
                    <div className="flex items-center gap-2">
                      <SectionTitle>CPU / GPU Boost</SectionTitle>
                      <Lock className="size-3.5 text-muted-foreground" />
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Locked. Boost and GPU TDP stay disabled until the safe
                      controls have on-device mileage; the daemon rejects them
                      without an explicit opt-in.
                    </p>
                  </CardContent>
                </Card>
              </div>
            )}

            {tab === "battery" && (
              <Card>
                <CardContent className="space-y-6 pt-6">
                  <div className="flex items-center gap-2">
                    <SectionTitle>Battery Health Optimizer</SectionTitle>
                    <BatteryCharging className="size-4 text-primary" />
                  </div>
                  <p className="text-xs text-foreground">
                    Battery will stop charging when it has reached the limit
                    (%).
                  </p>
                  <div className="flex items-center gap-4">
                    <Slider
                      value={[bho]}
                      min={BHO_MIN}
                      max={BHO_MAX}
                      step={1}
                      onValueChange={([value]) => setBho(value)}
                    />
                    <Badge className="shrink-0 rounded bg-primary font-mono text-primary-foreground">
                      {bho}%
                    </Badge>
                  </div>
                  <div className="flex justify-between text-xs text-muted-foreground">
                    <span>{BHO_MIN}</span>
                    <span>{BHO_MAX}</span>
                  </div>
                  <ApplyButton onClick={() => send(`bho ${bho}`)} />
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
                  <p className="text-xs text-muted-foreground">
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

function ModeTile({
  icon,
  label,
  caption,
  selected,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  caption: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <motion.button
      whileTap={{ scale: 0.98 }}
      onClick={onClick}
      className={`flex flex-col items-start gap-2 rounded-md border p-4 text-left transition-colors ${
        selected
          ? "border-primary bg-secondary text-primary"
          : "border-border bg-secondary text-foreground hover:border-muted-foreground"
      }`}
    >
      {icon}
      <span className="text-sm font-medium">{label}</span>
      <span className="text-xs text-muted-foreground">{caption}</span>
    </motion.button>
  );
}

function ApplyButton({ onClick }: { onClick: () => void }) {
  return (
    <motion.div whileTap={{ scale: 0.97 }} className="w-fit">
      <Button
        onClick={onClick}
        className="rounded-full bg-primary px-8 text-xs font-semibold uppercase tracking-wider text-primary-foreground hover:bg-primary/90"
      >
        Apply
      </Button>
    </motion.div>
  );
}
