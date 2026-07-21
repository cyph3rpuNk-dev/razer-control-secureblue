//! Cooling: automatic vs. manual fan speed.  Instant apply with a debounce;
//! the daemon's reply (the policy verdict) is toasted.  Shown as a group on
//! the Performance page.

use super::{FAN_DEFAULT_RPM, FAN_MAX_RPM, FAN_MIN_RPM};
use crate::app::poll::Snapshot;
use crate::app::{client, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;

pub fn group(seed: &Snapshot, overlay: &adw::ToastOverlay) -> adw::PreferencesGroup {
    // Seed from the daemon: fan is "auto" or "manual:<rpm>".
    let seeded_rpm = seed
        .status
        .get("fan")
        .and_then(|value| value.strip_prefix("manual:"))
        .and_then(|rpm| rpm.parse::<f64>().ok());
    let manual_initially = seeded_rpm.is_some();
    let initial_rpm = seeded_rpm
        .unwrap_or(FAN_DEFAULT_RPM)
        .clamp(FAN_MIN_RPM, FAN_MAX_RPM);

    let group = adw::PreferencesGroup::builder()
        .title("FAN SPEED")
        .description(
            "Manual control holds the fan at a fixed speed and reverts to automatic \
             when the daemon stops (logout or shutdown).",
        )
        .build();

    let manual_row = adw::SwitchRow::builder()
        .title("Manual fan speed")
        .subtitle("Off lets the system manage cooling")
        .active(manual_initially)
        .build();
    group.add(&manual_row);

    // One shared adjustment drives both the scale and the spin row, so they
    // can never disagree.
    let adjustment = gtk::Adjustment::new(initial_rpm, FAN_MIN_RPM, FAN_MAX_RPM, 50.0, 200.0, 0.0);

    let (scale_row, scale) = ui::scale_row("Target speed", Some("2,000 – 5,400 RPM"), &adjustment);
    scale.set_width_request(260);
    for (rpm, label) in [
        (FAN_MIN_RPM, "2000"),
        (FAN_DEFAULT_RPM, "3800"),
        (FAN_MAX_RPM, "5400"),
    ] {
        scale.add_mark(rpm, gtk::PositionType::Bottom, Some(label));
    }
    group.add(&scale_row);

    let spin_row = adw::SpinRow::builder()
        .title("Exact speed")
        .subtitle("RPM")
        .adjustment(&adjustment)
        .climb_rate(50.0)
        .digits(0)
        .build();
    group.add(&spin_row);

    let set_manual_sensitive = clone!(
        #[weak]
        scale_row,
        #[weak]
        spin_row,
        move |on: bool| {
            scale_row.set_sensitive(on);
            spin_row.set_sensitive(on);
        }
    );
    set_manual_sensitive(manual_initially);

    // Wired after the initial state is set, so building the page sends
    // nothing.  The switch applies immediately; the sliders debounce so a
    // drag is one request, not one per tick.
    manual_row.connect_active_notify(clone!(
        #[weak]
        overlay,
        #[weak]
        adjustment,
        move |row| {
            let on = row.is_active();
            set_manual_sensitive(on);
            let line = if on {
                format!("fan manual {}", adjustment.value().round() as u16)
            } else {
                "fan auto".to_owned()
            };
            client::send(&overlay, line, |_| {});
        }
    ));

    let pending = ui::debouncer();
    adjustment.connect_value_changed(clone!(
        #[weak]
        overlay,
        #[weak]
        manual_row,
        move |adjustment| {
            // Snap scale clicks to the 50 RPM step; setting the snapped
            // value re-enters this handler with an exact value.
            let snapped = (adjustment.value() / 50.0).round() * 50.0;
            if (adjustment.value() - snapped).abs() > f64::EPSILON {
                adjustment.set_value(snapped);
                return;
            }
            if !manual_row.is_active() {
                return;
            }
            let rpm = snapped as u16;
            let overlay = overlay.clone();
            ui::debounce(&pending, 300, move || {
                client::send(&overlay, format!("fan manual {rpm}"), |_| {});
            });
        }
    ));

    group
}
