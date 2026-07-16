//! Razer Blade EC wire protocol: pure packet construction, no I/O.
//!
//! Derived from razer-control-revived (GPL-2.0,
//! <https://github.com/encomjp/razer-control-revived>), the continuation of
//! the razer-laptop-control lineage, and cross-checked against the
//! independently derived fang-protocol crate (GPL-2.0,
//! <https://github.com/bladeandsoulx/fang-razer-linux>).  The two sources
//! agree byte-for-byte on everything this module implements.
//!
//! Nothing here touches hardware.  A backend (Phase 2) owns the hidraw node
//! and feeds these buffers to `send_feature_report`; this module exists so
//! the exact bytes for every operation are locked down by tests first.

use crate::{FanMode, RequestedOperation};

/// Feature-report buffer length, including the leading HID report number.
pub const REPORT_LEN: usize = 91;
const ARGS_LEN: usize = 80;

/// Transaction id used by the Blade laptop EC (peripherals use other values).
const TRANSACTION_ID: u8 = 0x1f;

/// Wire offsets within the 91-byte feature report.
const OFFSET_TRANSACTION_ID: usize = 2;
const OFFSET_DATA_SIZE: usize = 6;
const OFFSET_COMMAND_CLASS: usize = 7;
const OFFSET_COMMAND_ID: usize = 8;
const OFFSET_ARGS: usize = 9;
const OFFSET_CRC: usize = 89;

/// The EC's fan/performance commands address two zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Cpu,
    Gpu,
}

impl Zone {
    const ALL: [Zone; 2] = [Zone::Cpu, Zone::Gpu];

    fn wire_value(self) -> u8 {
        match self {
            Zone::Cpu => 0x01,
            Zone::Gpu => 0x02,
        }
    }
}

/// Status byte of a response report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseStatus {
    New,
    Busy,
    Success,
    Failure,
    Timeout,
    NotSupported,
}

impl ResponseStatus {
    pub fn from_wire(byte: u8) -> Option<ResponseStatus> {
        match byte {
            0x00 => Some(Self::New),
            0x01 => Some(Self::Busy),
            0x02 => Some(Self::Success),
            0x03 => Some(Self::Failure),
            0x04 => Some(Self::Timeout),
            0x05 => Some(Self::NotSupported),
            _ => None,
        }
    }
}

/// Reads the status byte out of a response buffer.
pub fn response_status(report: &[u8; REPORT_LEN]) -> Option<ResponseStatus> {
    ResponseStatus::from_wire(report[1])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The operation passed policy but has no wire encoding yet.
    NotImplemented(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotImplemented(operation) => {
                write!(f, "{operation} has no verified wire encoding yet")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// One EC command, not yet serialised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    command_class: u8,
    command_id: u8,
    data_size: u8,
    args: [u8; ARGS_LEN],
}

impl Packet {
    fn new(command_class: u8, command_id: u8, data_size: u8, arg_values: &[u8]) -> Packet {
        let mut args = [0u8; ARGS_LEN];
        args[..arg_values.len()].copy_from_slice(arg_values);
        Packet {
            command_class,
            command_id,
            data_size,
            args,
        }
    }

    /// True when `report` carries the EC's answer to this command: the
    /// response echoes the command class and id at the same wire offsets.
    pub fn matches_response(&self, report: &[u8; REPORT_LEN]) -> bool {
        report[OFFSET_COMMAND_CLASS] == self.command_class
            && report[OFFSET_COMMAND_ID] == self.command_id
    }

    /// Serialises to the exact buffer `send_feature_report` expects: report
    /// number 0x00, status 0x00 (new command), then the EC report proper.
    pub fn to_feature_report(&self) -> [u8; REPORT_LEN] {
        let mut buf = [0u8; REPORT_LEN];
        buf[OFFSET_TRANSACTION_ID] = TRANSACTION_ID;
        buf[OFFSET_DATA_SIZE] = self.data_size;
        buf[OFFSET_COMMAND_CLASS] = self.command_class;
        buf[OFFSET_COMMAND_ID] = self.command_id;
        buf[OFFSET_ARGS..OFFSET_ARGS + ARGS_LEN].copy_from_slice(&self.args);
        buf[OFFSET_CRC] = crc(&buf);
        buf
    }
}

/// XOR checksum over wire bytes 2..88: transaction id through args[78].
///
/// The final args byte (wire offset 88) is *not* covered.  That is the
/// lineage's convention — razer-control-revived XORs indices 2..88 of the
/// same 91-byte buffer, and real Blades accept those packets — so we match
/// it rather than the OpenRazer convention (which shifts the window by one).
pub fn crc(buf: &[u8; REPORT_LEN]) -> u8 {
    buf[2..88].iter().fold(0, |acc, byte| acc ^ byte)
}

/// Fan auto/manual toggle, per zone.  This is the EC's combined
/// performance-mode command: args are [0x00, zone, mode, manual_flag].
/// Until the daemon tracks a performance-mode state, mode is pinned to
/// 0x00 (Balanced), matching the lineage's default.
fn set_fan_state(zone: Zone, manual: bool) -> Packet {
    Packet::new(
        0x0d,
        0x02,
        0x04,
        &[0x00, zone.wire_value(), 0x00, manual as u8],
    )
}

/// Manual fan target, per zone.  The EC takes RPM in hundreds; policy
/// validation guarantees the verified range before this is ever built.
fn set_fan_rpm(zone: Zone, rpm: u16) -> Packet {
    debug_assert!(rpm <= 25_500, "RPM {rpm} does not fit the wire encoding");
    Packet::new(
        0x0d,
        0x01,
        0x03,
        &[0x00, zone.wire_value(), (rpm / 100) as u8],
    )
}

/// Battery Health Optimizer: bit 7 is the enable flag, bits 0..=6 the
/// charge threshold percentage.
fn set_battery_health(enabled: bool, threshold: u8) -> Packet {
    Packet::new(
        0x07,
        0x12,
        0x01,
        &[(threshold & 0x7f) | ((enabled as u8) << 7)],
    )
}

/// Query packet for the current Battery Health Optimizer state; the response
/// carries the same bit encoding in args[0].
pub fn get_battery_health() -> Packet {
    Packet::new(0x07, 0x92, 0x01, &[])
}

/// Decodes a BHO response arg byte into (enabled, threshold).
pub fn decode_battery_health(byte: u8) -> (bool, u8) {
    (byte & 0x80 != 0, byte & 0x7f)
}

/// The packet sequence implementing one accepted operation.
///
/// Callers must have run [`crate::validate_operation`] first: this function
/// encodes, it does not police.  When the daemon turns fans manual it flips
/// both zones before writing either RPM so the EC never applies a target in
/// automatic mode.
pub fn operation_packets(operation: RequestedOperation) -> Result<Vec<Packet>, ProtocolError> {
    match operation {
        RequestedOperation::Fan(FanMode::Auto) => Ok(Zone::ALL
            .iter()
            .map(|&zone| set_fan_state(zone, false))
            .collect()),
        RequestedOperation::Fan(FanMode::Manual(rpm)) => Ok(Zone::ALL
            .iter()
            .map(|&zone| set_fan_state(zone, true))
            .chain(Zone::ALL.iter().map(|&zone| set_fan_rpm(zone, rpm)))
            .collect()),
        RequestedOperation::BatteryHealthLimit(threshold) => {
            Ok(vec![set_battery_health(true, threshold)])
        }
        // Disabling keeps the last threshold in place with the flag cleared,
        // as the lineage does; 80 is the conservative default when none is
        // tracked yet.
        RequestedOperation::BatteryHealthOff => Ok(vec![set_battery_health(false, 80)]),
        RequestedOperation::Boost => Err(ProtocolError::NotImplemented("boost control")),
        RequestedOperation::GpuTdpWatts(_) => Err(ProtocolError::NotImplemented("GPU TDP control")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the expected wire buffer from first principles: every offset
    /// and the CRC literal are written out here independently, so a change
    /// to the serialiser cannot silently rewrite the expectation.
    fn golden(class: u8, id: u8, size: u8, args: &[u8], crc_literal: u8) -> [u8; REPORT_LEN] {
        let mut buf = [0u8; REPORT_LEN];
        buf[2] = 0x1f;
        buf[6] = size;
        buf[7] = class;
        buf[8] = id;
        buf[9..9 + args.len()].copy_from_slice(args);
        buf[89] = crc_literal;
        buf
    }

    #[test]
    fn fan_auto_reverts_both_zones() {
        let packets = operation_packets(RequestedOperation::Fan(FanMode::Auto)).unwrap();
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x00, 0x00], 0x15),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x00, 0x00], 0x16),
            ]
        );
    }

    #[test]
    fn fan_manual_3000_rpm_flips_both_zones_manual_before_any_rpm_write() {
        let packets = operation_packets(RequestedOperation::Fan(FanMode::Manual(3000))).unwrap();
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x00, 0x01], 0x14),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x00, 0x01], 0x17),
                golden(0x0d, 0x01, 0x03, &[0x00, 0x01, 0x1e], 0x0f),
                golden(0x0d, 0x01, 0x03, &[0x00, 0x02, 0x1e], 0x0c),
            ]
        );
    }

    #[test]
    fn fan_rpm_is_sent_in_hundreds_at_the_verified_range_edges() {
        // Blade 14 (2023): 2000 RPM -> 0x14, 5400 RPM -> 0x36.
        let low = operation_packets(RequestedOperation::Fan(FanMode::Manual(2000))).unwrap();
        let high = operation_packets(RequestedOperation::Fan(FanMode::Manual(5400))).unwrap();
        assert_eq!(low[2].to_feature_report()[11], 0x14);
        assert_eq!(high[2].to_feature_report()[11], 0x36);
    }

    #[test]
    fn battery_health_limit_sets_the_enable_bit_over_the_threshold() {
        let packets = operation_packets(RequestedOperation::BatteryHealthLimit(80)).unwrap();
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(reports, vec![golden(0x07, 0x12, 0x01, &[0xd0], 0xdb)]);
    }

    #[test]
    fn battery_health_off_clears_only_the_enable_bit() {
        let packets = operation_packets(RequestedOperation::BatteryHealthOff).unwrap();
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(reports, vec![golden(0x07, 0x12, 0x01, &[0x50], 0x5b)]);
    }

    #[test]
    fn battery_health_query_and_decode_round_trip() {
        assert_eq!(
            get_battery_health().to_feature_report(),
            golden(0x07, 0x92, 0x01, &[], 0x8b)
        );
        assert_eq!(decode_battery_health(0xd0), (true, 80));
        assert_eq!(decode_battery_health(0x50), (false, 80));
    }

    #[test]
    fn unencoded_operations_are_refused_not_guessed() {
        assert!(matches!(
            operation_packets(RequestedOperation::Boost),
            Err(ProtocolError::NotImplemented(_))
        ));
        assert!(matches!(
            operation_packets(RequestedOperation::GpuTdpWatts(80)),
            Err(ProtocolError::NotImplemented(_))
        ));
    }

    #[test]
    fn crc_covers_transaction_id_through_args_78_and_not_the_final_arg_byte() {
        let mut buf = golden(0x0d, 0x01, 0x03, &[0x00, 0x01, 0x1e], 0x0f);
        assert_eq!(crc(&buf), 0x0f);
        // The last args byte sits at wire offset 88 and is outside the
        // checksum window; flipping it must not change the CRC.
        buf[88] = 0xff;
        assert_eq!(crc(&buf), 0x0f);
        // A byte inside the window must.
        buf[10] = 0xff;
        assert_ne!(crc(&buf), 0x0f);
    }

    #[test]
    fn response_matching_requires_the_command_echo() {
        let request = get_battery_health();
        let mut response = request.to_feature_report();
        response[1] = 0x02;
        assert!(request.matches_response(&response));
        // An answer to some other command must never be accepted.
        response[8] = 0x12;
        assert!(!request.matches_response(&response));
    }

    #[test]
    fn response_status_decodes_the_lineage_status_bytes() {
        let mut report = [0u8; REPORT_LEN];
        report[1] = 0x02;
        assert_eq!(response_status(&report), Some(ResponseStatus::Success));
        report[1] = 0x01;
        assert_eq!(response_status(&report), Some(ResponseStatus::Busy));
        report[1] = 0x05;
        assert_eq!(response_status(&report), Some(ResponseStatus::NotSupported));
        report[1] = 0x99;
        assert_eq!(response_status(&report), None);
    }
}
