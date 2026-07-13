//! Hardware backends.  The daemon core only talks to this trait; the sole
//! implementation today is a dry run that records what it would have sent.
//! A real hidraw backend must not be added until the protocol layer has
//! model-specific integration tests.

use crate::{DeviceId, RequestedOperation};

pub trait Backend {
    fn name(&self) -> &'static str;
    fn apply(&mut self, device: DeviceId, operation: RequestedOperation) -> Result<(), String>;
}

/// Logs every accepted operation instead of touching hardware.
#[derive(Debug, Default)]
pub struct DryRunBackend {
    pub applied: Vec<RequestedOperation>,
}

impl Backend for DryRunBackend {
    fn name(&self) -> &'static str {
        "dry-run"
    }

    fn apply(&mut self, device: DeviceId, operation: RequestedOperation) -> Result<(), String> {
        eprintln!(
            "dry-run: would send {operation:?} to {:04x}:{:04x}",
            device.vendor_id, device.product_id
        );
        self.applied.push(operation);
        Ok(())
    }
}
