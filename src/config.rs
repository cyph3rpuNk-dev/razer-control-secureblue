//! Persisted daemon state: the last applied fan and battery settings plus
//! the AC/battery automation choices.  Stored as `key=value` lines (the
//! same shape as the IPC protocol) under the user's XDG config directory —
//! no extra dependencies, trivially inspectable.

use crate::FanMode;

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
