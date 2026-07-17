//! Platform-neutral daemon core: request handling, policy enforcement, and
//! the automatic-fan failsafe.  The Unix socket transport lives in
//! `daemon_unix`; keeping this part portable lets the safety behaviour be
//! unit-tested anywhere.

use crate::backend::Backend;
use crate::config::{BatteryHealth, PersistedState};
use crate::ipc::{
    Request, describe_effect, describe_fan_mode, describe_logo, describe_profile, parse_request,
};
use crate::{
    DeviceCapabilities, EcContext, FanMode, Profile, RequestedOperation, validate_operation,
};

pub struct Daemon<B: Backend> {
    device: DeviceCapabilities,
    backend: B,
    allow_experimental: bool,
    fan_mode: FanMode,
    profile: Profile,
    simulate_telemetry: bool,
    state: PersistedState,
    dirty: bool,
}

impl<B: Backend> Daemon<B> {
    pub fn new(device: DeviceCapabilities, backend: B, allow_experimental: bool) -> Self {
        Self {
            device,
            backend,
            allow_experimental,
            fan_mode: FanMode::Auto,
            profile: Profile::Balanced,
            simulate_telemetry: false,
            state: PersistedState::default(),
            dirty: false,
        }
    }

    /// The EC state the wire encoding needs alongside any operation.
    fn context(&self) -> EcContext {
        EcContext {
            profile: self.profile,
            fan_manual: matches!(self.fan_mode, FanMode::Manual(_)),
        }
    }

    /// Used by the GUI's in-process mock so dashboards can be developed on
    /// machines with no hwmon data; responses stay labelled `simulated=true`.
    pub fn with_simulated_telemetry(mut self) -> Self {
        self.simulate_telemetry = true;
        self
    }

    /// Handle one request line and return the response line.
    pub fn handle_line(&mut self, line: &str) -> String {
        let request = match parse_request(line) {
            Ok(request) => request,
            Err(error) => return format!("err {error}"),
        };
        match request {
            Request::Ping => "ok pong".to_owned(),
            Request::Status => format!(
                "ok device={:04x}:{:04x} backend={} fan={} profile={} automation_ac={} automation_battery={} kbd={} kbd_effect={} logo={} kbd_ac={} kbd_battery={} experimental={}",
                self.device.id.vendor_id,
                self.device.id.product_id,
                self.backend.name(),
                describe_fan_mode(self.fan_mode),
                describe_profile(self.profile),
                self.state
                    .fan_on_ac
                    .map_or("off".to_owned(), describe_fan_mode),
                self.state
                    .fan_on_battery
                    .map_or("off".to_owned(), describe_fan_mode),
                self.state
                    .kbd_brightness
                    .map_or("unset".to_owned(), |b| b.to_string()),
                self.state
                    .kbd_effect
                    .map_or("unset".to_owned(), describe_effect),
                self.state.logo.map_or("unset", describe_logo),
                self.state
                    .kbd_on_ac
                    .map_or("off".to_owned(), |b| b.to_string()),
                self.state
                    .kbd_on_battery
                    .map_or("off".to_owned(), |b| b.to_string()),
                self.allow_experimental
            ),
            Request::Telemetry => format!(
                "ok {}",
                crate::telemetry::read(self.simulate_telemetry).to_line()
            ),
            Request::Automation { on_ac, fan } => {
                if let Some(FanMode::Manual(rpm)) = fan
                    && let Err(error) = validate_operation(
                        self.device.id,
                        RequestedOperation::Fan(FanMode::Manual(rpm)),
                        self.allow_experimental,
                    )
                {
                    return format!("err {error}");
                }
                let slot = if on_ac {
                    &mut self.state.fan_on_ac
                } else {
                    &mut self.state.fan_on_battery
                };
                *slot = fan;
                self.dirty = true;
                format!(
                    "ok automation {}={}",
                    if on_ac { "ac" } else { "battery" },
                    fan.map_or("off".to_owned(), describe_fan_mode)
                )
            }
            Request::KbdAutomation { on_ac, brightness } => {
                if let Some(brightness) = brightness
                    && let Err(error) = validate_operation(
                        self.device.id,
                        RequestedOperation::KeyboardBrightness(brightness),
                        self.allow_experimental,
                    )
                {
                    return format!("err {error}");
                }
                let slot = if on_ac {
                    &mut self.state.kbd_on_ac
                } else {
                    &mut self.state.kbd_on_battery
                };
                *slot = brightness;
                self.dirty = true;
                format!(
                    "ok kbd-automation {}={}",
                    if on_ac { "ac" } else { "battery" },
                    brightness.map_or("off".to_owned(), |b| b.to_string())
                )
            }
            Request::Operation(operation) => self.apply(operation),
        }
    }

    fn apply(&mut self, operation: RequestedOperation) -> String {
        if let Err(error) = validate_operation(self.device.id, operation, self.allow_experimental) {
            return format!("err {error}");
        }
        let context = self.context();
        match self.backend.apply(self.device.id, operation, context) {
            Ok(()) => {
                match operation {
                    RequestedOperation::Fan(mode) => {
                        self.fan_mode = mode;
                        self.state.fan = Some(mode);
                        self.dirty = true;
                    }
                    RequestedOperation::BatteryHealthLimit(limit) => {
                        self.state.battery_health = BatteryHealth::Limit(limit);
                        self.dirty = true;
                    }
                    RequestedOperation::BatteryHealthOff => {
                        self.state.battery_health = BatteryHealth::Off;
                        self.dirty = true;
                    }
                    RequestedOperation::Profile(profile) => {
                        self.profile = profile;
                        self.state.profile = Some(profile);
                        self.dirty = true;
                    }
                    RequestedOperation::KeyboardBrightness(percent) => {
                        self.state.kbd_brightness = Some(percent);
                        self.dirty = true;
                    }
                    RequestedOperation::KeyboardEffect(effect) => {
                        self.state.kbd_effect = Some(effect);
                        self.dirty = true;
                    }
                    RequestedOperation::Logo(mode) => {
                        self.state.logo = Some(mode);
                        self.dirty = true;
                    }
                }
                format!(
                    "ok applied {operation:?} via {} backend",
                    self.backend.name()
                )
            }
            Err(error) => format!("err backend: {error}"),
        }
    }

    /// Load previously persisted state without touching hardware.
    pub fn load_state(&mut self, state: PersistedState) {
        self.state = state;
    }

    /// Re-apply the persisted profile, fan, and battery settings through
    /// the normal validation path; called once at daemon startup.  Profile
    /// first, so the fan packets that follow re-assert the right mode byte.
    pub fn reapply_persisted(&mut self) {
        if let Some(profile) = self.state.profile {
            let response = self.apply(RequestedOperation::Profile(profile));
            eprintln!("reapply profile: {response}");
        }
        if let Some(mode) = self.state.fan {
            let response = self.apply(RequestedOperation::Fan(mode));
            eprintln!("reapply fan: {response}");
        }
        match self.state.battery_health {
            BatteryHealth::Unset => {}
            BatteryHealth::Off => {
                let response = self.apply(RequestedOperation::BatteryHealthOff);
                eprintln!("reapply bho: {response}");
            }
            BatteryHealth::Limit(limit) => {
                let response = self.apply(RequestedOperation::BatteryHealthLimit(limit));
                eprintln!("reapply bho: {response}");
            }
        }
        if let Some(brightness) = self.state.kbd_brightness {
            let response = self.apply(RequestedOperation::KeyboardBrightness(brightness));
            eprintln!("reapply kbd brightness: {response}");
        }
        if let Some(effect) = self.state.kbd_effect {
            let response = self.apply(RequestedOperation::KeyboardEffect(effect));
            eprintln!("reapply kbd effect: {response}");
        }
        if let Some(logo) = self.state.logo {
            let response = self.apply(RequestedOperation::Logo(logo));
            eprintln!("reapply logo: {response}");
        }
    }

    /// The power source changed; apply the configured fan and keyboard
    /// brightness for the new source, if any.  Returns a description when
    /// something happened.
    pub fn on_power_change(&mut self, on_ac: bool) -> Option<String> {
        let source = if on_ac { "ac" } else { "battery" };
        let mut actions = Vec::new();
        let configured_fan = if on_ac {
            self.state.fan_on_ac
        } else {
            self.state.fan_on_battery
        };
        if let Some(configured) = configured_fan
            && configured != self.fan_mode
        {
            let response = self.apply(RequestedOperation::Fan(configured));
            actions.push(response);
        }
        let configured_kbd = if on_ac {
            self.state.kbd_on_ac
        } else {
            self.state.kbd_on_battery
        };
        if let Some(brightness) = configured_kbd
            && self.state.kbd_brightness != Some(brightness)
        {
            let response = self.apply(RequestedOperation::KeyboardBrightness(brightness));
            actions.push(response);
        }
        if actions.is_empty() {
            return None;
        }
        Some(format!(
            "power source changed to {source}: {}",
            actions.join("; ")
        ))
    }

    pub fn persisted(&self) -> &PersistedState {
        &self.state
    }

    /// True when persisted state changed since the last call; the transport
    /// uses this to decide when to write the state file.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Failsafe: the EC must never be left in manual fan mode with no
    /// supervisor running.  Called on SIGTERM/SIGINT and normal exit.
    pub fn shutdown(&mut self) {
        if matches!(self.fan_mode, FanMode::Manual(_)) {
            eprintln!("failsafe: reverting manual fan control to automatic");
            let context = self.context();
            match self.backend.apply(
                self.device.id,
                RequestedOperation::Fan(FanMode::Auto),
                context,
            ) {
                Ok(()) => self.fan_mode = FanMode::Auto,
                Err(error) => eprintln!("failsafe FAILED, fans remain manual: {error}"),
            }
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BLADE_14_2023;
    use crate::backend::DryRunBackend;

    fn daemon(allow_experimental: bool) -> Daemon<DryRunBackend> {
        Daemon::new(BLADE_14_2023, DryRunBackend::default(), allow_experimental)
    }

    #[test]
    fn shutdown_reverts_manual_fan_to_auto() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("fan manual 3000").starts_with("ok"));
        daemon.shutdown();
        assert_eq!(
            daemon.backend().applied,
            vec![
                RequestedOperation::Fan(FanMode::Manual(3000)),
                RequestedOperation::Fan(FanMode::Auto),
            ]
        );
    }

    #[test]
    fn accepts_disabling_battery_health_optimizer() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("bho off").starts_with("ok"));
        assert_eq!(
            daemon.backend().applied,
            vec![RequestedOperation::BatteryHealthOff]
        );
    }

    #[test]
    fn shutdown_sends_nothing_when_fans_are_automatic() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("bho 80").starts_with("ok"));
        daemon.shutdown();
        assert_eq!(
            daemon.backend().applied,
            vec![RequestedOperation::BatteryHealthLimit(80)]
        );
    }

    #[test]
    fn rejected_operations_never_reach_the_backend() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("fan manual 9000").starts_with("err"));
        assert!(daemon.handle_line("bho 100").starts_with("err"));
        assert!(daemon.handle_line("profile gaming").starts_with("err"));
        assert!(daemon.backend().applied.is_empty());
    }

    #[test]
    fn experimental_gate_holds_over_ipc() {
        let mut locked = daemon(false);
        assert!(locked.handle_line("profile gaming").starts_with("err"));
        let mut opted_in = daemon(true);
        assert!(opted_in.handle_line("profile gaming").starts_with("ok"));
    }

    #[test]
    fn profile_updates_status_and_rides_along_in_later_fan_contexts() {
        let mut daemon = daemon(true);
        assert!(daemon.handle_line("profile gaming").starts_with("ok"));
        assert!(daemon.handle_line("status").contains("profile=gaming"));
        assert_eq!(daemon.persisted().profile, Some(Profile::Gaming));

        // A later fan operation must carry the Gaming profile in its
        // context so the EC's mode byte is preserved.
        assert!(daemon.handle_line("fan manual 3000").starts_with("ok"));
        assert_eq!(
            daemon.backend().contexts.last(),
            Some(&EcContext {
                profile: Profile::Gaming,
                fan_manual: false,
            })
        );

        // And the profile packets themselves saw the pre-existing fan state.
        assert!(daemon.handle_line("profile silent").starts_with("ok"));
        assert_eq!(
            daemon.backend().contexts.last(),
            Some(&EcContext {
                profile: Profile::Gaming,
                fan_manual: true,
            })
        );
    }

    #[test]
    fn telemetry_is_a_read_only_request() {
        let mut daemon = daemon(false);
        let response = daemon.handle_line("telemetry");
        assert!(response.starts_with("ok cpu_temp="));
        assert!(response.contains("simulated=false"));
        assert!(daemon.backend().applied.is_empty());
        let mut simulated =
            Daemon::new(BLADE_14_2023, DryRunBackend::default(), false).with_simulated_telemetry();
        assert!(
            simulated
                .handle_line("telemetry")
                .contains("simulated=true")
        );
    }

    #[test]
    fn automation_applies_the_configured_fan_on_power_change() {
        let mut daemon = daemon(false);
        assert!(
            daemon
                .handle_line("automation battery fan auto")
                .starts_with("ok")
        );
        assert!(
            daemon
                .handle_line("automation ac fan manual 4000")
                .starts_with("ok")
        );
        let action = daemon.on_power_change(true).unwrap();
        assert!(action.contains("ac"));
        assert_eq!(
            daemon.backend().applied,
            vec![RequestedOperation::Fan(FanMode::Manual(4000))]
        );
        // Already in the configured mode: nothing to do.
        assert!(daemon.on_power_change(true).is_none());
        // Battery rule says auto.
        assert!(daemon.on_power_change(false).is_some());
        assert_eq!(
            daemon.backend().applied.last(),
            Some(&RequestedOperation::Fan(FanMode::Auto))
        );
    }

    #[test]
    fn automation_rules_are_validated_and_persisted() {
        let mut daemon = daemon(false);
        assert!(
            daemon
                .handle_line("automation ac fan manual 9000")
                .starts_with("err")
        );
        assert!(
            daemon
                .handle_line("automation ac fan auto")
                .starts_with("ok")
        );
        assert!(daemon.take_dirty());
        assert!(!daemon.take_dirty());
        assert_eq!(daemon.persisted().fan_on_ac, Some(FanMode::Auto));
    }

    #[test]
    fn reapply_restores_persisted_settings_through_validation() {
        use crate::config::{BatteryHealth, PersistedState};
        let mut daemon = daemon(false);
        daemon.load_state(PersistedState {
            fan: Some(FanMode::Manual(3200)),
            battery_health: BatteryHealth::Limit(80),
            ..PersistedState::default()
        });
        daemon.reapply_persisted();
        assert_eq!(
            daemon.backend().applied,
            vec![
                RequestedOperation::Fan(FanMode::Manual(3200)),
                RequestedOperation::BatteryHealthLimit(80),
            ]
        );
    }

    #[test]
    fn reapply_restores_the_profile_before_the_fan() {
        let mut daemon = daemon(true);
        daemon.load_state(PersistedState {
            fan: Some(FanMode::Auto),
            profile: Some(Profile::Gaming),
            ..PersistedState::default()
        });
        daemon.reapply_persisted();
        assert_eq!(
            daemon.backend().applied,
            vec![
                RequestedOperation::Profile(Profile::Gaming),
                RequestedOperation::Fan(FanMode::Auto),
            ]
        );
        // The fan packets after reapply carry the restored profile.
        assert_eq!(
            daemon.backend().contexts.last(),
            Some(&EcContext {
                profile: Profile::Gaming,
                fan_manual: false,
            })
        );
    }

    #[test]
    fn status_reports_fan_state() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("status").contains("fan=auto"));
        daemon.handle_line("fan manual 2500");
        assert!(daemon.handle_line("status").contains("fan=manual:2500"));
    }

    #[test]
    fn lighting_over_ipc_updates_status_and_persists() {
        use crate::{LightingEffect, LogoMode};
        // Locked daemon refuses all of it.
        let mut locked = daemon(false);
        assert!(locked.handle_line("kbd brightness 60").starts_with("err"));
        assert!(locked.handle_line("logo off").starts_with("err"));
        assert!(locked.backend().applied.is_empty());

        let mut daemon = daemon(true);
        assert!(daemon.handle_line("kbd brightness 60").starts_with("ok"));
        assert!(
            daemon
                .handle_line("kbd effect static 44d62c")
                .starts_with("ok")
        );
        assert!(daemon.handle_line("logo breathing").starts_with("ok"));
        let status = daemon.handle_line("status");
        assert!(status.contains("kbd=60"));
        assert!(status.contains("kbd_effect=static:44d62c"));
        assert!(status.contains("logo=breathing"));
        assert_eq!(daemon.persisted().kbd_brightness, Some(60));
        assert_eq!(
            daemon.persisted().kbd_effect,
            Some(LightingEffect::Static {
                red: 0x44,
                green: 0xd6,
                blue: 0x2c
            })
        );
        assert_eq!(daemon.persisted().logo, Some(LogoMode::Breathing));
    }

    #[test]
    fn kbd_automation_applies_brightness_on_power_change() {
        let mut daemon = daemon(true);
        assert!(
            daemon
                .handle_line("kbd-automation battery 20")
                .starts_with("ok")
        );
        assert!(daemon.handle_line("kbd-automation ac 80").starts_with("ok"));
        let action = daemon.on_power_change(false).unwrap();
        assert!(action.contains("battery"));
        assert_eq!(
            daemon.backend().applied.last(),
            Some(&RequestedOperation::KeyboardBrightness(20))
        );
        // Already at the configured brightness: nothing to do.
        assert!(daemon.on_power_change(false).is_none());
        let status = daemon.handle_line("status");
        assert!(status.contains("kbd_ac=80"));
        assert!(status.contains("kbd_battery=20"));
    }

    #[test]
    fn status_reports_automation_rules() {
        let mut daemon = daemon(false);
        let before = daemon.handle_line("status");
        assert!(before.contains("automation_ac=off"));
        assert!(before.contains("automation_battery=off"));
        daemon.handle_line("automation ac fan auto");
        daemon.handle_line("automation battery fan manual 2000");
        let after = daemon.handle_line("status");
        assert!(after.contains("automation_ac=auto"));
        assert!(after.contains("automation_battery=manual:2000"));
    }
}
