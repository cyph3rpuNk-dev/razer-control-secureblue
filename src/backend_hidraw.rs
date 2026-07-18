//! Real EC access over hidraw, via the hidapi crate.
//!
//! Compiled only with the `hidraw-backend` feature and selected only by the
//! explicit `daemon --backend hidraw` flag; dry-run remains the default in
//! every build.  The send/response discipline (feature reports, settle
//! delay, single retry on BUSY) follows razer-control-revived and
//! fang-razer-linux (both GPL-2.0), which run this exchange on real Blades.

use std::cell::Cell;
use std::thread;
use std::time::Duration;

use crate::backend::{Backend, HidCandidate, select_hid_candidate};
use crate::protocol::{self, CrcWindow, Packet, REPORT_LEN, ValidResponse};
use crate::{DeviceId, EcContext, RequestedOperation, find_device};

/// EC settle time between writing a command and reading its response.
const RESPONSE_DELAY: Duration = Duration::from_micros(1500);
/// Back-off before the single retry when the EC reports BUSY.
const BUSY_RETRY_DELAY: Duration = Duration::from_millis(20);

pub struct HidrawBackend {
    device: hidapi::HidDevice,
    id: DeviceId,
    /// Set once the first successful exchange has logged which CRC window
    /// the EC's responses satisfy, so Phase 3 journals record the evidence
    /// without per-packet noise.
    crc_window_logged: Cell<bool>,
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
            crc_window_logged: Cell::new(false),
        })
    }

    fn send_packet(&self, packet: &Packet) -> Result<(), String> {
        self.exchange(packet).map(|_| ())
    }

    /// Sends a query packet and returns the full validated response report
    /// and the CRC window it satisfied; callers decode args via
    /// [`protocol::RESPONSE_ARGS_OFFSET`].  Phase 3 verification uses this
    /// for the read-only 0x8x commands before any write is attempted.
    pub fn query(&self, packet: &Packet) -> Result<([u8; REPORT_LEN], CrcWindow), String> {
        self.exchange(packet)
    }

    /// One send/validate round trip.  Two independent single retries: one
    /// for a BUSY verdict (after a back-off) and one for a malformed or
    /// stale frame (immediate re-send), so a transient BUSY followed by one
    /// stale read still succeeds.  Only a fully validated SUCCESSFUL frame
    /// ever returns `Ok`.
    fn exchange(&self, packet: &Packet) -> Result<([u8; REPORT_LEN], CrcWindow), String> {
        let report = packet.to_feature_report();
        let mut busy_retried = false;
        let mut malformed_retried = false;
        loop {
            self.device
                .send_feature_report(&report)
                .map_err(|error| format!("feature-report write failed: {error}"))?;
            thread::sleep(RESPONSE_DELAY);

            let mut response = [0u8; REPORT_LEN];
            let read = self
                .device
                .get_feature_report(&mut response)
                .map_err(|error| format!("feature-report read failed: {error}"))?;

            match protocol::validate_response(packet, &response[..read]) {
                Ok(ValidResponse::Success { crc_window }) => {
                    if !self.crc_window_logged.replace(true) {
                        eprintln!("EC responses match the {} CRC window", crc_window.as_str());
                    }
                    return Ok((response, crc_window));
                }
                Ok(ValidResponse::Busy) if !busy_retried => {
                    busy_retried = true;
                    thread::sleep(BUSY_RETRY_DELAY);
                }
                Ok(ValidResponse::Busy) => return Err("EC still busy after retry".to_owned()),
                Err(error) if error.is_retryable() && !malformed_retried => {
                    malformed_retried = true;
                }
                Err(error) if error.is_retryable() => {
                    return Err(format!("invalid EC response: {error}"));
                }
                // The EC's own verdicts pass through unwrapped.
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

impl Backend for HidrawBackend {
    fn name(&self) -> &'static str {
        "hidraw"
    }

    fn apply(
        &mut self,
        device: DeviceId,
        operation: RequestedOperation,
        context: EcContext,
    ) -> Result<(), String> {
        if device != self.id {
            return Err(format!(
                "backend holds {:04x}:{:04x}, not {:04x}:{:04x}",
                self.id.vendor_id, self.id.product_id, device.vendor_id, device.product_id
            ));
        }
        for packet in &protocol::operation_packets(operation, context) {
            self.send_packet(packet)?;
        }
        Ok(())
    }
}
