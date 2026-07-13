//! Line-oriented IPC protocol shared by the daemon, the `ctl` client, and the
//! `validate` CLI.  One request per line; the daemon answers with a single
//! line starting with `ok` or `err`.

use crate::{FanMode, RequestedOperation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Ping,
    Status,
    Operation(RequestedOperation),
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.as_slice() {
        ["ping"] => Ok(Request::Ping),
        ["status"] => Ok(Request::Status),
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
    }

    #[test]
    fn parses_control_requests() {
        assert_eq!(parse_request("ping"), Ok(Request::Ping));
        assert_eq!(parse_request("status"), Ok(Request::Status));
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_request("").is_err());
        assert!(parse_request("fan manual very-fast").is_err());
        assert!(parse_request("reboot now").is_err());
    }
}
