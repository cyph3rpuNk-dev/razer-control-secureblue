//! One module per sidebar page.  Every page is a plain function returning a
//! `gtk::Widget`; live data arrives through a [`super::poll::Poller`]
//! subscription, actions leave through [`super::client::send`].

pub mod automation;
pub mod battery;
pub mod cooling;
pub mod diagnostics;
pub mod display;
pub mod lighting;
pub mod overview;
pub mod performance;

// Verified fan range and Razer Battery Health Optimizer limits for the Blade
// 14 (2023).  The daemon re-checks every value; these only bound the widgets
// so the UI cannot even offer an out-of-policy request.
pub const FAN_MIN_RPM: f64 = 2000.0;
pub const FAN_MAX_RPM: f64 = 5400.0;
/// The EC's own default target, marked on the fan scale.
pub const FAN_DEFAULT_RPM: f64 = 3800.0;
pub const BHO_MIN_PERCENT: f64 = 50.0;
pub const BHO_MAX_PERCENT: f64 = 80.0;

/// The note shown wherever an experimental control is locked.
pub const EXPERIMENTAL_LOCKED: &str = "These controls send EC commands not yet verified on this \
     machine (Phase 3). Start the daemon with --experimental to enable them.";
