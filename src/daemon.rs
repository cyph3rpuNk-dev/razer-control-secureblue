//! Platform-neutral daemon core: request handling, policy enforcement, and
//! the automatic-fan failsafe.  The Unix socket transport lives in
//! `daemon_unix`; keeping this part portable lets the safety behaviour be
//! unit-tested anywhere.

use crate::backend::Backend;
use crate::config::{BatteryHealth, PersistedState};
use crate::ipc::{Request, describe_fan_mode, parse_request};
use crate::{DeviceCapabilities, FanMode, RequestedOperation, validate_operation};

pub struct Daemon<B: Backend> {
    device: DeviceCapabilities,
    backend: B,
    allow_experimental: bool,
    fan_mode: FanMode,
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
            simulate_telemetry: false,
            state: PersistedState::default(),
            dirty: false,
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
                "ok device={:04x}:{:04x} backend={} fan={} experimental={}",
                self.device.id.vendor_id,
                self.device.id.product_id,
                self.backend.name(),
                describe_fan_mode(self.fan_mode),
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
            Request::Operation(operation) => self.apply(operation),
        }
    }

    fn apply(&mut self, operation: RequestedOperation) -> String {
        if let Err(error) = validate_operation(self.device.id, operation, self.allow_experimental) {
            return format!("err {error}");
        }
        match self.backend.apply(self.device.id, operation) {
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
                    _ => {}
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

    /// Re-apply the persisted fan and battery settings through the normal
    /// validation path; called once at daemon startup.
    pub fn reapply_persisted(&mut self) {
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
    }

    /// The power source changed; apply the configured fan choice for the
    /// new source, if any.  Returns a description when something happened.
    pub fn on_power_change(&mut self, on_ac: bool) -> Option<String> {
        let configured = if on_ac {
            self.state.fan_on_ac
        } else {
            self.state.fan_on_battery
        }?;
        if configured == self.fan_mode {
            return None;
        }
        let response = self.apply(RequestedOperation::Fan(configured));
        Some(format!(
            "power source changed to {}: {response}",
            if on_ac { "ac" } else { "battery" }
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
            match self
                .backend
                .apply(self.device.id, RequestedOperation::Fan(FanMode::Auto))
            {
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
        assert!(daemon.handle_line("gpu-tdp 140").starts_with("err"));
        assert!(daemon.backend().applied.is_empty());
    }

    #[test]
    fn experimental_gate_holds_over_ipc() {
        let mut locked = daemon(false);
        assert!(locked.handle_line("boost").starts_with("err"));
        let mut opted_in = daemon(true);
        assert!(opted_in.handle_line("boost").starts_with("ok"));
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
            fan_on_ac: None,
            fan_on_battery: None,
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
    fn status_reports_fan_state() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("status").contains("fan=auto"));
        daemon.handle_line("fan manual 2500");
        assert!(daemon.handle_line("status").contains("fan=manual:2500"));
    }
}
