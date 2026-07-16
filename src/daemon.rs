//! Platform-neutral daemon core: request handling, policy enforcement, and
//! the automatic-fan failsafe.  The Unix socket transport lives in
//! `daemon_unix`; keeping this part portable lets the safety behaviour be
//! unit-tested anywhere.

use crate::backend::Backend;
use crate::ipc::{Request, describe_fan_mode, parse_request};
use crate::{DeviceCapabilities, FanMode, RequestedOperation, validate_operation};

pub struct Daemon<B: Backend> {
    device: DeviceCapabilities,
    backend: B,
    allow_experimental: bool,
    fan_mode: FanMode,
    simulate_telemetry: bool,
}

impl<B: Backend> Daemon<B> {
    pub fn new(device: DeviceCapabilities, backend: B, allow_experimental: bool) -> Self {
        Self {
            device,
            backend,
            allow_experimental,
            fan_mode: FanMode::Auto,
            simulate_telemetry: false,
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
            Request::Operation(operation) => self.apply(operation),
        }
    }

    fn apply(&mut self, operation: RequestedOperation) -> String {
        if let Err(error) = validate_operation(self.device.id, operation, self.allow_experimental) {
            return format!("err {error}");
        }
        match self.backend.apply(self.device.id, operation) {
            Ok(()) => {
                if let RequestedOperation::Fan(mode) = operation {
                    self.fan_mode = mode;
                }
                format!(
                    "ok applied {operation:?} via {} backend",
                    self.backend.name()
                )
            }
            Err(error) => format!("err backend: {error}"),
        }
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
    fn status_reports_fan_state() {
        let mut daemon = daemon(false);
        assert!(daemon.handle_line("status").contains("fan=auto"));
        daemon.handle_line("fan manual 2500");
        assert!(daemon.handle_line("status").contains("fan=manual:2500"));
    }
}
