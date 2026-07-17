//! Safety policy and device facts for a future Razer Blade HID controller.
//!
//! The crate purposefully contains no HID write implementation yet.  The first
//! release boundary is to prevent an unsupported model or an unsafe value from
//! ever reaching that layer once it exists.

use std::path::PathBuf;

pub mod backend;
#[cfg(feature = "hidraw-backend")]
pub mod backend_hidraw;
pub mod config;
pub mod daemon;
#[cfg(unix)]
pub mod daemon_unix;
pub mod ipc;
pub mod protocol;
pub mod telemetry;

pub const RAZER_VENDOR_ID: u16 = 0x1532;
pub const BLADE_14_2023_PRODUCT_ID: u16 = 0x029d;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanRange {
    pub min_rpm: u16,
    pub max_rpm: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub name: &'static str,
    pub id: DeviceId,
    pub fan_range: FanRange,
    pub supports_battery_health_optimizer: bool,
    pub supports_boost: bool,
    pub supports_logo_led: bool,
}

pub const BLADE_14_2023: DeviceCapabilities = DeviceCapabilities {
    name: "Razer Blade 14 (2023)",
    id: DeviceId {
        vendor_id: RAZER_VENDOR_ID,
        product_id: BLADE_14_2023_PRODUCT_ID,
    },
    // Source: Razer Synapse fan UI on the maintainer's Blade 14 (2023);
    // see docs/DEVICES.md. EC read-back verification still pending.
    fan_range: FanRange {
        min_rpm: 2000,
        max_rpm: 5400,
    },
    supports_battery_health_optimizer: true,
    supports_boost: true,
    // Synapse 4 shows a LOGO control on the maintainer's own unit.
    supports_logo_led: true,
};

pub fn find_device(id: DeviceId) -> Option<DeviceCapabilities> {
    (id == BLADE_14_2023.id).then_some(BLADE_14_2023)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Manual(u16),
}

/// CPU/GPU power level inside the Custom profile.  Wire values and names
/// follow the razer-laptop-control lineage: 0=low, 1=medium, 2=high,
/// 3=boost (CPU only, and only on models whose table says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoostLevel {
    Low,
    Medium,
    High,
    Boost,
}

impl BoostLevel {
    pub fn wire_value(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Boost => 3,
        }
    }
}

/// Performance profile.  Balanced and Gaming are the EC's own modes 0 and
/// 1.  Silent and Custom are both EC mode 4 (custom) — Silent is the
/// fang-razer-linux preset of custom with both boosts pinned low, kept as
/// a distinct variant so the UI and persistence can show the user's intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Silent,
    Balanced,
    Gaming,
    Custom { cpu: BoostLevel, gpu: BoostLevel },
}

impl Profile {
    /// The mode byte of EC command 0x0d/0x02.
    pub fn mode_wire_value(self) -> u8 {
        match self {
            Self::Balanced => 0,
            Self::Gaming => 1,
            Self::Silent | Self::Custom { .. } => 4,
        }
    }

    /// The boost pair the profile pins, when it pins one.
    pub fn boosts(self) -> Option<(BoostLevel, BoostLevel)> {
        match self {
            Self::Silent => Some((BoostLevel::Low, BoostLevel::Low)),
            Self::Custom { cpu, gpu } => Some((cpu, gpu)),
            Self::Balanced | Self::Gaming => None,
        }
    }
}

/// Keyboard backlight effect.  Fang's four; ids follow the classic
/// matrix-effect command shared by revived, fang-protocol, and OpenRazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingEffect {
    Off,
    Static { red: u8, green: u8, blue: u8 },
    Spectrum,
    Wave,
}

/// Lid logo LED mode (the snake).  Present on the Blade 14 (2023):
/// Synapse 4 exposes a LOGO control on the maintainer's unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoMode {
    Off,
    Static,
    Breathing,
}

/// Daemon-tracked EC state that the wire encoding of *other* operations
/// depends on: the mode byte rides along in every fan-state packet, and the
/// manual-fan flag rides along in every profile packet.  Without this a fan
/// toggle would silently reset the EC to Balanced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EcContext {
    pub profile: Profile,
    pub fan_manual: bool,
}

impl Default for EcContext {
    fn default() -> Self {
        Self {
            profile: Profile::Balanced,
            fan_manual: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedOperation {
    Fan(FanMode),
    BatteryHealthLimit(u8),
    BatteryHealthOff,
    /// Experimental until Phase 3 verifies the perf-mode packets on
    /// hardware; the daemon refuses it without the opt-in flag.
    Profile(Profile),
    /// Keyboard backlight brightness in percent (0-100).  Experimental
    /// until Phase 3 verifies the lighting packets on hardware.
    KeyboardBrightness(u8),
    /// Keyboard backlight effect.  Experimental, as above.
    KeyboardEffect(LightingEffect),
    /// Lid logo LED mode.  Experimental, and gated on the device table.
    Logo(LogoMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    UnsupportedDevice(DeviceId),
    FanOutOfRange { requested: u16, range: FanRange },
    InvalidChargeLimit(u8),
    InvalidBrightness(u8),
    FeatureUnsupported(&'static str),
    ExperimentalFeatureDisabled(&'static str),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedDevice(id) => write!(
                f,
                "unsupported device {:04x}:{:04x}; refusing to send a hardware command",
                id.vendor_id, id.product_id
            ),
            Self::FanOutOfRange { requested, range } => write!(
                f,
                "manual fan speed {requested} RPM is outside the verified {}–{} RPM range",
                range.min_rpm, range.max_rpm
            ),
            Self::InvalidChargeLimit(limit) => write!(
                f,
                "battery health limit {limit}% is invalid; choose a value from 50% through 80%",
            ),
            Self::InvalidBrightness(value) => {
                write!(f, "brightness {value}% is invalid; choose 0% through 100%")
            }
            Self::FeatureUnsupported(feature) => {
                write!(f, "{feature} is not supported on this device")
            }
            Self::ExperimentalFeatureDisabled(feature) => {
                write!(f, "{feature} is experimental and requires explicit opt-in",)
            }
        }
    }
}

impl std::error::Error for PolicyError {}

pub fn validate_operation(
    id: DeviceId,
    operation: RequestedOperation,
    allow_experimental: bool,
) -> Result<(), PolicyError> {
    let device = find_device(id).ok_or(PolicyError::UnsupportedDevice(id))?;

    match operation {
        RequestedOperation::Fan(FanMode::Auto) => Ok(()),
        RequestedOperation::Fan(FanMode::Manual(rpm))
            if (device.fan_range.min_rpm..=device.fan_range.max_rpm).contains(&rpm) =>
        {
            Ok(())
        }
        RequestedOperation::Fan(FanMode::Manual(requested)) => Err(PolicyError::FanOutOfRange {
            requested,
            range: device.fan_range,
        }),
        RequestedOperation::BatteryHealthLimit(limit)
            if device.supports_battery_health_optimizer && (50..=80).contains(&limit) =>
        {
            Ok(())
        }
        RequestedOperation::BatteryHealthLimit(limit) => {
            Err(PolicyError::InvalidChargeLimit(limit))
        }
        RequestedOperation::BatteryHealthOff if device.supports_battery_health_optimizer => Ok(()),
        RequestedOperation::BatteryHealthOff => {
            Err(PolicyError::FeatureUnsupported("battery health optimizer"))
        }
        RequestedOperation::KeyboardBrightness(percent) => {
            if !allow_experimental {
                return Err(PolicyError::ExperimentalFeatureDisabled(
                    "keyboard lighting",
                ));
            }
            if percent > 100 {
                return Err(PolicyError::InvalidBrightness(percent));
            }
            Ok(())
        }
        RequestedOperation::KeyboardEffect(_) if allow_experimental => Ok(()),
        RequestedOperation::KeyboardEffect(_) => Err(PolicyError::ExperimentalFeatureDisabled(
            "keyboard lighting",
        )),
        RequestedOperation::Logo(_) if !device.supports_logo_led => {
            Err(PolicyError::FeatureUnsupported("logo LED"))
        }
        RequestedOperation::Logo(_) if allow_experimental => Ok(()),
        RequestedOperation::Logo(_) => {
            Err(PolicyError::ExperimentalFeatureDisabled("logo lighting"))
        }
        RequestedOperation::Profile(profile) => {
            if !allow_experimental {
                return Err(PolicyError::ExperimentalFeatureDisabled(
                    "performance profiles",
                ));
            }
            match profile {
                // The GPU has no boost level 3 anywhere in the lineage.
                Profile::Custom {
                    gpu: BoostLevel::Boost,
                    ..
                } => Err(PolicyError::FeatureUnsupported("GPU boost level")),
                Profile::Custom {
                    cpu: BoostLevel::Boost,
                    ..
                } if !device.supports_boost => {
                    Err(PolicyError::FeatureUnsupported("CPU boost level"))
                }
                _ => Ok(()),
            }
        }
    }
}

pub fn runtime_directory(xdg_runtime_dir: Option<&str>) -> Option<PathBuf> {
    xdg_runtime_dir.map(|directory| PathBuf::from(directory).join("razer-control"))
}

pub fn blade_14_2023_udev_rule() -> &'static str {
    "KERNEL==\"hidraw*\", ATTRS{idVendor}==\"1532\", ATTRS{idProduct}==\"029d\", TAG+=\"uaccess\""
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: DeviceId = DeviceId {
        vendor_id: RAZER_VENDOR_ID,
        product_id: BLADE_14_2023_PRODUCT_ID,
    };

    #[test]
    fn recognises_blade_14_2023() {
        assert_eq!(find_device(DEVICE), Some(BLADE_14_2023));
    }

    #[test]
    fn rejects_unknown_hardware_before_any_write_layer() {
        let unknown = DeviceId {
            vendor_id: RAZER_VENDOR_ID,
            product_id: 0xffff,
        };
        assert!(matches!(
            validate_operation(unknown, RequestedOperation::Fan(FanMode::Auto), false),
            Err(PolicyError::UnsupportedDevice(_))
        ));
    }

    #[test]
    fn allows_auto_fan_and_verified_manual_range() {
        assert!(validate_operation(DEVICE, RequestedOperation::Fan(FanMode::Auto), false).is_ok());
        assert!(
            validate_operation(
                DEVICE,
                RequestedOperation::Fan(FanMode::Manual(2000)),
                false
            )
            .is_ok()
        );
        assert!(
            validate_operation(
                DEVICE,
                RequestedOperation::Fan(FanMode::Manual(5400)),
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_manual_fans_outside_the_verified_range() {
        assert!(matches!(
            validate_operation(
                DEVICE,
                RequestedOperation::Fan(FanMode::Manual(1999)),
                false
            ),
            Err(PolicyError::FanOutOfRange { .. })
        ));
        assert!(matches!(
            validate_operation(
                DEVICE,
                RequestedOperation::Fan(FanMode::Manual(5401)),
                false
            ),
            Err(PolicyError::FanOutOfRange { .. })
        ));
    }

    #[test]
    fn enforces_razer_battery_health_limits() {
        assert!(
            validate_operation(DEVICE, RequestedOperation::BatteryHealthLimit(50), false).is_ok()
        );
        assert!(
            validate_operation(DEVICE, RequestedOperation::BatteryHealthLimit(80), false).is_ok()
        );
        assert!(matches!(
            validate_operation(DEVICE, RequestedOperation::BatteryHealthLimit(81), false),
            Err(PolicyError::InvalidChargeLimit(81))
        ));
    }

    #[test]
    fn keeps_performance_profiles_experimental() {
        assert!(matches!(
            validate_operation(DEVICE, RequestedOperation::Profile(Profile::Gaming), false),
            Err(PolicyError::ExperimentalFeatureDisabled(_))
        ));
        assert!(
            validate_operation(DEVICE, RequestedOperation::Profile(Profile::Gaming), true).is_ok()
        );
        assert!(
            validate_operation(DEVICE, RequestedOperation::Profile(Profile::Silent), true).is_ok()
        );
    }

    #[test]
    fn custom_profile_boost_levels_follow_the_device_table() {
        let custom = |cpu, gpu| RequestedOperation::Profile(Profile::Custom { cpu, gpu });
        // The Blade 14 (2023) supports CPU boost level 3.
        assert!(
            validate_operation(DEVICE, custom(BoostLevel::Boost, BoostLevel::High), true).is_ok()
        );
        // No device supports a GPU "boost" level.
        assert!(matches!(
            validate_operation(DEVICE, custom(BoostLevel::Low, BoostLevel::Boost), true),
            Err(PolicyError::FeatureUnsupported(_))
        ));
    }

    #[test]
    fn keeps_lighting_experimental_and_validates_ranges() {
        assert!(matches!(
            validate_operation(DEVICE, RequestedOperation::KeyboardBrightness(50), false),
            Err(PolicyError::ExperimentalFeatureDisabled(_))
        ));
        assert!(
            validate_operation(DEVICE, RequestedOperation::KeyboardBrightness(50), true).is_ok()
        );
        assert!(matches!(
            validate_operation(DEVICE, RequestedOperation::KeyboardBrightness(101), true),
            Err(PolicyError::InvalidBrightness(101))
        ));
        assert!(
            validate_operation(
                DEVICE,
                RequestedOperation::KeyboardEffect(LightingEffect::Spectrum),
                true
            )
            .is_ok()
        );
        assert!(
            validate_operation(DEVICE, RequestedOperation::Logo(LogoMode::Breathing), true).is_ok()
        );
    }

    #[test]
    fn uses_the_user_runtime_directory_not_tmp() {
        assert_eq!(
            runtime_directory(Some("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000/razer-control"))
        );
        assert_eq!(runtime_directory(None), None);
    }

    #[test]
    fn udev_rule_never_grants_world_write_access() {
        let rule = blade_14_2023_udev_rule();
        assert!(rule.contains("TAG+=\"uaccess\""));
        assert!(!rule.contains("MODE="));
        assert!(!rule.contains("0666"));
    }
}
