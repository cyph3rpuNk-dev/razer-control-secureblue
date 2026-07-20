//! Automation: the daemon-side fan rules applied when the power source
//! changes.  Rules persist across daemon restarts; the row for the source
//! that is active right now carries an "Active now" tag.

use super::FAN_MIN_RPM;
use crate::app::client;
use crate::app::poll::{Poller, Snapshot};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use std::rc::Rc;

const CHOICES: [&str; 3] = ["Do nothing", "Automatic fan", "Quiet (2000 RPM)"];

pub fn page(seed: &Snapshot, overlay: &adw::ToastOverlay, poller: &Rc<Poller>) -> gtk::Widget {
    let group = adw::PreferencesGroup::builder()
        .title("Fan rules")
        .description(
            "Applied by the daemon whenever the power source changes; the quiet rule \
             pins the fans at the verified 2,000 RPM floor. Rules persist across restarts.",
        )
        .build();

    let mut tags = Vec::new();
    for (title, source, status_key) in [
        ("When plugged in", "ac", "automation_ac"),
        ("When on battery", "battery", "automation_battery"),
    ] {
        let rule = seed.status.get(status_key).map_or("off", String::as_str);
        let initial = if rule == "off" {
            0
        } else if rule.starts_with("manual") {
            2
        } else {
            1
        };
        let row = adw::ComboRow::builder()
            .title(title)
            .model(&gtk::StringList::new(&CHOICES))
            .selected(initial)
            .build();
        let tag = gtk::Label::builder()
            .label("Active now")
            .css_classes(["accent", "caption"])
            .visible(false)
            .build();
        row.add_suffix(&tag);
        row.connect_selected_notify(clone!(
            #[weak]
            overlay,
            move |row| {
                let line = match row.selected() {
                    0 => format!("automation {source} off"),
                    1 => format!("automation {source} fan auto"),
                    _ => format!("automation {source} fan manual {}", FAN_MIN_RPM as u16),
                };
                client::send(&overlay, line, |_| {});
            }
        ));
        group.add(&row);
        tags.push((source, tag));
    }

    // The "Active now" tag follows the live power source from the shared poll.
    poller.subscribe(move |snapshot| {
        let power = snapshot.telemetry.get("power").map(String::as_str);
        for (source, tag) in &tags {
            tag.set_visible(power == Some(source));
        }
    });

    let page = adw::PreferencesPage::new();
    page.add(&group);
    page.upcast()
}
