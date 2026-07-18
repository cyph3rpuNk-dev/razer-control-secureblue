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

use crate::{BoostLevel, EcContext, FanMode, LightingEffect, LogoMode, RequestedOperation};

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

/// Which XOR window the checksum of a successful response satisfied.
///
/// Outgoing packets always use the lineage window (see [`crc`]); real Blade
/// ECs are reported (fang-razer-linux 0.9.1) to answer with the OpenRazer
/// window instead.  Phase 3 records which one this machine's EC emits; until
/// then both are accepted and the match is surfaced here so the probe and
/// the daemon can log the evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcWindow {
    /// XOR of wire bytes 2..88: transaction id through args[78].
    Lineage,
    /// XOR of wire bytes 3..=88: remaining-packets through args[79].
    OpenRazer,
    /// The two windows coincide on this frame (wire byte 2 equals byte 88)
    /// and both matched; not evidence for either convention.
    Ambiguous,
}

impl CrcWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            CrcWindow::Lineage => "lineage",
            CrcWindow::OpenRazer => "openrazer",
            CrcWindow::Ambiguous => "ambiguous",
        }
    }
}

/// Why a response buffer was rejected.  Converted to `String` at the
/// backend boundary; [`ResponseError::is_retryable`] separates malformed or
/// stale frames (worth one re-send) from the EC's own terminal verdicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseError {
    /// Not exactly [`REPORT_LEN`] bytes.
    Length(usize),
    /// Leading HID report number was not 0x00.
    ReportId(u8),
    /// A multi-packet response; nothing this module builds elicits one.
    RemainingPackets(u16),
    /// Protocol-type byte was not 0x00.
    ProtocolType(u8),
    /// Declared data size exceeds the 80 args bytes a report can carry.
    DataSize(u8),
    /// Status byte outside the lineage's known set.
    UnknownStatus(u8),
    /// A well-formed frame carrying the EC's own non-success verdict.
    Status(ResponseStatus),
    /// Success frame whose transaction id is not the one we send.
    TransactionId(u8),
    /// Success frame answering some other command: a stale reply.
    Command { class: u8, id: u8 },
    /// Success frame not echoing the request's data size.
    DataSizeEcho { got: u8, expected: u8 },
    /// Success frame whose checksum matches neither window.
    Checksum { got: u8, lineage: u8, openrazer: u8 },
}

impl ResponseError {
    /// Whether one re-send is worth attempting.  Everything malformed or
    /// stale is; the EC's own verdicts are terminal — except NEW, which
    /// means the command has not been processed yet.
    pub fn is_retryable(self) -> bool {
        match self {
            ResponseError::Status(status) => status == ResponseStatus::New,
            _ => true,
        }
    }
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseError::Length(actual) => {
                write!(f, "EC response is {actual} bytes, expected {REPORT_LEN}")
            }
            ResponseError::ReportId(actual) => {
                write!(f, "EC response report number {actual:#04x}, expected 0x00")
            }
            ResponseError::RemainingPackets(remaining) => {
                write!(f, "multi-packet EC response ({remaining} remaining)")
            }
            ResponseError::ProtocolType(actual) => {
                write!(f, "EC response protocol type {actual:#04x}, expected 0x00")
            }
            ResponseError::DataSize(actual) => {
                write!(f, "EC response data size {actual} exceeds {ARGS_LEN}")
            }
            ResponseError::UnknownStatus(byte) => {
                write!(f, "EC returned an unknown status byte ({byte:#04x})")
            }
            ResponseError::Status(ResponseStatus::NotSupported) => {
                write!(f, "EC reports this command as unsupported")
            }
            ResponseError::Status(status) => write!(f, "EC returned {status:?}"),
            ResponseError::TransactionId(actual) => write!(
                f,
                "EC response transaction id {actual:#04x}, expected {TRANSACTION_ID:#04x}"
            ),
            ResponseError::Command { .. } => write!(f, "EC answered a different command"),
            ResponseError::DataSizeEcho { got, expected } => {
                write!(f, "EC response data size {got}, expected {expected}")
            }
            ResponseError::Checksum {
                got,
                lineage,
                openrazer,
            } => write!(
                f,
                "EC response checksum {got:#04x} matches neither window (lineage {lineage:#04x}, openrazer {openrazer:#04x})"
            ),
        }
    }
}

/// A response frame the backend may act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidResponse {
    /// EC busy; a BUSY reply is not required to echo the command, so only
    /// the frame and status were checked.  Retry — never treat as success.
    Busy,
    /// Fully validated successful reply, and which checksum window it
    /// satisfied.
    Success { crc_window: CrcWindow },
}

/// Validates `buf` as the EC's reply to `request`.
///
/// Three tiers, so failures attribute precisely: structural frame checks
/// that any genuine EC frame passes regardless of status; then the status
/// byte, where BUSY returns early (no echo required) and the EC's other
/// verdicts are surfaced as errors rather than masked by echo checks; then,
/// for SUCCESSFUL frames only, the transaction id, command and data-size
/// echoes and finally the checksum against both known windows.
pub fn validate_response(request: &Packet, buf: &[u8]) -> Result<ValidResponse, ResponseError> {
    let report: &[u8; REPORT_LEN] = buf
        .try_into()
        .map_err(|_| ResponseError::Length(buf.len()))?;
    if report[0] != 0 {
        return Err(ResponseError::ReportId(report[0]));
    }
    let remaining = u16::from_be_bytes([report[3], report[4]]);
    if remaining != 0 {
        return Err(ResponseError::RemainingPackets(remaining));
    }
    if report[5] != 0 {
        return Err(ResponseError::ProtocolType(report[5]));
    }
    let data_size = report[OFFSET_DATA_SIZE];
    if usize::from(data_size) > ARGS_LEN {
        return Err(ResponseError::DataSize(data_size));
    }

    match ResponseStatus::from_wire(report[1]) {
        None => return Err(ResponseError::UnknownStatus(report[1])),
        Some(ResponseStatus::Busy) => return Ok(ValidResponse::Busy),
        Some(ResponseStatus::Success) => {}
        Some(status) => return Err(ResponseError::Status(status)),
    }

    if report[OFFSET_TRANSACTION_ID] != TRANSACTION_ID {
        return Err(ResponseError::TransactionId(report[OFFSET_TRANSACTION_ID]));
    }
    if !request.matches_response(report) {
        return Err(ResponseError::Command {
            class: report[OFFSET_COMMAND_CLASS],
            id: report[OFFSET_COMMAND_ID],
        });
    }
    if data_size != request.data_size {
        return Err(ResponseError::DataSizeEcho {
            got: data_size,
            expected: request.data_size,
        });
    }

    let lineage = crc(report);
    let openrazer = crc_openrazer(report);
    let got = report[OFFSET_CRC];
    let crc_window = match (got == lineage, got == openrazer) {
        (true, true) => CrcWindow::Ambiguous,
        (true, false) => CrcWindow::Lineage,
        (false, true) => CrcWindow::OpenRazer,
        (false, false) => {
            return Err(ResponseError::Checksum {
                got,
                lineage,
                openrazer,
            });
        }
    };
    Ok(ValidResponse::Success { crc_window })
}

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

/// XOR checksum over wire bytes 3..=88: the OpenRazer window, which drops
/// the transaction id and covers all 80 args bytes.
///
/// Response validation only.  fang-razer-linux 0.9.1 reports that real
/// Blade ECs checksum their *responses* with this window, while the lineage
/// window above is only proven for the packets we send.  Until Phase 3
/// records which window this machine's EC emits, [`validate_response`]
/// accepts either and reports the match as a [`CrcWindow`].
fn crc_openrazer(buf: &[u8; REPORT_LEN]) -> u8 {
    buf[3..89].iter().fold(0, |acc, byte| acc ^ byte)
}

/// The EC's combined performance-mode/fan-state command: args are
/// [0x00, zone, mode, manual_flag].  Both the fan operations and the
/// profile operation build this packet, which is why every operation takes
/// an [`EcContext`] — each side must re-assert the other's current byte.
/// Mode bytes: 0 Balanced, 1 Gaming, 4 Custom (razer-control-revived
/// `set_power`, fang-protocol `set_power_mode`; the sources agree).
fn set_power_mode(zone: Zone, mode: u8, manual: bool) -> Packet {
    Packet::new(
        0x0d,
        0x02,
        0x04,
        &[0x00, zone.wire_value(), mode, manual as u8],
    )
}

/// CPU/GPU power level for the Custom profile: args [0x00, zone, level].
/// Levels 0..=3; policy validation keeps level 3 CPU-only and gated on the
/// device table before this is ever built.
fn set_boost(zone: Zone, level: BoostLevel) -> Packet {
    Packet::new(
        0x0d,
        0x07,
        0x03,
        &[0x00, zone.wire_value(), level.wire_value()],
    )
}

/// Query for the current mode and manual flag of one zone; the response
/// echoes them in args[2] and args[3].  Phase 3 verification reads these
/// before any write is attempted.
pub fn get_power_mode(zone: Zone) -> Packet {
    Packet::new(0x0d, 0x82, 0x04, &[0x00, zone.wire_value()])
}

/// Query for one zone's boost level; the response carries it in args[2].
pub fn get_boost(zone: Zone) -> Packet {
    Packet::new(0x0d, 0x87, 0x03, &[0x00, zone.wire_value()])
}

/// Query for one zone's stored manual fan setpoint; the response carries
/// RPM/100 in args[2].  This is the read-back the device table's fan range
/// comment has been waiting on.
pub fn get_fan_setpoint(zone: Zone) -> Packet {
    Packet::new(0x0d, 0x81, 0x03, &[0x00, zone.wire_value()])
}

/// Offset of args[0] within a wire report; response decoding indexes from
/// here (args[i] = report[RESPONSE_ARGS_OFFSET + i]).
pub const RESPONSE_ARGS_OFFSET: usize = OFFSET_ARGS;

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

/// Classic LED addressing used by the lighting commands (class 0x03).
/// razer-control-revived and fang-protocol agree on this family for the
/// Blade lineage; OpenRazer's razerkbd also routes the Blade 14 (2023)
/// matrix effects through it.  Should Phase 3's read-back disagree, the
/// known alternative for brightness is OpenRazer's blade-misc variant
/// (class 0x0e, ids 0x04/0x84) with the identical argument triple.
const VARSTORE: u8 = 0x01;
const LOGO_LED: u8 = 0x04;
const BACKLIGHT_LED: u8 = 0x05;

/// Keyboard backlight brightness, 0-255 (the daemon scales from percent).
fn set_keyboard_brightness(value: u8) -> Packet {
    Packet::new(0x03, 0x03, 0x03, &[VARSTORE, BACKLIGHT_LED, value])
}

/// Query for the current keyboard brightness; the response carries the
/// 0-255 value in args[2].
pub fn get_keyboard_brightness() -> Packet {
    Packet::new(0x03, 0x83, 0x03, &[VARSTORE, BACKLIGHT_LED, 0x00])
}

/// Logo LED on/off.
fn set_logo_state(on: bool) -> Packet {
    Packet::new(0x03, 0x00, 0x03, &[VARSTORE, LOGO_LED, on as u8])
}

/// Logo LED effect: 0x00 static, 0x02 breathing (classic led-effect ids).
fn set_logo_effect(effect: u8) -> Packet {
    Packet::new(0x03, 0x02, 0x03, &[VARSTORE, LOGO_LED, effect])
}

/// Keyboard matrix effect (classic 0x03/0x0a): ids and payload sizes are
/// OpenRazer's standard set — none 0x00, wave 0x01 + direction, spectrum
/// 0x04, static 0x06 + rgb.
fn matrix_effect(args: &[u8]) -> Packet {
    Packet::new(0x03, 0x0a, args.len() as u8, args)
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
/// encodes, it does not police.  `context` carries the daemon's current EC
/// state: fan operations re-assert the active profile's mode byte, and the
/// profile operation re-asserts the fan manual flag — without it either
/// side would silently reset the other.  When the daemon turns fans manual
/// it flips both zones before writing either RPM so the EC never applies a
/// target in automatic mode.
pub fn operation_packets(operation: RequestedOperation, context: EcContext) -> Vec<Packet> {
    let mode = context.profile.mode_wire_value();
    match operation {
        RequestedOperation::Fan(FanMode::Auto) => Zone::ALL
            .iter()
            .map(|&zone| set_power_mode(zone, mode, false))
            .collect(),
        RequestedOperation::Fan(FanMode::Manual(rpm)) => Zone::ALL
            .iter()
            .map(|&zone| set_power_mode(zone, mode, true))
            .chain(Zone::ALL.iter().map(|&zone| set_fan_rpm(zone, rpm)))
            .collect(),
        RequestedOperation::BatteryHealthLimit(threshold) => {
            vec![set_battery_health(true, threshold)]
        }
        // Disabling keeps the last threshold in place with the flag cleared,
        // as the lineage does; 80 is the conservative default when none is
        // tracked yet.
        RequestedOperation::BatteryHealthOff => vec![set_battery_health(false, 80)],
        RequestedOperation::Profile(profile) => {
            let new_mode = profile.mode_wire_value();
            let mut packets: Vec<Packet> = Zone::ALL
                .iter()
                .map(|&zone| set_power_mode(zone, new_mode, context.fan_manual))
                .collect();
            if let Some((cpu, gpu)) = profile.boosts() {
                packets.push(set_boost(Zone::Cpu, cpu));
                packets.push(set_boost(Zone::Gpu, gpu));
            }
            packets
        }
        // Brightness comes in as percent; the wire wants 0-255.
        RequestedOperation::KeyboardBrightness(percent) => {
            let value = ((percent as u16 * 255) / 100) as u8;
            vec![set_keyboard_brightness(value)]
        }
        RequestedOperation::KeyboardEffect(effect) => vec![match effect {
            LightingEffect::Off => matrix_effect(&[0x00]),
            // Direction 1 (left-to-right); the lineage's default.
            LightingEffect::Wave => matrix_effect(&[0x01, 0x01]),
            LightingEffect::Spectrum => matrix_effect(&[0x04]),
            LightingEffect::Static { red, green, blue } => matrix_effect(&[0x06, red, green, blue]),
        }],
        RequestedOperation::Logo(mode) => match mode {
            LogoMode::Off => vec![set_logo_state(false)],
            LogoMode::Static => vec![set_logo_state(true), set_logo_effect(0x00)],
            LogoMode::Breathing => vec![set_logo_state(true), set_logo_effect(0x02)],
        },
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

    use crate::Profile;

    fn balanced() -> EcContext {
        EcContext::default()
    }

    /// Builds a successful response frame for `request`: the EC echoes the
    /// request's wire bytes with the status set.  The CRC is a literal so a
    /// change to the validator cannot silently rewrite the expectation —
    /// for `get_fan_setpoint(Zone::Cpu)` the lineage window sums to 0x91
    /// and the OpenRazer window to 0x8e (they always differ by
    /// transaction id ^ args[79], here 0x1f ^ 0x00).
    fn golden_ok_response(request: &Packet, crc_literal: u8) -> [u8; REPORT_LEN] {
        let mut buf = request.to_feature_report();
        buf[1] = 0x02;
        buf[89] = crc_literal;
        buf
    }

    #[test]
    fn fan_auto_reverts_both_zones() {
        let packets = operation_packets(RequestedOperation::Fan(FanMode::Auto), balanced());
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
        let packets = operation_packets(RequestedOperation::Fan(FanMode::Manual(3000)), balanced());
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
    fn fan_operations_preserve_the_active_profile_mode_byte() {
        // A fan toggle while Gaming is active must keep mode byte 1, not
        // silently reset the EC to Balanced.
        let gaming = EcContext {
            profile: Profile::Gaming,
            fan_manual: false,
        };
        let packets = operation_packets(RequestedOperation::Fan(FanMode::Auto), gaming);
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x01, 0x00], 0x14),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x01, 0x00], 0x17),
            ]
        );
    }

    #[test]
    fn fan_rpm_is_sent_in_hundreds_at_the_verified_range_edges() {
        // Blade 14 (2023): 2000 RPM -> 0x14, 5400 RPM -> 0x36.
        let low = operation_packets(RequestedOperation::Fan(FanMode::Manual(2000)), balanced());
        let high = operation_packets(RequestedOperation::Fan(FanMode::Manual(5400)), balanced());
        assert_eq!(low[2].to_feature_report()[11], 0x14);
        assert_eq!(high[2].to_feature_report()[11], 0x36);
    }

    #[test]
    fn gaming_profile_sets_mode_1_on_both_zones_and_no_boost_packets() {
        let packets = operation_packets(RequestedOperation::Profile(Profile::Gaming), balanced());
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x01, 0x00], 0x14),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x01, 0x00], 0x17),
            ]
        );
    }

    #[test]
    fn silent_profile_is_custom_mode_with_both_boosts_low() {
        let packets = operation_packets(RequestedOperation::Profile(Profile::Silent), balanced());
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x04, 0x00], 0x11),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x04, 0x00], 0x12),
                golden(0x0d, 0x07, 0x03, &[0x00, 0x01, 0x00], 0x17),
                golden(0x0d, 0x07, 0x03, &[0x00, 0x02, 0x00], 0x14),
            ]
        );
    }

    #[test]
    fn custom_profile_writes_mode_4_then_each_zone_boost() {
        let custom = RequestedOperation::Profile(Profile::Custom {
            cpu: BoostLevel::Boost,
            gpu: BoostLevel::High,
        });
        let packets = operation_packets(custom, balanced());
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![
                golden(0x0d, 0x02, 0x04, &[0x00, 0x01, 0x04, 0x00], 0x11),
                golden(0x0d, 0x02, 0x04, &[0x00, 0x02, 0x04, 0x00], 0x12),
                golden(0x0d, 0x07, 0x03, &[0x00, 0x01, 0x03], 0x14),
                golden(0x0d, 0x07, 0x03, &[0x00, 0x02, 0x02], 0x16),
            ]
        );
    }

    #[test]
    fn profile_packets_preserve_the_manual_fan_flag() {
        let manual_fans = EcContext {
            profile: Profile::Balanced,
            fan_manual: true,
        };
        let packets = operation_packets(RequestedOperation::Profile(Profile::Gaming), manual_fans);
        // args[3] (wire offset 12) is the manual flag on both zone packets.
        assert_eq!(packets[0].to_feature_report()[12], 0x01);
        assert_eq!(packets[1].to_feature_report()[12], 0x01);
    }

    #[test]
    fn perf_query_packets_use_the_read_command_ids() {
        assert_eq!(
            get_power_mode(Zone::Cpu).to_feature_report(),
            golden(0x0d, 0x82, 0x04, &[0x00, 0x01], 0x95)
        );
        assert_eq!(
            get_boost(Zone::Gpu).to_feature_report(),
            golden(0x0d, 0x87, 0x03, &[0x00, 0x02], 0x94)
        );
        assert_eq!(
            get_fan_setpoint(Zone::Cpu).to_feature_report(),
            golden(0x0d, 0x81, 0x03, &[0x00, 0x01], 0x91)
        );
    }

    #[test]
    fn keyboard_brightness_scales_percent_to_wire_range() {
        // 50% -> 127 (0x7f), 100% -> 255, 0% -> 0.
        let half = operation_packets(RequestedOperation::KeyboardBrightness(50), balanced());
        let reports: Vec<_> = half.iter().map(Packet::to_feature_report).collect();
        assert_eq!(
            reports,
            vec![golden(0x03, 0x03, 0x03, &[0x01, 0x05, 0x7f], 0x67)]
        );
        let full = operation_packets(RequestedOperation::KeyboardBrightness(100), balanced());
        assert_eq!(full[0].to_feature_report()[11], 0xff);
        let off = operation_packets(RequestedOperation::KeyboardBrightness(0), balanced());
        assert_eq!(off[0].to_feature_report()[11], 0x00);
    }

    #[test]
    fn keyboard_brightness_query_uses_the_read_command_id() {
        assert_eq!(
            get_keyboard_brightness().to_feature_report(),
            golden(0x03, 0x83, 0x03, &[0x01, 0x05, 0x00], 0x98)
        );
    }

    #[test]
    fn keyboard_effects_use_the_classic_matrix_ids() {
        let effect =
            |effect| operation_packets(RequestedOperation::KeyboardEffect(effect), balanced());
        assert_eq!(
            effect(LightingEffect::Off)[0].to_feature_report(),
            golden(0x03, 0x0a, 0x01, &[0x00], 0x17)
        );
        assert_eq!(
            effect(LightingEffect::Wave)[0].to_feature_report(),
            golden(0x03, 0x0a, 0x02, &[0x01, 0x01], 0x14)
        );
        assert_eq!(
            effect(LightingEffect::Spectrum)[0].to_feature_report(),
            golden(0x03, 0x0a, 0x01, &[0x04], 0x13)
        );
        // Razer green #44d62c.
        assert_eq!(
            effect(LightingEffect::Static {
                red: 0x44,
                green: 0xd6,
                blue: 0x2c
            })[0]
                .to_feature_report(),
            golden(0x03, 0x0a, 0x04, &[0x06, 0x44, 0xd6, 0x2c], 0xaa)
        );
    }

    #[test]
    fn logo_modes_drive_led_state_then_effect() {
        let logo = |mode| operation_packets(RequestedOperation::Logo(mode), balanced());
        let off: Vec<_> = logo(LogoMode::Off)
            .iter()
            .map(Packet::to_feature_report)
            .collect();
        assert_eq!(
            off,
            vec![golden(0x03, 0x00, 0x03, &[0x01, 0x04, 0x00], 0x1a)]
        );
        let breathing: Vec<_> = logo(LogoMode::Breathing)
            .iter()
            .map(Packet::to_feature_report)
            .collect();
        assert_eq!(
            breathing,
            vec![
                golden(0x03, 0x00, 0x03, &[0x01, 0x04, 0x01], 0x1b),
                golden(0x03, 0x02, 0x03, &[0x01, 0x04, 0x02], 0x1a),
            ]
        );
        assert_eq!(
            logo(LogoMode::Static)[1].to_feature_report(),
            golden(0x03, 0x02, 0x03, &[0x01, 0x04, 0x00], 0x18)
        );
    }

    #[test]
    fn battery_health_limit_sets_the_enable_bit_over_the_threshold() {
        let packets = operation_packets(RequestedOperation::BatteryHealthLimit(80), balanced());
        let reports: Vec<_> = packets.iter().map(Packet::to_feature_report).collect();
        assert_eq!(reports, vec![golden(0x07, 0x12, 0x01, &[0xd0], 0xdb)]);
    }

    #[test]
    fn battery_health_off_clears_only_the_enable_bit() {
        let packets = operation_packets(RequestedOperation::BatteryHealthOff, balanced());
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

    #[test]
    fn success_response_with_lineage_crc_is_accepted() {
        let request = get_fan_setpoint(Zone::Cpu);
        let response = golden_ok_response(&request, 0x91);
        assert_eq!(
            validate_response(&request, &response),
            Ok(ValidResponse::Success {
                crc_window: CrcWindow::Lineage
            })
        );
    }

    #[test]
    fn success_response_with_openrazer_crc_is_accepted() {
        let request = get_fan_setpoint(Zone::Cpu);
        let response = golden_ok_response(&request, 0x8e);
        assert_eq!(
            validate_response(&request, &response),
            Ok(ValidResponse::Success {
                crc_window: CrcWindow::OpenRazer
            })
        );
    }

    #[test]
    fn coinciding_crc_windows_report_ambiguous() {
        // With args[79] (wire 88) equal to the transaction id the two
        // windows sum identically; such a frame pins neither convention.
        let request = get_fan_setpoint(Zone::Cpu);
        let mut response = golden_ok_response(&request, 0x91);
        response[88] = 0x1f;
        assert_eq!(
            validate_response(&request, &response),
            Ok(ValidResponse::Success {
                crc_window: CrcWindow::Ambiguous
            })
        );
    }

    #[test]
    fn short_or_long_buffers_are_rejected_and_retryable() {
        let request = get_fan_setpoint(Zone::Cpu);
        let response = golden_ok_response(&request, 0x91);
        let error = validate_response(&request, &response[..90]).unwrap_err();
        assert_eq!(error, ResponseError::Length(90));
        assert!(error.is_retryable());
        let mut long = [0u8; REPORT_LEN + 1];
        long[..REPORT_LEN].copy_from_slice(&response);
        assert_eq!(
            validate_response(&request, &long),
            Err(ResponseError::Length(REPORT_LEN + 1))
        );
    }

    #[test]
    fn invalid_framing_is_rejected() {
        let request = get_fan_setpoint(Zone::Cpu);
        let good = golden_ok_response(&request, 0x91);

        let mut wrong_report_id = good;
        wrong_report_id[0] = 0x01;
        assert_eq!(
            validate_response(&request, &wrong_report_id),
            Err(ResponseError::ReportId(0x01))
        );

        let mut packets_remaining = good;
        packets_remaining[3] = 0x01;
        assert_eq!(
            validate_response(&request, &packets_remaining),
            Err(ResponseError::RemainingPackets(256))
        );
        let mut one_remaining = good;
        one_remaining[4] = 0x01;
        assert_eq!(
            validate_response(&request, &one_remaining),
            Err(ResponseError::RemainingPackets(1))
        );

        let mut wrong_protocol = good;
        wrong_protocol[5] = 0x01;
        assert_eq!(
            validate_response(&request, &wrong_protocol),
            Err(ResponseError::ProtocolType(0x01))
        );

        let mut oversized = good;
        oversized[6] = 81;
        assert_eq!(
            validate_response(&request, &oversized),
            Err(ResponseError::DataSize(81))
        );
    }

    #[test]
    fn wrong_transaction_id_is_rejected() {
        let request = get_fan_setpoint(Zone::Cpu);
        // CRC fixed up (0x91 ^ 0x1f ^ 0x3f) so the failure attributes to
        // the transaction id, not the checksum.
        let mut response = golden_ok_response(&request, 0xb1);
        response[2] = 0x3f;
        assert_eq!(
            validate_response(&request, &response),
            Err(ResponseError::TransactionId(0x3f))
        );
    }

    #[test]
    fn stale_command_echo_is_rejected() {
        let request = get_fan_setpoint(Zone::Cpu);
        // A well-formed success frame answering get_power_mode's id
        // instead; CRC fixed up (0x91 ^ 0x81 ^ 0x82).
        let mut response = golden_ok_response(&request, 0x92);
        response[8] = 0x82;
        assert_eq!(
            validate_response(&request, &response),
            Err(ResponseError::Command {
                class: 0x0d,
                id: 0x82
            })
        );
    }

    #[test]
    fn data_size_echo_mismatch_is_rejected() {
        let request = get_fan_setpoint(Zone::Cpu);
        // Size 4 instead of the request's 3; CRC fixed up (0x91 ^ 0x03 ^ 0x04).
        let mut response = golden_ok_response(&request, 0x96);
        response[6] = 0x04;
        assert_eq!(
            validate_response(&request, &response),
            Err(ResponseError::DataSizeEcho {
                got: 4,
                expected: 3
            })
        );
    }

    #[test]
    fn crc_matching_neither_window_is_rejected() {
        let request = get_fan_setpoint(Zone::Cpu);
        let response = golden_ok_response(&request, 0x00);
        let error = validate_response(&request, &response).unwrap_err();
        assert_eq!(
            error,
            ResponseError::Checksum {
                got: 0x00,
                lineage: 0x91,
                openrazer: 0x8e
            }
        );
        assert!(error.is_retryable());
    }

    #[test]
    fn busy_reply_without_echo_is_classified_busy() {
        // A BUSY frame need not echo the command and carries no trusted
        // checksum; only the frame structure and status byte are checked.
        let request = get_fan_setpoint(Zone::Cpu);
        let mut response = [0u8; REPORT_LEN];
        response[1] = 0x01;
        response[89] = 0xaa;
        assert_eq!(
            validate_response(&request, &response),
            Ok(ValidResponse::Busy)
        );
    }

    #[test]
    fn terminal_statuses_are_errors_and_only_new_is_retryable() {
        let request = get_fan_setpoint(Zone::Cpu);
        for (byte, status) in [
            (0x03, ResponseStatus::Failure),
            (0x04, ResponseStatus::Timeout),
            (0x05, ResponseStatus::NotSupported),
        ] {
            let mut response = golden_ok_response(&request, 0x91);
            response[1] = byte;
            let error = validate_response(&request, &response).unwrap_err();
            assert_eq!(error, ResponseError::Status(status));
            assert!(!error.is_retryable());
        }
        // NEW means not processed yet — a stale frame, worth a re-send.
        let mut unprocessed = golden_ok_response(&request, 0x91);
        unprocessed[1] = 0x00;
        let error = validate_response(&request, &unprocessed).unwrap_err();
        assert_eq!(error, ResponseError::Status(ResponseStatus::New));
        assert!(error.is_retryable());
        // So is a status byte outside the known set.
        let mut unknown = golden_ok_response(&request, 0x91);
        unknown[1] = 0x99;
        let error = validate_response(&request, &unknown).unwrap_err();
        assert_eq!(error, ResponseError::UnknownStatus(0x99));
        assert!(error.is_retryable());
    }

    #[test]
    fn outgoing_crc_stays_on_the_lineage_window() {
        // The dual-window acceptance is for responses only; packets we
        // send must keep the lineage checksum real Blades accept.
        let report = get_fan_setpoint(Zone::Cpu).to_feature_report();
        assert_eq!(report[89], 0x91);
    }
}
