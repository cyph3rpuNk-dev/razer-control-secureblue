//! Line-oriented IPC protocol shared by the daemon, the `ctl` client, and the
//! `validate` CLI.  One request per line; the daemon answers with a single
//! line starting with `ok` or `err`.

use crate::{FanMode, RequestedOperation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Ping,
    Status,
    Telemetry,
    /// Configure the fan choice applied when the power source changes;
    /// `None` clears the rule for that source.
    Automation {
        on_ac: bool,
        fan: Option<FanMode>,
    },
    Operation(RequestedOperation),
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.as_slice() {
        ["ping"] => Ok(Request::Ping),
        ["status"] => Ok(Request::Status),
        ["telemetry"] => Ok(Request::Telemetry),
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
        ["boost"] => Ok(RequestedOperation::Boost),
        ["gpu-tdp", watts] => watts
            .parse::<u16>()
            .map(RequestedOperation::GpuTdpWatts)
            .map_err(|_| "GPU TDP must be an integer".to_owned()),
        _ => Err(format!("unknown request {:?}", tokens.join(" "))),
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
