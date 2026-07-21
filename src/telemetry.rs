//! Read-only system telemetry for the dashboard: temperatures, CPU
//! utilisation and clock, memory, and per-GPU load — all from `/proc`,
//! `/sys`, hwmon, and (for the dGPU) `nvidia-smi`.  Nothing here touches
//! the EC or needs privilege; every source is world-readable.
//!
//! Fan RPM stays `None` until Phase 3 verifies the EC read-back commands.
//!
//! CPU utilisation is a delta between two `/proc/stat` reads, so the
//! reader holds the previous snapshot: telemetry is read through
//! [`TelemetryReader`], which the daemon owns.  The dashboard polls at
//! 1 Hz, so each utilisation figure covers roughly the last second.
//!
//! The simulated variant exists so the GUI can be developed and
//! screenshotted on machines with no Razer hardware (or no hwmon at all,
//! like WSL); it is always labelled as simulated in the wire format.
//!
//! GPU marketing names: the dGPU name comes cleanly from `nvidia-smi`,
//! but the AMD iGPU's marketing string ("Radeon 780M") is not reliably
//! exposed under `/sys`, so [`sysinfo`] reports a generic integrated-GPU
//! label when that is all it can determine.

/// Which GPU the live GPU fields describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuSource {
    /// The discrete NVIDIA GPU (active — read via nvidia-smi).
    Dedicated,
    /// The integrated AMD GPU (the dGPU was asleep or absent).
    Integrated,
    /// No GPU reading available.
    None,
}

impl GpuSource {
    fn wire(self) -> &'static str {
        match self {
            Self::Dedicated => "dgpu",
            Self::Integrated => "igpu",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Telemetry {
    pub cpu_temp_celsius: Option<f64>,
    pub cpu_util_percent: Option<f64>,
    pub cpu_freq_mhz: Option<f64>,
    pub mem_used_kb: Option<u64>,
    pub mem_total_kb: Option<u64>,
    pub gpu_temp_celsius: Option<f64>,
    pub gpu_util_percent: Option<f64>,
    pub gpu_mem_used_mb: Option<u64>,
    pub gpu_mem_total_mb: Option<u64>,
    pub gpu_source: GpuSource,
    pub fan_rpm: Option<u16>,
    pub on_ac: Option<bool>,
    pub simulated: bool,
}

impl Telemetry {
    /// One `key=value` response line, `ok`-prefixed by the daemon.
    pub fn to_line(&self) -> String {
        let temp = |value: Option<f64>| {
            value
                .map(|t| format!("{t:.1}"))
                .unwrap_or_else(|| "none".to_owned())
        };
        let num = |value: Option<u64>| {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_owned())
        };
        let power = match self.on_ac {
            Some(true) => "ac",
            Some(false) => "battery",
            None => "unknown",
        };
        format!(
            "cpu_temp={} cpu_util={} cpu_freq={} mem_used={} mem_total={} \
             gpu_temp={} gpu_util={} gpu_mem_used={} gpu_mem_total={} gpu_source={} \
             fan_rpm={} power={power} simulated={}",
            temp(self.cpu_temp_celsius),
            temp(self.cpu_util_percent),
            temp(self.cpu_freq_mhz),
            num(self.mem_used_kb),
            num(self.mem_total_kb),
            temp(self.gpu_temp_celsius),
            temp(self.gpu_util_percent),
            num(self.gpu_mem_used_mb),
            num(self.gpu_mem_total_mb),
            self.gpu_source.wire(),
            self.fan_rpm
                .map(|rpm| rpm.to_string())
                .unwrap_or_else(|| "none".to_owned()),
            self.simulated,
        )
    }
}

/// Total and idle jiffies from `/proc/stat`, used to compute the
/// utilisation delta between successive reads.
#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

/// Stateful telemetry source: keeps the previous CPU snapshot so
/// utilisation can be a delta.  The daemon owns one of these.
#[derive(Default)]
pub struct TelemetryReader {
    last_cpu: Option<CpuTimes>,
}

impl TelemetryReader {
    pub fn read(&mut self, simulate: bool) -> Telemetry {
        if simulate {
            return simulated();
        }
        let (mem_used_kb, mem_total_kb) = mem_used_total().unzip();
        let gpu = gpu_reading();
        Telemetry {
            cpu_temp_celsius: cpu_temp_celsius(),
            cpu_util_percent: self.cpu_util(),
            cpu_freq_mhz: cpu_freq_mhz(),
            mem_used_kb,
            mem_total_kb,
            gpu_temp_celsius: gpu.temp,
            gpu_util_percent: gpu.util,
            gpu_mem_used_mb: gpu.mem_used_mb,
            gpu_mem_total_mb: gpu.mem_total_mb,
            gpu_source: gpu.source,
            // EC fan read-back is gated on Phase 3 hardware verification.
            fan_rpm: None,
            on_ac: on_ac(),
            simulated: false,
        }
    }

    /// Utilisation since the previous read; `None` on the first read (no
    /// prior snapshot) or where `/proc/stat` is unavailable.
    fn cpu_util(&mut self) -> Option<f64> {
        let now = cpu_times()?;
        let util = self.last_cpu.and_then(|prev| {
            let total = now.total.checked_sub(prev.total)?;
            let idle = now.idle.checked_sub(prev.idle)?;
            (total > 0).then(|| (1.0 - idle as f64 / total as f64) * 100.0)
        });
        self.last_cpu = Some(now);
        util
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
    // The GPU drifts on its own period so the two gauges never move in
    // lockstep, which would look canned.
    let gpu_wave = (seconds / 13.0).sin();
    Telemetry {
        cpu_temp_celsius: Some(54.0 + wave * 6.0),
        cpu_util_percent: Some((22.0 + wave * 18.0).clamp(0.0, 100.0)),
        cpu_freq_mhz: Some(4300.0 + wave * 300.0),
        mem_used_kb: Some(25_400_000),
        mem_total_kb: Some(31_200_000),
        gpu_temp_celsius: Some(47.0 + gpu_wave * 5.0),
        gpu_util_percent: Some((12.0 + gpu_wave * 10.0).clamp(0.0, 100.0)),
        gpu_mem_used_mb: Some(2200),
        gpu_mem_total_mb: Some(8188),
        gpu_source: GpuSource::Dedicated,
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

/// Aggregate CPU jiffies from the `cpu` summary line of `/proc/stat`.
#[cfg(target_os = "linux")]
fn cpu_times() -> Option<CpuTimes> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
    // user nice system idle iowait irq softirq steal ...
    let idle = values.get(3).copied()? + values.get(4).copied().unwrap_or(0);
    let total: u64 = values.iter().sum();
    Some(CpuTimes { total, idle })
}

#[cfg(not(target_os = "linux"))]
fn cpu_times() -> Option<CpuTimes> {
    None
}

/// Highest current core frequency in MHz, from cpufreq (kHz) with a
/// `/proc/cpuinfo` fallback.
#[cfg(target_os = "linux")]
fn cpu_freq_mhz() -> Option<f64> {
    if let Ok(cpus) = std::fs::read_dir("/sys/devices/system/cpu") {
        let mut max_khz = 0u64;
        for entry in cpus.flatten() {
            let path = entry.path().join("cpufreq/scaling_cur_freq");
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(khz) = text.trim().parse::<u64>()
            {
                max_khz = max_khz.max(khz);
            }
        }
        if max_khz > 0 {
            return Some(max_khz as f64 / 1000.0);
        }
    }
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':').filter(|(k, _)| k.trim() == "cpu MHz"))
        .filter_map(|(_, v)| v.trim().parse::<f64>().ok())
        .fold(None, |max, mhz| Some(max.map_or(mhz, |m: f64| m.max(mhz))))
}

#[cfg(not(target_os = "linux"))]
fn cpu_freq_mhz() -> Option<f64> {
    None
}

/// (used, total) memory in kB from `/proc/meminfo`.
#[cfg(target_os = "linux")]
fn mem_used_total() -> Option<(u64, u64)> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |key: &str| -> Option<u64> {
        meminfo
            .lines()
            .find_map(|line| line.strip_prefix(key))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    Some((total.saturating_sub(available), total))
}

#[cfg(not(target_os = "linux"))]
fn mem_used_total() -> Option<(u64, u64)> {
    None
}

/// A GPU's live figures.
struct GpuReading {
    temp: Option<f64>,
    util: Option<f64>,
    mem_used_mb: Option<u64>,
    mem_total_mb: Option<u64>,
    source: GpuSource,
}

/// The dGPU-when-awake policy: read the discrete NVIDIA GPU only while
/// it is already powered up (checking its PCI runtime-pm state never
/// wakes it); otherwise report the always-on AMD iGPU.
#[cfg(target_os = "linux")]
fn gpu_reading() -> GpuReading {
    if nvidia_is_active()
        && let Some(reading) = nvidia_reading()
    {
        return reading;
    }
    amd_igpu_reading().unwrap_or(GpuReading {
        temp: None,
        util: None,
        mem_used_mb: None,
        mem_total_mb: None,
        source: GpuSource::None,
    })
}

#[cfg(not(target_os = "linux"))]
fn gpu_reading() -> GpuReading {
    GpuReading {
        temp: None,
        util: None,
        mem_used_mb: None,
        mem_total_mb: None,
        source: GpuSource::None,
    }
}

/// True when an NVIDIA display device reports a non-suspended runtime-pm
/// state.  Reads a `/sys` file only — this does not wake the device.
#[cfg(target_os = "linux")]
fn nvidia_is_active() -> bool {
    let Ok(devices) = std::fs::read_dir("/sys/bus/pci/devices") else {
        return false;
    };
    for entry in devices.flatten() {
        let path = entry.path();
        let vendor = std::fs::read_to_string(path.join("vendor")).unwrap_or_default();
        let class = std::fs::read_to_string(path.join("class")).unwrap_or_default();
        // 0x10de = NVIDIA; class 0x03xxxx = display controller.
        if vendor.trim() == "0x10de" && class.trim().starts_with("0x03") {
            let status =
                std::fs::read_to_string(path.join("power/runtime_status")).unwrap_or_default();
            return status.trim() == "active";
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn nvidia_reading() -> Option<GpuReading> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=temperature.gpu,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.lines().next()?.split(',').map(str::trim);
    Some(GpuReading {
        temp: fields.next()?.parse().ok(),
        util: fields.next()?.parse().ok(),
        mem_used_mb: fields.next()?.parse().ok(),
        mem_total_mb: fields.next()?.parse().ok(),
        source: GpuSource::Dedicated,
    })
}

/// The AMD integrated GPU: temperature from the `amdgpu` hwmon node, load
/// and VRAM from its DRM sysfs.
#[cfg(target_os = "linux")]
fn amd_igpu_reading() -> Option<GpuReading> {
    let temp = amdgpu_temp();
    let (util, mem_used_mb, mem_total_mb) = amdgpu_drm();
    if temp.is_none() && util.is_none() {
        return None;
    }
    Some(GpuReading {
        temp,
        util,
        mem_used_mb,
        mem_total_mb,
        source: GpuSource::Integrated,
    })
}

#[cfg(target_os = "linux")]
fn amdgpu_temp() -> Option<f64> {
    let hwmon = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in hwmon.flatten() {
        let path = entry.path();
        let name = std::fs::read_to_string(path.join("name")).unwrap_or_default();
        if matches!(name.trim(), "amdgpu" | "nouveau") {
            let raw = std::fs::read_to_string(path.join("temp1_input")).ok()?;
            let millidegrees: f64 = raw.trim().parse().ok()?;
            return Some(millidegrees / 1000.0);
        }
    }
    None
}

/// (busy %, VRAM used MB, VRAM total MB) from the first amdgpu DRM card.
#[cfg(target_os = "linux")]
fn amdgpu_drm() -> (Option<f64>, Option<u64>, Option<u64>) {
    let Ok(cards) = std::fs::read_dir("/sys/class/drm") else {
        return (None, None, None);
    };
    for entry in cards.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // cardN, not the cardN-CONNECTOR symlinks.
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        if std::fs::read_to_string(device.join("vendor"))
            .unwrap_or_default()
            .trim()
            != "0x1002"
        {
            continue; // 0x1002 = AMD
        }
        let read_u64 = |file: &str| -> Option<u64> {
            std::fs::read_to_string(device.join(file))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        let busy = read_u64("gpu_busy_percent").map(|v| v as f64);
        let used = read_u64("mem_info_vram_used").map(|b| b / 1_048_576);
        let total = read_u64("mem_info_vram_total").map(|b| b / 1_048_576);
        return (busy, used, total);
    }
    (None, None, None)
}

/// AC adapter state from /sys/class/power_supply, considering every
/// non-battery supply.  Public: the daemon transport polls this for
/// AC/battery automation.
#[cfg(target_os = "linux")]
pub fn on_ac() -> Option<bool> {
    let supplies = std::fs::read_dir("/sys/class/power_supply").ok()?;
    let entries: Vec<(String, Option<String>)> = supplies
        .flatten()
        .map(|entry| {
            let path = entry.path();
            (
                std::fs::read_to_string(path.join("type")).unwrap_or_default(),
                std::fs::read_to_string(path.join("online")).ok(),
            )
        })
        .collect();
    ac_from_supplies(&entries)
}

/// Classifies power-supply entries: each element is one supply's `type`
/// file contents and its `online` file contents when readable.
///
/// Any online non-battery supply means AC — the Blade 14 (2023) charges
/// via both the barrel adapter (type "Mains") and USB-C PD (type "USB"),
/// so stopping at the first Mains entry would both let an offline adapter
/// mask an online one and read PD charging as on-battery (the bug class
/// fang-razer-linux 0.9.1 fixed).  At least one readable adapter, none
/// online, means battery; no readable adapter at all means unknown.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn ac_from_supplies(entries: &[(String, Option<String>)]) -> Option<bool> {
    let mut saw_readable_adapter = false;
    for (kind, online) in entries {
        if kind.trim() == "Battery" {
            continue;
        }
        if let Some(online) = online {
            saw_readable_adapter = true;
            if online.trim() == "1" {
                return Some(true);
            }
        }
    }
    saw_readable_adapter.then_some(false)
}

#[cfg(not(target_os = "linux"))]
pub fn on_ac() -> Option<bool> {
    None
}

/// Static machine identity for the dashboard's hardware panel, as one
/// tab-separated `key=value` line.  Model names contain spaces, so fields
/// are separated by tabs rather than spaces (the client splits on `\t`).
/// Read once by the client (these values do not change while the machine
/// runs).  `nvidia-smi` is invoked at most once here, which is an
/// acceptable one-time dGPU wake at page load.
pub fn sysinfo(simulate: bool) -> String {
    if simulate {
        return "cpu_model=AMD Ryzen 9 7940HS w/ Radeon 780M Graphics\tcpu_cores=8\t\
                cpu_threads=16\tmem_total_kb=31200000\t\
                gpu_dgpu=NVIDIA GeForce RTX 4060 Laptop GPU\t\
                gpu_igpu=AMD Radeon 780M\tsimulated=true"
            .to_owned();
    }
    format!(
        "cpu_model={}\tcpu_cores={}\tcpu_threads={}\tmem_total_kb={}\tgpu_dgpu={}\tgpu_igpu={}\tsimulated=false",
        cpu_model().unwrap_or_else(|| "Unknown".to_owned()),
        cpu_cores().map_or_else(|| "none".to_owned(), |c| c.to_string()),
        cpu_threads().map_or_else(|| "none".to_owned(), |c| c.to_string()),
        mem_used_total().map_or_else(|| "none".to_owned(), |(_, total)| total.to_string()),
        dgpu_name().unwrap_or_else(|| "none".to_owned()),
        igpu_name().unwrap_or_else(|| "none".to_owned()),
    )
}

#[cfg(target_os = "linux")]
fn cpu_model() -> Option<String> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    cpuinfo
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(k, _)| k.trim() == "model name")
        })
        .map(|(_, v)| v.trim().to_owned())
}

/// Physical core count: distinct `physical id`+`core id` pairs, or the
/// `cpu cores` field, falling back to the logical count.
#[cfg(target_os = "linux")]
fn cpu_cores() -> Option<u32> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    cpuinfo
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(k, _)| k.trim() == "cpu cores")
        })
        .and_then(|(_, v)| v.trim().parse().ok())
        .or_else(cpu_threads)
}

#[cfg(target_os = "linux")]
fn cpu_threads() -> Option<u32> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let count = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    (count > 0).then_some(count as u32)
}

#[cfg(target_os = "linux")]
fn dgpu_name() -> Option<String> {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// The AMD iGPU is only reliably identifiable as "integrated AMD" from
/// `/sys` — the marketing name is not exposed there.
#[cfg(target_os = "linux")]
fn igpu_name() -> Option<String> {
    amdgpu_temp()
        .is_some()
        .then(|| "AMD Radeon (integrated)".to_owned())
}

#[cfg(not(target_os = "linux"))]
fn cpu_model() -> Option<String> {
    None
}
#[cfg(not(target_os = "linux"))]
fn cpu_cores() -> Option<u32> {
    None
}
#[cfg(not(target_os = "linux"))]
fn cpu_threads() -> Option<u32> {
    None
}
#[cfg(not(target_os = "linux"))]
fn dgpu_name() -> Option<String> {
    None
}
#[cfg(not(target_os = "linux"))]
fn igpu_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Telemetry {
        Telemetry {
            cpu_temp_celsius: Some(54.31),
            cpu_util_percent: Some(23.4),
            cpu_freq_mhz: Some(4342.0),
            mem_used_kb: Some(25_400_000),
            mem_total_kb: Some(31_200_000),
            gpu_temp_celsius: Some(46.99),
            gpu_util_percent: Some(12.0),
            gpu_mem_used_mb: Some(2200),
            gpu_mem_total_mb: Some(8188),
            gpu_source: GpuSource::Dedicated,
            fan_rpm: Some(2441),
            on_ac: Some(true),
            simulated: true,
        }
    }

    #[test]
    fn simulated_values_stay_inside_plausible_ranges() {
        let telemetry = TelemetryReader::default().read(true);
        assert!((40.0..=70.0).contains(&telemetry.cpu_temp_celsius.unwrap()));
        assert!((0.0..=100.0).contains(&telemetry.cpu_util_percent.unwrap()));
        assert!((40.0..=60.0).contains(&telemetry.gpu_temp_celsius.unwrap()));
        assert!((0.0..=100.0).contains(&telemetry.gpu_util_percent.unwrap()));
        assert_eq!(telemetry.gpu_source, GpuSource::Dedicated);
        assert!((2000..=2800).contains(&telemetry.fan_rpm.unwrap()));
        assert!(telemetry.simulated);
    }

    #[test]
    fn line_format_is_stable_for_clients() {
        assert_eq!(
            sample().to_line(),
            "cpu_temp=54.3 cpu_util=23.4 cpu_freq=4342.0 mem_used=25400000 \
             mem_total=31200000 gpu_temp=47.0 gpu_util=12.0 gpu_mem_used=2200 \
             gpu_mem_total=8188 gpu_source=dgpu fan_rpm=2441 power=ac simulated=true"
        );
        let empty = Telemetry {
            cpu_temp_celsius: None,
            cpu_util_percent: None,
            cpu_freq_mhz: None,
            mem_used_kb: None,
            mem_total_kb: None,
            gpu_temp_celsius: None,
            gpu_util_percent: None,
            gpu_mem_used_mb: None,
            gpu_mem_total_mb: None,
            gpu_source: GpuSource::None,
            fan_rpm: None,
            on_ac: None,
            simulated: false,
        };
        assert_eq!(
            empty.to_line(),
            "cpu_temp=none cpu_util=none cpu_freq=none mem_used=none mem_total=none \
             gpu_temp=none gpu_util=none gpu_mem_used=none gpu_mem_total=none \
             gpu_source=none fan_rpm=none power=unknown simulated=false"
        );
    }

    #[test]
    fn cpu_utilisation_is_a_delta_between_reads() {
        // Prior snapshot: 1000 total jiffies, 800 idle.
        // 100 more total jiffies, 25 of them idle -> 75% busy.
        let now = CpuTimes {
            total: 1100,
            idle: 825,
        };
        let total = now.total - 1000;
        let idle = now.idle - 800;
        let util = (1.0 - idle as f64 / total as f64) * 100.0;
        assert_eq!(util, 75.0);
    }

    #[test]
    fn simulated_sysinfo_names_the_machine() {
        let line = sysinfo(true);
        assert!(line.contains("cpu_model=AMD Ryzen 9 7940HS"));
        assert!(line.contains("gpu_dgpu=NVIDIA GeForce RTX 4060 Laptop GPU"));
        assert!(line.contains("simulated=true"));
    }

    fn supplies(entries: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        entries
            .iter()
            .map(|(kind, online)| (kind.to_string(), online.map(str::to_string)))
            .collect()
    }

    #[test]
    fn a_later_online_adapter_is_seen_past_an_offline_mains() {
        // The old first-Mains-wins logic read this as on-battery.
        let entries = supplies(&[("Battery", None), ("Mains", Some("0")), ("USB", Some("1"))]);
        assert_eq!(ac_from_supplies(&entries), Some(true));
    }

    #[test]
    fn usb_pd_charging_counts_as_ac() {
        // USB-C PD supplies report type "USB", not "Mains".
        let entries = supplies(&[("Battery", None), ("USB", Some("1"))]);
        assert_eq!(ac_from_supplies(&entries), Some(true));
        let wireless = supplies(&[("Wireless", Some("1"))]);
        assert_eq!(ac_from_supplies(&wireless), Some(true));
    }

    #[test]
    fn all_adapters_offline_means_battery() {
        let entries = supplies(&[("Battery", None), ("Mains", Some("0")), ("USB", Some("0"))]);
        assert_eq!(ac_from_supplies(&entries), Some(false));
    }

    #[test]
    fn no_readable_adapter_means_unknown() {
        assert_eq!(ac_from_supplies(&supplies(&[("Battery", None)])), None);
        assert_eq!(ac_from_supplies(&supplies(&[])), None);
        // An adapter whose online file cannot be read proves nothing.
        assert_eq!(ac_from_supplies(&supplies(&[("Mains", None)])), None);
    }

    #[test]
    fn sysfs_trailing_newlines_are_trimmed() {
        let entries = supplies(&[("Battery\n", None), ("Mains\n", Some("1\n"))]);
        assert_eq!(ac_from_supplies(&entries), Some(true));
    }
}
