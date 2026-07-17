use std::{env, process::ExitCode};

use razer_control_secureblue::{
    DeviceId, backend::BackendChoice, blade_14_2023_udev_rule, find_device, ipc, validate_operation,
};

fn usage() {
    eprintln!(
        "Usage:\n  razer-control device <vendor-hex> <product-hex>\n  razer-control validate fan auto\n  razer-control validate fan manual <rpm>\n  razer-control validate bho <50-80>\n  razer-control validate profile <silent|balanced|gaming> [--experimental]\n  razer-control validate profile custom cpu <level> gpu <level> [--experimental]\n  razer-control probe   (read-only EC state dump; hidraw builds only)\n  razer-control daemon [--experimental] [--backend dry-run|hidraw]\n  razer-control ctl <request...>   (e.g. ctl fan manual 3000, ctl status)\n  razer-control udev-rule"
    );
}

fn parse_hex(value: &str) -> Result<u16, String> {
    u16::from_str_radix(value.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{value:?} is not a valid hexadecimal USB ID"))
}

fn parse_device(args: &[String]) -> Result<DeviceId, String> {
    if args.len() != 2 {
        return Err("expected <vendor-hex> <product-hex>".into());
    }
    Ok(DeviceId {
        vendor_id: parse_hex(&args[0])?,
        product_id: parse_hex(&args[1])?,
    })
}

fn current_supported_device() -> DeviceId {
    DeviceId {
        vendor_id: 0x1532,
        product_id: 0x029d,
    }
}

#[cfg(unix)]
fn run_daemon(experimental: bool, backend: BackendChoice) -> Result<String, String> {
    razer_control_secureblue::daemon_unix::run(experimental, backend)
        .map(|()| "daemon exited cleanly".to_owned())
}

#[cfg(not(unix))]
fn run_daemon(_experimental: bool, _backend: BackendChoice) -> Result<String, String> {
    Err("the daemon requires Linux (hidraw and systemd)".to_owned())
}

/// `--backend <name>`, defaulting to the dry run: hardware access is opt-in
/// per invocation, never ambient.
fn parse_backend_flag(rest: &[String]) -> Result<BackendChoice, String> {
    match rest.iter().position(|argument| argument == "--backend") {
        None => Ok(BackendChoice::DryRun),
        Some(index) => rest
            .get(index + 1)
            .ok_or_else(|| "--backend requires a value: dry-run or hidraw".to_owned())
            .and_then(|value| BackendChoice::parse(value)),
    }
}

/// Read-only EC state dump for Phase 3 verification: power mode, boost
/// levels, fan setpoints, and BHO, via the 0x8x query commands.  Nothing
/// here writes to the EC.
#[cfg(feature = "hidraw-backend")]
fn run_probe() -> Result<String, String> {
    use razer_control_secureblue::backend_hidraw::HidrawBackend;
    use razer_control_secureblue::protocol::{self, RESPONSE_ARGS_OFFSET, Zone};
    let backend = HidrawBackend::open(current_supported_device())?;
    let arg =
        |report: &[u8; protocol::REPORT_LEN], index: usize| report[RESPONSE_ARGS_OFFSET + index];
    let mut out = String::new();
    for (name, zone) in [("cpu", Zone::Cpu), ("gpu", Zone::Gpu)] {
        let mode = backend.query(&protocol::get_power_mode(zone))?;
        let boost = backend.query(&protocol::get_boost(zone))?;
        let setpoint = backend.query(&protocol::get_fan_setpoint(zone))?;
        out.push_str(&format!(
            "{name}: mode={} manual_fan={} boost={} fan_setpoint_rpm={}\n",
            arg(&mode, 2),
            arg(&mode, 3),
            arg(&boost, 2),
            arg(&setpoint, 2) as u16 * 100,
        ));
    }
    let bho = backend.query(&protocol::get_battery_health())?;
    let (enabled, threshold) = protocol::decode_battery_health(arg(&bho, 0));
    out.push_str(&format!("bho: enabled={enabled} threshold={threshold}%"));
    Ok(out)
}

#[cfg(not(feature = "hidraw-backend"))]
fn run_probe() -> Result<String, String> {
    Err("probe requires a build with the hidraw-backend feature".to_owned())
}

#[cfg(unix)]
fn send_to_daemon(command: &str) -> Result<String, String> {
    razer_control_secureblue::daemon_unix::send(command)
}

#[cfg(not(unix))]
fn send_to_daemon(_command: &str) -> Result<String, String> {
    Err("the daemon client requires Linux".to_owned())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, vendor, product] if command == "device" => {
            parse_device(&[vendor.clone(), product.clone()]).map(|id| match find_device(id) {
                Some(device) => format!(
                    "supported: {} ({:04x}:{:04x}); fan {}–{} RPM; BHO={}; boost={}",
                    device.name,
                    id.vendor_id,
                    id.product_id,
                    device.fan_range.min_rpm,
                    device.fan_range.max_rpm,
                    device.supports_battery_health_optimizer,
                    device.supports_boost
                ),
                None => format!("unsupported: {:04x}:{:04x}", id.vendor_id, id.product_id),
            })
        }
        [command] if command == "udev-rule" => Ok(blade_14_2023_udev_rule().to_owned()),
        [command] if command == "probe" => run_probe(),
        [command, rest @ ..] if command == "validate" && !rest.is_empty() => {
            let experimental = rest.iter().any(|argument| argument == "--experimental");
            let tokens: Vec<&str> = rest
                .iter()
                .map(String::as_str)
                .filter(|argument| *argument != "--experimental")
                .collect();
            ipc::parse_operation(&tokens)
                .and_then(|operation| {
                    validate_operation(current_supported_device(), operation, experimental)
                        .map_err(|error| error.to_string())
                })
                .map(|_| "accepted by safety policy; no hardware command was sent".to_owned())
        }
        [command, rest @ ..] if command == "daemon" => {
            let experimental = rest.iter().any(|argument| argument == "--experimental");
            parse_backend_flag(rest).and_then(|backend| run_daemon(experimental, backend))
        }
        [command, rest @ ..] if command == "ctl" && !rest.is_empty() => {
            send_to_daemon(&rest.join(" "))
        }
        _ => {
            usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rejected: {error}");
            ExitCode::from(1)
        }
    }
}
