//! Lighting: keyboard backlight (per-power-source brightness, effects with a
//! static color), the idle switch-off rule, and the lid logo.  All EC
//! lighting is experimental-gated; the page locks with a banner when the
//! daemon runs without `--experimental`.

use super::EXPERIMENTAL_LOCKED;
use crate::app::poll::{Poller, Snapshot};
use crate::app::{client, system, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use std::cell::Cell;
use std::rc::Rc;

/// Fixed color presets for the static effect; class names match the
/// stylesheet's swatch entries.
const SWATCHES: [&str; 6] = ["44d62c", "ffffff", "ff3a3a", "2c7bd6", "a02cd6", "2cd6c8"];

pub fn page(seed: &Snapshot, overlay: &adw::ToastOverlay, poller: &Rc<Poller>) -> gtk::Widget {
    let experimental = seed.status_is("experimental", "true");

    let banner = adw::Banner::builder()
        .title(EXPERIMENTAL_LOCKED)
        .revealed(!experimental)
        .build();

    let keyboard_group = keyboard_group(seed, overlay, experimental);
    let logo_group = logo_group(seed, overlay, experimental);
    let idle_group = idle_group(seed, poller, experimental);

    let page = adw::PreferencesPage::new();
    page.add(&keyboard_group);
    page.add(&logo_group);
    page.add(&idle_group);
    page.set_vexpand(true);

    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.append(&banner);
    container.append(&page);
    container.upcast()
}

/// Per-power-source brightness rows plus the effect selector.  Brightness
/// changes store the daemon-side rule and apply live when the row matches
/// the current power source.
fn keyboard_group(
    seed: &Snapshot,
    overlay: &adw::ToastOverlay,
    experimental: bool,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("KEYBOARD BACKLIGHT")
        .description(
            "Brightness is remembered per power source and applied when you plug in \
             or unplug.",
        )
        .sensitive(experimental)
        .build();

    let fallback = seed
        .status
        .get("kbd")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(60.0);
    let rule = |key: &str| {
        seed.status
            .get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(fallback)
    };

    for (title, source, initial) in [
        ("Brightness when plugged in", "ac", rule("kbd_ac")),
        ("Brightness on battery", "battery", rule("kbd_battery")),
    ] {
        let adjustment = gtk::Adjustment::new(initial, 0.0, 100.0, 5.0, 10.0, 0.0);
        let (row, _scale) = ui::scale_row(title, None, &adjustment);
        let pending = ui::debouncer();
        adjustment.connect_value_changed(clone!(
            #[weak]
            overlay,
            move |adjustment| {
                let percent = adjustment.value().round() as u8;
                let overlay = overlay.clone();
                ui::debounce(&pending, 300, move || {
                    let rule_line = format!("kbd-automation {source} {percent}");
                    client::send(&overlay.clone(), rule_line, move |result| {
                        if result.is_err() {
                            return;
                        }
                        // Apply live when this row matches the current power
                        // source (checked off-thread inside send's worker).
                        client::spawn_result(
                            &overlay,
                            move || {
                                let telemetry = client::request_blocking("telemetry")
                                    .as_deref()
                                    .map(client::parse_fields)
                                    .unwrap_or_default();
                                if telemetry.get("power").map(String::as_str) == Some(source) {
                                    client::request_blocking(&format!("kbd brightness {percent}"))
                                } else {
                                    Ok(format!("Saved for {source}"))
                                }
                            },
                            |_| {},
                        );
                    });
                });
            }
        ));
        group.add(&row);
    }

    // Effect selector and, for Static, the color presets.
    let effect_value = seed
        .status
        .get("kbd_effect")
        .map_or("unset", String::as_str);
    let initial_effect = match effect_value {
        "off" => 0,
        "spectrum" => 2,
        "wave" => 3,
        _ => 1, // static or unset
    };
    let initial_swatch = effect_value
        .strip_prefix("static:")
        .and_then(|hex| SWATCHES.iter().position(|candidate| *candidate == hex))
        .unwrap_or(0);

    let effect_row = adw::ComboRow::builder()
        .title("Effect")
        .model(&gtk::StringList::new(&[
            "Off",
            "Static color",
            "Spectrum",
            "Wave",
        ]))
        .selected(initial_effect)
        .build();
    group.add(&effect_row);

    let color_row = adw::ActionRow::builder()
        .title("Color")
        .visible(initial_effect == 1)
        .build();
    let mut swatches: Vec<gtk::ToggleButton> = Vec::with_capacity(SWATCHES.len());
    for (index, hex) in SWATCHES.iter().enumerate() {
        let swatch = gtk::ToggleButton::builder()
            .css_classes(["swatch", &format!("swatch-{index}")[..]])
            .valign(gtk::Align::Center)
            .tooltip_text(format!("#{hex}"))
            .build();
        if let Some(first) = swatches.first() {
            swatch.set_group(Some(first));
        }
        color_row.add_suffix(&swatch);
        swatches.push(swatch);
    }
    swatches[initial_swatch].set_active(true);
    group.add(&color_row);

    let chosen_swatch = Rc::new(Cell::new(initial_swatch));
    let send_effect = {
        let chosen_swatch = Rc::clone(&chosen_swatch);
        move |overlay: &adw::ToastOverlay, effect: u32| {
            let line = match effect {
                0 => "kbd effect off".to_owned(),
                2 => "kbd effect spectrum".to_owned(),
                3 => "kbd effect wave".to_owned(),
                _ => format!("kbd effect static {}", SWATCHES[chosen_swatch.get()]),
            };
            client::send(overlay, line, |_| {});
        }
    };

    effect_row.connect_selected_notify(clone!(
        #[weak]
        overlay,
        #[weak]
        color_row,
        #[strong]
        send_effect,
        move |row| {
            color_row.set_visible(row.selected() == 1);
            send_effect(&overlay, row.selected());
        }
    ));
    for (index, swatch) in swatches.iter().enumerate() {
        let send_effect = send_effect.clone();
        swatch.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            effect_row,
            #[strong]
            chosen_swatch,
            move |swatch| {
                if !swatch.is_active() {
                    return;
                }
                chosen_swatch.set(index);
                if effect_row.selected() == 1 {
                    send_effect(&overlay, 1);
                }
            }
        ));
    }

    group
}

fn logo_group(
    seed: &Snapshot,
    overlay: &adw::ToastOverlay,
    experimental: bool,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("LID LOGO")
        .description("The snake on the lid: keep it lit, pulse it slowly, or turn it off.")
        .sensitive(experimental)
        .build();
    let initial = match seed.status.get("logo").map(String::as_str) {
        Some("off") => 0,
        Some("breathing") => 2,
        _ => 1,
    };
    let row = adw::ComboRow::builder()
        .title("Logo")
        .model(&gtk::StringList::new(&["Off", "Static", "Breathing"]))
        .selected(initial)
        .build();
    row.connect_selected_notify(clone!(
        #[weak]
        overlay,
        move |row| {
            let line = ["logo off", "logo static", "logo breathing"][row.selected() as usize];
            client::send(&overlay, line, |_| {});
        }
    ));
    group.add(&row);
    group
}

/// The idle switch-off rule: a GUI-side timer using the desktop's idle
/// counter.  Past the threshold the keyboard backlight goes to 0%, and
/// activity restores the active power source's brightness.
fn idle_group(_seed: &Snapshot, _poller: &Rc<Poller>, experimental: bool) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("TURN OFF WHEN IDLE")
        .description(
            "Uses the desktop's idle timer (KDE). Lighting returns on activity at the \
             brightness set for the current power source.",
        )
        .sensitive(experimental)
        .build();

    let switch_row = adw::SwitchRow::builder()
        .title("Turn off keyboard lighting when idle")
        .active(system::gui_config_get("kbd_idle_off").as_deref() == Some("true"))
        .build();
    switch_row.connect_active_notify(|row| {
        system::gui_config_set(
            "kbd_idle_off",
            if row.is_active() { "true" } else { "false" },
        );
    });
    group.add(&switch_row);

    let minutes = system::gui_config_get("kbd_idle_minutes")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(10.0);
    let adjustment = gtk::Adjustment::new(minutes, 1.0, 60.0, 1.0, 5.0, 0.0);
    let spin_row = adw::SpinRow::builder()
        .title("Idle time")
        .subtitle("Minutes")
        .adjustment(&adjustment)
        .climb_rate(1.0)
        .digits(0)
        .build();
    let pending = ui::debouncer();
    adjustment.connect_value_changed(move |adjustment| {
        let value = adjustment.value().round() as u32;
        ui::debounce(&pending, 300, move || {
            system::gui_config_set("kbd_idle_minutes", &value.to_string());
        });
    });
    group.add(&spin_row);

    // The rule: poll session idle time every 30 s.  Absent bus or method
    // (WSL, non-KDE) the poll silently does nothing.  Requests run
    // off-thread; the flag flips only after they succeed.
    let dimmed = Rc::new(Cell::new(false));
    glib::timeout_add_seconds_local(
        30,
        clone!(
            #[weak]
            switch_row,
            #[weak]
            adjustment,
            #[strong]
            dimmed,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                if !switch_row.is_active() {
                    return glib::ControlFlow::Continue;
                }
                let threshold = adjustment.value().round() as u64 * 60;
                let dimmed = Rc::clone(&dimmed);
                glib::spawn_future_local(async move {
                    let idle = gtk::gio::spawn_blocking(system::session_idle_seconds)
                        .await
                        .ok()
                        .flatten();
                    let Some(idle_seconds) = idle else { return };
                    if idle_seconds >= threshold && !dimmed.get() {
                        let ok = gtk::gio::spawn_blocking(|| {
                            client::request_blocking("kbd brightness 0").is_ok()
                        })
                        .await
                        .unwrap_or(false);
                        if ok {
                            dimmed.set(true);
                        }
                    } else if idle_seconds < threshold && dimmed.get() {
                        let ok = gtk::gio::spawn_blocking(|| {
                            let telemetry = client::request_blocking("telemetry")
                                .as_deref()
                                .map(client::parse_fields)
                                .unwrap_or_default();
                            let key =
                                if telemetry.get("power").map(String::as_str) == Some("battery") {
                                    "kbd_battery"
                                } else {
                                    "kbd_ac"
                                };
                            let restore = client::request_blocking("status")
                                .as_deref()
                                .map(client::parse_fields)
                                .unwrap_or_default()
                                .get(key)
                                .and_then(|value| value.parse::<u8>().ok())
                                .unwrap_or(60);
                            client::request_blocking(&format!("kbd brightness {restore}")).is_ok()
                        })
                        .await
                        .unwrap_or(false);
                        if ok {
                            dimmed.set(false);
                        }
                    }
                });
                glib::ControlFlow::Continue
            }
        ),
    );
    group
}
