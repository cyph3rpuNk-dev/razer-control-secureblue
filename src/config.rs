//! Persisted daemon state: the last applied fan and battery settings plus
//! the AC/battery automation choices.  Stored as `key=value` lines (the
//! same shape as the IPC protocol) under the user's XDG config directory —
//! no extra dependencies, trivially inspectable.

use crate::ipc::{
    describe_effect, describe_logo, describe_profile, parse_effect, parse_logo, parse_profile,
};
use crate::{FanMode, LightingEffect, LogoMode, Profile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryHealth {
    #[default]
    Unset,
    Off,
    Limit(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PersistedState {
    pub fan: Option<FanMode>,
    pub battery_health: BatteryHealth,
    pub fan_on_ac: Option<FanMode>,
    pub fan_on_battery: Option<FanMode>,
    pub profile: Option<Profile>,
    pub kbd_brightness: Option<u8>,
    pub kbd_effect: Option<LightingEffect>,
    pub logo: Option<LogoMode>,
    pub kbd_on_ac: Option<u8>,
    pub kbd_on_battery: Option<u8>,
}

fn render_fan(mode: FanMode) -> String {
    match mode {
        FanMode::Auto => "auto".to_owned(),
        FanMode::Manual(rpm) => format!("manual:{rpm}"),
    }
}

fn parse_fan(value: &str) -> Option<FanMode> {
    if value == "auto" {
        return Some(FanMode::Auto);
    }
    value
        .strip_prefix("manual:")
        .and_then(|rpm| rpm.parse().ok())
        .map(FanMode::Manual)
}

impl PersistedState {
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        if let Some(fan) = self.fan {
            lines.push(format!("fan={}", render_fan(fan)));
        }
        match self.battery_health {
            BatteryHealth::Unset => {}
            BatteryHealth::Off => lines.push("bho=off".to_owned()),
            BatteryHealth::Limit(limit) => lines.push(format!("bho={limit}")),
        }
        if let Some(fan) = self.fan_on_ac {
            lines.push(format!("fan_on_ac={}", render_fan(fan)));
        }
        if let Some(fan) = self.fan_on_battery {
            lines.push(format!("fan_on_battery={}", render_fan(fan)));
        }
        if let Some(profile) = self.profile {
            lines.push(format!("profile={}", describe_profile(profile)));
        }
        if let Some(brightness) = self.kbd_brightness {
            lines.push(format!("kbd_brightness={brightness}"));
        }
        if let Some(effect) = self.kbd_effect {
            lines.push(format!("kbd_effect={}", describe_effect(effect)));
        }
        if let Some(logo) = self.logo {
            lines.push(format!("logo={}", describe_logo(logo)));
        }
        if let Some(brightness) = self.kbd_on_ac {
            lines.push(format!("kbd_on_ac={brightness}"));
        }
        if let Some(brightness) = self.kbd_on_battery {
            lines.push(format!("kbd_on_battery={brightness}"));
        }
        lines.join("\n") + "\n"
    }

    /// Unknown keys and malformed values are skipped, never fatal: a stale
    /// or hand-edited file must not stop the daemon.
    pub fn parse(text: &str) -> PersistedState {
        let mut state = PersistedState::default();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            match key {
                "fan" => state.fan = parse_fan(value),
                "bho" => {
                    state.battery_health = if value == "off" {
                        BatteryHealth::Off
                    } else if let Ok(limit) = value.parse() {
                        BatteryHealth::Limit(limit)
                    } else {
                        BatteryHealth::Unset
                    }
                }
                "fan_on_ac" => state.fan_on_ac = parse_fan(value),
                "fan_on_battery" => state.fan_on_battery = parse_fan(value),
                "profile" => state.profile = parse_profile(value),
                "kbd_brightness" => state.kbd_brightness = value.parse().ok(),
                "kbd_effect" => state.kbd_effect = parse_effect(value),
                "logo" => state.logo = parse_logo(value),
                "kbd_on_ac" => state.kbd_on_ac = value.parse().ok(),
                "kbd_on_battery" => state.kbd_on_battery = value.parse().ok(),
                _ => {}
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_full_state() {
        let state = PersistedState {
            fan: Some(FanMode::Manual(3500)),
            battery_health: BatteryHealth::Limit(80),
            fan_on_ac: Some(FanMode::Manual(4000)),
            fan_on_battery: Some(FanMode::Auto),
            profile: Some(Profile::Custom {
                cpu: crate::BoostLevel::Boost,
                gpu: crate::BoostLevel::High,
            }),
            kbd_brightness: Some(60),
            kbd_effect: Some(LightingEffect::Static {
                red: 0x44,
                green: 0xd6,
                blue: 0x2c,
            }),
            logo: Some(LogoMode::Breathing),
            kbd_on_ac: Some(80),
            kbd_on_battery: Some(20),
        };
        assert_eq!(PersistedState::parse(&state.render()), state);
    }

    #[test]
    fn tolerates_garbage_and_partial_files() {
        let state = PersistedState::parse("fan=manual:oops\nnonsense\nbho=75\nfuture_key=1\n");
        assert_eq!(state.fan, None);
        assert_eq!(state.battery_health, BatteryHealth::Limit(75));
        assert_eq!(PersistedState::parse(""), PersistedState::default());
    }
}
