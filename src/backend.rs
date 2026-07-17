//! Hardware backends.  The daemon core only talks to this trait.  The
//! default implementation is a dry run that records what it would have sent;
//! the real hidraw backend (`backend_hidraw`, behind the `hidraw-backend`
//! feature) exists now that the protocol layer is pinned by golden-byte
//! tests, and must be selected explicitly at runtime.

use crate::{DeviceId, EcContext, RequestedOperation};

/// Which backend the daemon drives, chosen on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendChoice {
    DryRun,
    Hidraw,
}

impl BackendChoice {
    pub fn parse(name: &str) -> Result<BackendChoice, String> {
        match name {
            "dry-run" => Ok(Self::DryRun),
            "hidraw" => Ok(Self::Hidraw),
            other => Err(format!(
                "unknown backend {other:?}; expected \"dry-run\" or \"hidraw\""
            )),
        }
    }
}

/// The facts about one enumerated HID interface that the laptop-vs-peripheral
/// decision needs.  Mirrors hidapi's `DeviceInfo` without depending on it so
/// the selection logic stays unit-testable on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidCandidate {
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface_number: i32,
    pub usage_page: u16,
}

/// Picks the EC interface for `expected` from an enumeration snapshot.
///
/// Heuristic from fang-razer-linux (GPL-2.0): the exact product on
/// interface 0 wins; otherwise the exact product on a vendor-defined usage
/// page (>= 0xff00).  Anything else — another PID, a plugged-in Razer mouse
/// or keyboard, a generic-desktop collection — is never selected, so a
/// peripheral can never be mistaken for the laptop's EC.
pub fn select_hid_candidate(expected: DeviceId, candidates: &[HidCandidate]) -> Option<usize> {
    let is_expected = |candidate: &HidCandidate| {
        candidate.vendor_id == expected.vendor_id && candidate.product_id == expected.product_id
    };
    candidates
        .iter()
        .position(|candidate| is_expected(candidate) && candidate.interface_number == 0)
        .or_else(|| {
            candidates
                .iter()
                .position(|candidate| is_expected(candidate) && candidate.usage_page >= 0xff00)
        })
}

pub trait Backend {
    fn name(&self) -> &'static str;
    /// `context` is the daemon's current EC state; the wire encoding of fan
    /// and profile operations each re-asserts part of the other's state.
    fn apply(
        &mut self,
        device: DeviceId,
        operation: RequestedOperation,
        context: EcContext,
    ) -> Result<(), String>;
}

/// Logs every accepted operation instead of touching hardware.
#[derive(Debug, Default)]
pub struct DryRunBackend {
    pub applied: Vec<RequestedOperation>,
    /// The context each operation was applied under, index-aligned with
    /// `applied`; tests use it to prove state rides along correctly.
    pub contexts: Vec<EcContext>,
}

impl Backend for DryRunBackend {
    fn name(&self) -> &'static str {
        "dry-run"
    }

    fn apply(
        &mut self,
        device: DeviceId,
        operation: RequestedOperation,
        context: EcContext,
    ) -> Result<(), String> {
        eprintln!(
            "dry-run: would send {operation:?} (context {context:?}) to {:04x}:{:04x}",
            device.vendor_id, device.product_id
        );
        self.applied.push(operation);
        self.contexts.push(context);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLADE_14_2023, RAZER_VENDOR_ID};

    fn blade(interface_number: i32, usage_page: u16) -> HidCandidate {
        HidCandidate {
            vendor_id: RAZER_VENDOR_ID,
            product_id: BLADE_14_2023.id.product_id,
            interface_number,
            usage_page,
        }
    }

    #[test]
    fn prefers_the_expected_product_on_interface_zero() {
        let candidates = [blade(1, 0x0001), blade(0, 0x0001), blade(2, 0xff00)];
        assert_eq!(select_hid_candidate(BLADE_14_2023.id, &candidates), Some(1));
    }

    #[test]
    fn falls_back_to_a_vendor_defined_usage_page() {
        let candidates = [blade(1, 0x0001), blade(2, 0xff00)];
        assert_eq!(select_hid_candidate(BLADE_14_2023.id, &candidates), Some(1));
    }

    #[test]
    fn never_selects_a_razer_peripheral_instead_of_the_laptop() {
        // A Razer mouse on interface 0 with a generic-desktop usage page:
        // right vendor, wrong product.  It must not be chosen.
        let mouse = HidCandidate {
            vendor_id: RAZER_VENDOR_ID,
            product_id: 0x00ab,
            interface_number: 0,
            usage_page: 0x0001,
        };
        assert_eq!(select_hid_candidate(BLADE_14_2023.id, &[mouse]), None);
        // Even alongside the laptop, the mouse never wins.
        let candidates = [mouse, blade(0, 0x0001)];
        assert_eq!(select_hid_candidate(BLADE_14_2023.id, &candidates), Some(1));
    }

    #[test]
    fn backend_choice_parses_only_known_names() {
        assert_eq!(BackendChoice::parse("dry-run"), Ok(BackendChoice::DryRun));
        assert_eq!(BackendChoice::parse("hidraw"), Ok(BackendChoice::Hidraw));
        assert!(BackendChoice::parse("yolo").is_err());
    }
}
