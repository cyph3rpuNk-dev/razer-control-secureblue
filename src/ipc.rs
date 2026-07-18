//! Line-oriented IPC protocol shared by the daemon, the `ctl` client, and the
//! `validate` CLI.  One request per line; the daemon answers with a single
//! line starting with `ok` or `err`.

use crate::{BoostLevel, FanMode, LightingEffect, LogoMode, Profile, RequestedOperation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Ping,
    Status,
    Telemetry,
    SysInfo,
    /// Configure the fan choice applied when the power source changes;
    /// `None` clears the rule for that source.
    Automation {
        on_ac: bool,
        fan: Option<FanMode>,
    },
    /// Configure the keyboard brightness applied when the power source
    /// changes; `None` clears the rule for that source.
    KbdAutomation {
        on_ac: bool,
        brightness: Option<u8>,
    },
    Operation(RequestedOperation),
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.as_slice() {
        ["ping"] => Ok(Request::Ping),
        ["status"] => Ok(Request::Status),
        ["telemetry"] => Ok(Request::Telemetry),
        ["sysinfo"] => Ok(Request::SysInfo),
        ["automation", source, rest @ ..] => {
            let on_ac = match *source {
                "ac" => true,
                "battery" => false,
                other => return Err(format!("unknown power source {other:?}")),
            };
            let fan = match rest {
                ["off"] => None,
                ["fan", "auto"] => Some(FanMode::Auto),
                ["fan", "manual", rpm] => Some(FanMode::Manual(
                    rpm.parse()
                        .map_err(|_| "fan RPM must be an integer".to_owned())?,
                )),
                _ => {
                    return Err(
                        "expected: automation <ac|battery> <off|fan auto|fan manual <rpm>>"
                            .to_owned(),
                    );
                }
            };
            Ok(Request::Automation { on_ac, fan })
        }
        ["kbd-automation", source, rule] => {
            let on_ac = match *source {
                "ac" => true,
                "battery" => false,
                other => return Err(format!("unknown power source {other:?}")),
            };
            let brightness =
                if *rule == "off" {
                    None
                } else {
                    Some(rule.parse::<u8>().map_err(|_| {
                        "expected: kbd-automation <ac|battery> <0-100|off>".to_owned()
                    })?)
                };
            Ok(Request::KbdAutomation { on_ac, brightness })
        }
        other => parse_operation(other).map(Request::Operation),
    }
}

pub fn parse_operation(tokens: &[&str]) -> Result<RequestedOperation, String> {
    match tokens {
        ["fan", "auto"] => Ok(RequestedOperation::Fan(FanMode::Auto)),
        ["fan", "manual", rpm] => rpm
            .parse::<u16>()
            .map(|rpm| RequestedOperation::Fan(FanMode::Manual(rpm)))
            .map_err(|_| "fan RPM must be an integer".to_owned()),
        ["bho", "off"] => Ok(RequestedOperation::BatteryHealthOff),
        ["bho", limit] => limit
            .parse::<u8>()
            .map(RequestedOperation::BatteryHealthLimit)
            .map_err(|_| "battery health limit must be an integer".to_owned()),
        ["profile", "silent"] => Ok(RequestedOperation::Profile(Profile::Silent)),
        ["profile", "balanced"] => Ok(RequestedOperation::Profile(Profile::Balanced)),
        ["profile", "gaming"] => Ok(RequestedOperation::Profile(Profile::Gaming)),
        ["profile", "custom", "cpu", cpu, "gpu", gpu] => {
            Ok(RequestedOperation::Profile(Profile::Custom {
                cpu: parse_boost_level(cpu)?,
                gpu: parse_boost_level(gpu)?,
            }))
        }
        ["profile", ..] => Err(
            "expected: profile <silent|balanced|gaming> or profile custom cpu <level> gpu <level>"
                .to_owned(),
        ),
        ["kbd", "brightness", percent] => percent
            .parse::<u8>()
            .map(RequestedOperation::KeyboardBrightness)
            .map_err(|_| "keyboard brightness must be an integer percent".to_owned()),
        ["kbd", "effect", "off"] => Ok(RequestedOperation::KeyboardEffect(LightingEffect::Off)),
        ["kbd", "effect", "spectrum"] => {
            Ok(RequestedOperation::KeyboardEffect(LightingEffect::Spectrum))
        }
        ["kbd", "effect", "wave"] => Ok(RequestedOperation::KeyboardEffect(LightingEffect::Wave)),
        ["kbd", "effect", "static", color] => parse_rgb(color)
            .map(|(red, green, blue)| {
                RequestedOperation::KeyboardEffect(LightingEffect::Static { red, green, blue })
            })
            .ok_or_else(|| "expected: kbd effect static <rrggbb>".to_owned()),
        ["kbd", ..] => Err(
            "expected: kbd brightness <0-100> or kbd effect <off|spectrum|wave|static <rrggbb>>"
                .to_owned(),
        ),
        ["logo", "off"] => Ok(RequestedOperation::Logo(LogoMode::Off)),
        ["logo", "static"] => Ok(RequestedOperation::Logo(LogoMode::Static)),
        ["logo", "breathing"] => Ok(RequestedOperation::Logo(LogoMode::Breathing)),
        ["logo", ..] => Err("expected: logo <off|static|breathing>".to_owned()),
        _ => Err(format!("unknown request {:?}", tokens.join(" "))),
    }
}

/// `rrggbb` hex → (r, g, b).
pub fn parse_rgb(color: &str) -> Option<(u8, u8, u8)> {
    if color.len() != 6 {
        return None;
    }
    let channel = |range| u8::from_str_radix(color.get(range)?, 16).ok();
    Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

pub fn describe_effect(effect: LightingEffect) -> String {
    match effect {
        LightingEffect::Off => "off".to_owned(),
        LightingEffect::Spectrum => "spectrum".to_owned(),
        LightingEffect::Wave => "wave".to_owned(),
        LightingEffect::Static { red, green, blue } => {
            format!("static:{red:02x}{green:02x}{blue:02x}")
        }
    }
}

pub fn parse_effect(value: &str) -> Option<LightingEffect> {
    match value {
        "off" => Some(LightingEffect::Off),
        "spectrum" => Some(LightingEffect::Spectrum),
        "wave" => Some(LightingEffect::Wave),
        _ => {
            let (red, green, blue) = parse_rgb(value.strip_prefix("static:")?)?;
            Some(LightingEffect::Static { red, green, blue })
        }
    }
}

pub fn describe_logo(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Off => "off",
        LogoMode::Static => "static",
        LogoMode::Breathing => "breathing",
    }
}

pub fn parse_logo(value: &str) -> Option<LogoMode> {
    match value {
        "off" => Some(LogoMode::Off),
        "static" => Some(LogoMode::Static),
        "breathing" => Some(LogoMode::Breathing),
        _ => None,
    }
}

pub fn parse_boost_level(name: &str) -> Result<BoostLevel, String> {
    match name {
        "low" => Ok(BoostLevel::Low),
        "medium" => Ok(BoostLevel::Medium),
        "high" => Ok(BoostLevel::High),
        "boost" => Ok(BoostLevel::Boost),
        other => Err(format!(
            "unknown boost level {other:?}; expected low, medium, high, or boost"
        )),
    }
}

pub fn describe_boost_level(level: BoostLevel) -> &'static str {
    match level {
        BoostLevel::Low => "low",
        BoostLevel::Medium => "medium",
        BoostLevel::High => "high",
        BoostLevel::Boost => "boost",
    }
}

/// One-token profile description, used by `status`, persistence, and the
/// GUI; [`parse_profile`] is its inverse.
pub fn describe_profile(profile: Profile) -> String {
    match profile {
        Profile::Silent => "silent".to_owned(),
        Profile::Balanced => "balanced".to_owned(),
        Profile::Gaming => "gaming".to_owned(),
        Profile::Custom { cpu, gpu } => format!(
            "custom:cpu={},gpu={}",
            describe_boost_level(cpu),
            describe_boost_level(gpu)
        ),
    }
}

pub fn parse_profile(value: &str) -> Option<Profile> {
    match value {
        "silent" => Some(Profile::Silent),
        "balanced" => Some(Profile::Balanced),
        "gaming" => Some(Profile::Gaming),
        _ => {
            let rest = value.strip_prefix("custom:cpu=")?;
            let (cpu, gpu) = rest.split_once(",gpu=")?;
            Some(Profile::Custom {
                cpu: parse_boost_level(cpu).ok()?,
                gpu: parse_boost_level(gpu).ok()?,
            })
        }
    }
}

pub fn describe_fan_mode(mode: FanMode) -> String {
    match mode {
        FanMode::Auto => "auto".to_owned(),
        FanMode::Manual(rpm) => format!("manual:{rpm}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_operations() {
        assert_eq!(
            parse_request("profile gaming"),
            Ok(Request::Operation(RequestedOperation::Profile(
                Profile::Gaming
            )))
        );
        assert_eq!(
            parse_request("profile custom cpu boost gpu high"),
            Ok(Request::Operation(RequestedOperation::Profile(
                Profile::Custom {
                    cpu: BoostLevel::Boost,
                    gpu: BoostLevel::High,
                }
            )))
        );
        assert!(parse_request("profile custom cpu warp gpu high").is_err());
        assert!(parse_request("profile ludicrous").is_err());
    }

    #[test]
    fn profile_descriptions_round_trip() {
        for profile in [
            Profile::Silent,
            Profile::Balanced,
            Profile::Gaming,
            Profile::Custom {
                cpu: BoostLevel::Boost,
                gpu: BoostLevel::Low,
            },
        ] {
            assert_eq!(parse_profile(&describe_profile(profile)), Some(profile));
        }
        assert_eq!(parse_profile("custom:cpu=warp,gpu=low"), None);
        assert_eq!(parse_profile("garbage"), None);
    }

    #[test]
    fn parses_lighting_operations() {
        assert_eq!(
            parse_request("kbd brightness 60"),
            Ok(Request::Operation(RequestedOperation::KeyboardBrightness(
                60
            )))
        );
        assert_eq!(
            parse_request("kbd effect static 44d62c"),
            Ok(Request::Operation(RequestedOperation::KeyboardEffect(
                LightingEffect::Static {
                    red: 0x44,
                    green: 0xd6,
                    blue: 0x2c
                }
            )))
        );
        assert_eq!(
            parse_request("logo breathing"),
            Ok(Request::Operation(RequestedOperation::Logo(
                LogoMode::Breathing
            )))
        );
        assert_eq!(
            parse_request("kbd-automation battery 30"),
            Ok(Request::KbdAutomation {
                on_ac: false,
                brightness: Some(30)
            })
        );
        assert_eq!(
            parse_request("kbd-automation ac off"),
            Ok(Request::KbdAutomation {
                on_ac: true,
                brightness: None
            })
        );
        assert!(parse_request("kbd effect static zzzzzz").is_err());
        assert!(parse_request("kbd effect disco").is_err());
        assert!(parse_request("logo rainbow").is_err());
    }

    #[test]
    fn lighting_descriptions_round_trip() {
        for effect in [
            LightingEffect::Off,
            LightingEffect::Spectrum,
            LightingEffect::Wave,
            LightingEffect::Static {
                red: 0x44,
                green: 0xd6,
                blue: 0x2c,
            },
        ] {
            assert_eq!(parse_effect(&describe_effect(effect)), Some(effect));
        }
        for mode in [LogoMode::Off, LogoMode::Static, LogoMode::Breathing] {
            assert_eq!(parse_logo(describe_logo(mode)), Some(mode));
        }
    }

    #[test]
    fn parses_safe_operations() {
        assert_eq!(
            parse_request("fan auto"),
            Ok(Request::Operation(RequestedOperation::Fan(FanMode::Auto)))
        );
        assert_eq!(
            parse_request("  fan   manual  3000 "),
            Ok(Request::Operation(RequestedOperation::Fan(
                FanMode::Manual(3000)
            )))
        );
        assert_eq!(
            parse_request("bho 75"),
            Ok(Request::Operation(RequestedOperation::BatteryHealthLimit(
                75
            )))
        );
        assert_eq!(
            parse_request("bho off"),
            Ok(Request::Operation(RequestedOperation::BatteryHealthOff))
        );
    }

    #[test]
    fn parses_control_requests() {
        assert_eq!(parse_request("ping"), Ok(Request::Ping));
        assert_eq!(parse_request("status"), Ok(Request::Status));
        assert_eq!(parse_request("telemetry"), Ok(Request::Telemetry));
        assert_eq!(parse_request("sysinfo"), Ok(Request::SysInfo));
    }

    #[test]
    fn parses_automation_rules() {
        assert_eq!(
            parse_request("automation ac fan manual 4000"),
            Ok(Request::Automation {
                on_ac: true,
                fan: Some(FanMode::Manual(4000))
            })
        );
        assert_eq!(
            parse_request("automation battery fan auto"),
            Ok(Request::Automation {
                on_ac: false,
                fan: Some(FanMode::Auto)
            })
        );
        assert_eq!(
            parse_request("automation ac off"),
            Ok(Request::Automation {
                on_ac: true,
                fan: None
            })
        );
        assert!(parse_request("automation solar fan auto").is_err());
        assert!(parse_request("automation ac fan manual soon").is_err());
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_request("").is_err());
        assert!(parse_request("fan manual very-fast").is_err());
        assert!(parse_request("reboot now").is_err());
    }
}
