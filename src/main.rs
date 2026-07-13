use std::{env, process::ExitCode};

use razer_control_secureblue::{
    DeviceId, FanMode, RequestedOperation, blade_14_2023_udev_rule, find_device, validate_operation,
};

fn usage() {
    eprintln!(
        "Usage:\n  razer-control device <vendor-hex> <product-hex>\n  razer-control validate fan auto\n  razer-control validate fan manual <rpm>\n  razer-control validate bho <50-80>\n  razer-control validate boost [--experimental]\n  razer-control validate gpu-tdp <watts> [--experimental]\n  razer-control udev-rule"
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
        [command, feature, rest @ ..] if command == "validate" => {
            let experimental = rest.iter().any(|argument| argument == "--experimental");
            let operation = match (feature.as_str(), rest) {
                ("fan", [mode]) if mode == "auto" => Ok(RequestedOperation::Fan(FanMode::Auto)),
                ("fan", [mode, rpm]) if mode == "manual" => rpm
                    .parse::<u16>()
                    .map(FanMode::Manual)
                    .map(RequestedOperation::Fan)
                    .map_err(|_| "fan RPM must be an integer".to_owned()),
                ("bho", [limit]) => limit
                    .parse::<u8>()
                    .map(RequestedOperation::BatteryHealthLimit)
                    .map_err(|_| "battery health limit must be an integer".to_owned()),
                ("boost", []) => Ok(RequestedOperation::Boost),
                ("boost", [flag]) if flag == "--experimental" => Ok(RequestedOperation::Boost),
                ("gpu-tdp", [watts]) => watts
                    .parse::<u16>()
                    .map(RequestedOperation::GpuTdpWatts)
                    .map_err(|_| "GPU TDP must be an integer".to_owned()),
                ("gpu-tdp", [watts, flag]) if flag == "--experimental" => watts
                    .parse::<u16>()
                    .map(RequestedOperation::GpuTdpWatts)
                    .map_err(|_| "GPU TDP must be an integer".to_owned()),
                _ => Err("unknown validation request".to_owned()),
            };
            operation
                .and_then(|operation| {
                    validate_operation(current_supported_device(), operation, experimental)
                        .map_err(|error| error.to_string())
                })
                .map(|_| "accepted by safety policy; no hardware command was sent".to_owned())
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
