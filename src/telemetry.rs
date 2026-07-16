//! Read-only system telemetry for the dashboard: CPU temperature from
//! hwmon, AC/battery state from the power-supply class.  Fan RPM stays
//! `None` until Phase 3 verifies the EC read-back commands on hardware —
//! this module never talks to the EC.
//!
//! The simulated variant exists so the GUI can be developed and
//! screenshotted on machines with no Razer hardware (or no hwmon at all,
//! like WSL); it is always labelled as simulated in the wire format.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Telemetry {
    pub cpu_temp_celsius: Option<f64>,
    pub fan_rpm: Option<u16>,
    pub on_ac: Option<bool>,
    pub simulated: bool,
}

impl Telemetry {
    /// One `key=value` response line, `ok`-prefixed by the daemon.
    pub fn to_line(&self) -> String {
        let cpu = self
            .cpu_temp_celsius
            .map(|t| format!("{t:.1}"))
            .unwrap_or_else(|| "none".to_owned());
        let fan = self
            .fan_rpm
            .map(|rpm| rpm.to_string())
            .unwrap_or_else(|| "none".to_owned());
        let power = match self.on_ac {
            Some(true) => "ac",
            Some(false) => "battery",
            None => "unknown",
        };
        format!(
            "cpu_temp={cpu} fan_rpm={fan} power={power} simulated={}",
            self.simulated
        )
    }
}

pub fn read(simulate: bool) -> Telemetry {
    if simulate {
        return simulated();
    }
    Telemetry {
        cpu_temp_celsius: cpu_temp_celsius(),
        // EC fan read-back is gated on Phase 3 hardware verification.
        fan_rpm: None,
        on_ac: on_ac(),
        simulated: false,
    }
}

/// Plausible values that drift over time, so charts and gauges can be
/// exercised without hardware.
fn simulated() -> Telemetry {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0);
    let wave = (seconds / 9.0).sin();
    Telemetry {
        cpu_temp_celsius: Some(54.0 + wave * 6.0),
        fan_rpm: Some((2400.0 + wave * 350.0) as u16),
        on_ac: Some(true),
        simulated: true,
    }
}

/// First plausible CPU package temperature from /sys/class/hwmon.
/// k10temp is the AMD driver (Blade 14 2023); coretemp covers Intel models.
#[cfg(target_os = "linux")]
fn cpu_temp_celsius() -> Option<f64> {
    let hwmon = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in hwmon.flatten() {
        let path = entry.path();
        let name = std::fs::read_to_string(path.join("name")).unwrap_or_default();
        if matches!(name.trim(), "k10temp" | "coretemp" | "zenpower") {
            let raw = std::fs::read_to_string(path.join("temp1_input")).ok()?;
            let millidegrees: f64 = raw.trim().parse().ok()?;
            return Some(millidegrees / 1000.0);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn cpu_temp_celsius() -> Option<f64> {
    None
}

/// AC adapter state from /sys/class/power_supply (type == "Mains").
#[cfg(target_os = "linux")]
fn on_ac() -> Option<bool> {
    let supplies = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in supplies.flatten() {
        let path = entry.path();
        let kind = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim() == "Mains" {
            let online = std::fs::read_to_string(path.join("online")).ok()?;
            return Some(online.trim() == "1");
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn on_ac() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_values_stay_inside_plausible_ranges() {
        let telemetry = read(true);
        let cpu = telemetry.cpu_temp_celsius.unwrap();
        assert!((40.0..=70.0).contains(&cpu));
        let rpm = telemetry.fan_rpm.unwrap();
        assert!((2000..=2800).contains(&rpm));
        assert!(telemetry.simulated);
    }

    #[test]
    fn line_format_is_stable_for_clients() {
        let telemetry = Telemetry {
            cpu_temp_celsius: Some(54.31),
            fan_rpm: Some(2441),
            on_ac: Some(true),
            simulated: true,
        };
        assert_eq!(
            telemetry.to_line(),
            "cpu_temp=54.3 fan_rpm=2441 power=ac simulated=true"
        );
        let empty = Telemetry {
            cpu_temp_celsius: None,
            fan_rpm: None,
            on_ac: None,
            simulated: false,
        };
        assert_eq!(
            empty.to_line(),
            "cpu_temp=none fan_rpm=none power=unknown simulated=false"
        );
    }
}
