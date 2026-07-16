//! Real EC access over hidraw, via the hidapi crate.
//!
//! Compiled only with the `hidraw-backend` feature and selected only by the
//! explicit `daemon --backend hidraw` flag; dry-run remains the default in
//! every build.  The send/response discipline (feature reports, settle
//! delay, single retry on BUSY) follows razer-control-revived and
//! fang-razer-linux (both GPL-2.0), which run this exchange on real Blades.

use std::thread;
use std::time::Duration;

use crate::backend::{Backend, HidCandidate, select_hid_candidate};
use crate::protocol::{self, Packet, REPORT_LEN, ResponseStatus};
use crate::{DeviceId, RequestedOperation, find_device};

/// EC settle time between writing a command and reading its response.
const RESPONSE_DELAY: Duration = Duration::from_micros(1500);
/// Back-off before the single retry when the EC reports BUSY.
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(20);

pub struct HidrawBackend {
    device: hidapi::HidDevice,
    id: DeviceId,
}

impl HidrawBackend {
    /// Opens the EC interface of a device in the capability table.  Unknown
    /// hardware is refused here as well as in policy: this constructor never
    /// opens a device the table cannot vouch for.
    pub fn open(expected: DeviceId) -> Result<Self, String> {
        find_device(expected).ok_or_else(|| {
            format!(
                "refusing to open unsupported device {:04x}:{:04x}",
                expected.vendor_id, expected.product_id
            )
        })?;
        let api =
            hidapi::HidApi::new().map_err(|error| format!("cannot initialise hidapi: {error}"))?;
        let infos: Vec<&hidapi::DeviceInfo> = api.device_list().collect();
        let candidates: Vec<HidCandidate> = infos
            .iter()
            .map(|info| HidCandidate {
                vendor_id: info.vendor_id(),
                product_id: info.product_id(),
                interface_number: info.interface_number(),
                usage_page: info.usage_page(),
            })
            .collect();
        let index = select_hid_candidate(expected, &candidates).ok_or_else(|| {
            format!(
                "device {:04x}:{:04x} not found on the bus; check the udev rule is installed",
                expected.vendor_id, expected.product_id
            )
        })?;
        let device = infos[index]
            .open_device(&api)
            .map_err(|error| format!("cannot open the EC hidraw node: {error}"))?;
        Ok(Self {
            device,
            id: expected,
        })
    }

    fn send_packet(&self, packet: &Packet) -> Result<(), String> {
        let report = packet.to_feature_report();
        for attempt in 0..2 {
            self.device
                .send_feature_report(&report)
                .map_err(|error| format!("feature-report write failed: {error}"))?;
            thread::sleep(RESPONSE_DELAY);

            let mut response = [0u8; REPORT_LEN];
            let read = self
                .device
                .get_feature_report(&mut response)
                .map_err(|error| format!("feature-report read failed: {error}"))?;
            if read != REPORT_LEN {
                return Err(format!("short EC response: {read} of {REPORT_LEN} bytes"));
            }

            match protocol::response_status(&response) {
                Some(ResponseStatus::Busy) if attempt == 0 => {
                    thread::sleep(BUSY_RETRY_DELAY);
                    continue;
                }
                Some(ResponseStatus::Success) => {
                    // Only a successful response is required to echo the
                    // command; a BUSY reply may not carry it.
                    if !packet.matches_response(&response) {
                        return Err("EC answered a different command".to_owned());
                    }
                    return Ok(());
                }
                Some(ResponseStatus::NotSupported) => {
                    return Err("EC reports this command as unsupported".to_owned());
                }
                Some(other) => return Err(format!("EC returned {other:?}")),
                None => return Err("EC returned an unknown status byte".to_owned()),
            }
        }
        Err("EC still busy after retry".to_owned())
    }
}

impl Backend for HidrawBackend {
    fn name(&self) -> &'static str {
        "hidraw"
    }

    fn apply(&mut self, device: DeviceId, operation: RequestedOperation) -> Result<(), String> {
        if device != self.id {
            return Err(format!(
                "backend holds {:04x}:{:04x}, not {:04x}:{:04x}",
                self.id.vendor_id, self.id.product_id, device.vendor_id, device.product_id
            ));
        }
        let packets = protocol::operation_packets(operation).map_err(|error| error.to_string())?;
        for packet in &packets {
            self.send_packet(packet)?;
        }
        Ok(())
    }
}
