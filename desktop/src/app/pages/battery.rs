//! Battery: the Battery Health Optimizer.  Bounded to the verified 50–80%
//! range so the UI never offers a limit the daemon would reject.

use super::{BHO_MAX_PERCENT, BHO_MIN_PERCENT};
use crate::app::poll::Snapshot;
use crate::app::{client, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;

pub fn page(seed: &Snapshot, overlay: &adw::ToastOverlay) -> gtk::Widget {
    // Seed from the daemon: bho is "unset"/"off" or the limit number.
    let (enabled, limit) = match seed
        .status
        .get("bho")
        .and_then(|value| value.parse::<f64>().ok())
    {
        Some(value) => (true, value.clamp(BHO_MIN_PERCENT, BHO_MAX_PERCENT)),
        None => (false, BHO_MAX_PERCENT),
    };

    let group = adw::PreferencesGroup::builder()
        .title("BATTERY HEALTH OPTIMIZER")
        .description("Stops charging at the limit to protect long-term battery health.")
        .build();

    let switch_row = adw::SwitchRow::builder()
        .title("Limit charging")
        .active(enabled)
        .build();
    group.add(&switch_row);

    let adjustment = gtk::Adjustment::new(limit, BHO_MIN_PERCENT, BHO_MAX_PERCENT, 5.0, 10.0, 0.0);
    let spin_row = adw::SpinRow::builder()
        .title("Charge limit")
        .subtitle("50 – 80%")
        .adjustment(&adjustment)
        .climb_rate(5.0)
        .digits(0)
        .sensitive(enabled)
        .build();
    group.add(&spin_row);

    // Wired after the initial state, so building the page sends nothing.
    switch_row.connect_active_notify(clone!(
        #[weak]
        overlay,
        #[weak]
        spin_row,
        #[weak]
        adjustment,
        move |row| {
            let on = row.is_active();
            spin_row.set_sensitive(on);
            let line = if on {
                format!("bho {}", adjustment.value().round() as u8)
            } else {
                "bho off".to_owned()
            };
            client::send(&overlay, line, |_| {});
        }
    ));

    let pending = ui::debouncer();
    adjustment.connect_value_changed(clone!(
        #[weak]
        overlay,
        #[weak]
        switch_row,
        move |adjustment| {
            if !switch_row.is_active() {
                return;
            }
            let value = adjustment.value().round() as u8;
            let overlay = overlay.clone();
            ui::debounce(&pending, 300, move || {
                client::send(&overlay, format!("bho {value}"), |_| {});
            });
        }
    ));

    let page = adw::PreferencesPage::new();
    page.add(&group);
    page.upcast()
}
