//! Display & GPU: desktop integration that Synapse can only do on Windows —
//! refresh rates via kscreen-doctor (KDE), the GPU MUX/offload mode via the
//! installed switching tool, panel backlight via logind, and external
//! monitors over DDC/CI.  None of it goes through the daemon; each row says
//! which tool it drives.

use crate::app::poll::Poller;
use crate::app::{client, system, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use std::cell::Cell;
use std::rc::Rc;

pub fn page(overlay: &adw::ToastOverlay, poller: &Rc<Poller>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();

    let outputs = system::display_outputs();
    let laptop_index = outputs
        .iter()
        .position(|(name, _)| name.starts_with("eDP"))
        .or(if outputs.is_empty() { None } else { Some(0) });

    if let Some(index) = laptop_index {
        let (name, modes) = &outputs[index];
        page.add(&laptop_display_group(overlay, poller, name, modes));
    } else {
        let group = adw::PreferencesGroup::builder()
            .title("Laptop display")
            .description(
                "No controllable display in this session: kscreen-doctor (KDE) is unavailable.",
            )
            .build();
        page.add(&group);
    }

    page.add(&gpu_mode_group(overlay, system::gpu::detect()));

    if let Some(group) = brightness_group(overlay) {
        page.add(&group);
    }

    let externals: Vec<&(String, Vec<system::DisplayMode>)> = outputs
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != laptop_index)
        .map(|(_, output)| output)
        .collect();
    if !externals.is_empty() {
        page.add(&external_display_group(overlay, &externals));
    }
    if system::ddc_available() {
        page.add(&ddc_group(overlay));
    }

    page.add(&color_group(overlay));
    page.upcast()
}

/// Refresh-rate combo for one output.  Returns the row and a cell holding
/// the user's chosen index (the battery rule's restore target).
fn rate_row(
    overlay: &adw::ToastOverlay,
    title: &str,
    subtitle: &str,
    output_name: &str,
    candidates: Rc<Vec<(String, String)>>,
    initial: usize,
) -> (adw::ComboRow, Rc<Cell<usize>>) {
    let labels: Vec<String> = candidates
        .iter()
        .map(|(hz, _)| format!("{hz} Hz"))
        .collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(&label_refs))
        .selected(initial as u32)
        .build();
    let chosen = Rc::new(Cell::new(initial));
    let output_name = output_name.to_owned();
    row.connect_selected_notify(clone!(
        #[weak]
        overlay,
        #[strong]
        chosen,
        #[strong]
        candidates,
        move |row| {
            let index = row.selected() as usize;
            chosen.set(index);
            let Some((_, mode_id)) = candidates.get(index) else {
                return;
            };
            let output_name = output_name.clone();
            let mode_id = mode_id.clone();
            client::spawn_result(
                &overlay,
                move || system::apply_display_mode(&output_name, &mode_id),
                |_| {},
            );
        }
    ));
    (row, chosen)
}

/// Laptop display: resolution, refresh rate, and the 60 Hz-on-battery rule.
/// The rule watches the daemon's power telemetry: dropping to battery
/// applies the lowest rate directly (the combo keeps showing your choice),
/// and returning to AC re-applies the chosen rate.
fn laptop_display_group(
    overlay: &adw::ToastOverlay,
    poller: &Rc<Poller>,
    output_name: &str,
    modes: &[system::DisplayMode],
) -> adw::PreferencesGroup {
    let (candidates, initial) = system::rate_candidates(modes);
    let candidates = Rc::new(candidates);
    let resolution = modes
        .iter()
        .find(|mode| mode.current)
        .or(modes.first())
        .and_then(|mode| system::split_mode_label(&mode.label))
        .map(|(resolution, _)| resolution.replace('x', " × "))
        .unwrap_or_default();

    let group = adw::PreferencesGroup::builder()
        .title("Laptop display")
        .description("Applies instantly through kscreen-doctor (KDE).")
        .build();

    let (resolution_row, resolution_value) = ui::value_row("Resolution", None);
    resolution_value.set_text(&resolution);
    group.add(&resolution_row);

    let (rate, chosen) = rate_row(
        overlay,
        "Refresh rate",
        output_name,
        output_name,
        Rc::clone(&candidates),
        initial,
    );
    group.add(&rate);

    let battery_rule = adw::SwitchRow::builder()
        .title("Drop to the lowest rate on battery")
        .subtitle("Restores your chosen rate when plugged back in")
        .active(system::gui_config_get("battery_60hz").as_deref() == Some("true"))
        .build();
    battery_rule.connect_active_notify(|row| {
        system::gui_config_set(
            "battery_60hz",
            if row.is_active() { "true" } else { "false" },
        );
    });
    group.add(&battery_rule);

    // The rule itself: act on power-source transitions only.
    let output_name = output_name.to_owned();
    let last_power = Cell::new(None::<bool>);
    poller.subscribe(move |snapshot| {
        let on_ac = match snapshot.telemetry.get("power").map(String::as_str) {
            Some("ac") => true,
            Some("battery") => false,
            _ => return,
        };
        let previous = last_power.replace(Some(on_ac));
        if previous == Some(on_ac) || previous.is_none() || !battery_rule.is_active() {
            return;
        }
        let target = if on_ac {
            candidates.get(chosen.get())
        } else {
            candidates.first() // lowest rate
        };
        if let Some((_, mode_id)) = target {
            let output_name = output_name.clone();
            let mode_id = mode_id.clone();
            gtk::gio::spawn_blocking(move || {
                if let Err(error) = system::apply_display_mode(&output_name, &mode_id) {
                    eprintln!("battery refresh-rate rule: {error}");
                }
            });
        }
    });
    group
}

fn gpu_mode_group(
    overlay: &adw::ToastOverlay,
    detected: Option<(system::gpu::Tool, system::gpu::Mode)>,
) -> adw::PreferencesGroup {
    use system::gpu::Mode;
    const OPTIONS: [(&str, &str, Mode); 3] = [
        (
            "Hybrid (NVIDIA Optimus)",
            "Integrated and dedicated GPU switching for performance and battery life",
            Mode::Hybrid,
        ),
        (
            "Dedicated GPU only",
            "Drive graphics through the dedicated GPU for lower latency",
            Mode::Dedicated,
        ),
        (
            "Integrated only",
            "The NVIDIA GPU powers down completely — maximum battery life",
            Mode::Integrated,
        ),
    ];
    let row_index = |mode: Mode| {
        OPTIONS
            .iter()
            .position(|(_, _, candidate)| *candidate == mode)
            .unwrap_or(0)
    };

    let group = adw::PreferencesGroup::builder()
        .title("GPU mode")
        .description("A mode change applies after you log out or reboot.")
        .build();

    let mut checks: Vec<gtk::CheckButton> = Vec::with_capacity(OPTIONS.len());
    let mut rows: Vec<adw::ActionRow> = Vec::with_capacity(OPTIONS.len());
    for (title, subtitle, _) in OPTIONS {
        let (row, check) = ui::radio_row(title, subtitle, checks.first());
        group.add(&row);
        rows.push(row);
        checks.push(check);
    }

    let Some((tool, current)) = detected else {
        group.set_sensitive(false);
        group.set_description(Some(
            "Locked: no supported GPU switching tool found. Install supergfxctl (Fedora), \
             prime-select (Ubuntu), or envycontrol.",
        ));
        return group;
    };

    checks[row_index(current)].set_active(true);
    if !system::gpu::supports_dedicated(tool) {
        rows[row_index(Mode::Dedicated)].set_sensitive(false);
        rows[row_index(Mode::Dedicated)]
            .set_subtitle("Not supported by supergfxctl on this hardware");
    }

    let last_applied = Rc::new(Cell::new(current));
    let all_checks = Rc::new(checks.clone());
    for (index, check) in checks.iter().enumerate() {
        let mode = OPTIONS[index].2;
        check.connect_toggled(clone!(
            #[weak]
            overlay,
            #[strong]
            last_applied,
            #[strong]
            all_checks,
            move |check| {
                if !check.is_active() || last_applied.get() == mode {
                    return;
                }
                // pkexec may pop a polkit dialog; the switch runs off-thread
                // and the radios revert if it fails.
                let last_applied = Rc::clone(&last_applied);
                let all_checks = Rc::clone(&all_checks);
                client::spawn_result(
                    &overlay,
                    move || system::gpu::switch(tool, mode),
                    move |result| match result {
                        Ok(_) => last_applied.set(mode),
                        Err(_) => {
                            // Put the selection back on the mode that is
                            // actually active; the guard above keeps this
                            // from re-triggering a switch.
                            let active = OPTIONS
                                .iter()
                                .position(|(_, _, candidate)| *candidate == last_applied.get())
                                .unwrap_or(0);
                            all_checks[active].set_active(true);
                        }
                    },
                );
            }
        ));
    }
    group
}

fn brightness_group(overlay: &adw::ToastOverlay) -> Option<adw::PreferencesGroup> {
    let (device, current, max) = system::backlight_device()?;
    let group = adw::PreferencesGroup::builder()
        .title("Panel brightness")
        .description(
            "The built-in screen's backlight, set through logind — no privileges \
             needed.",
        )
        .build();

    let adjustment = gtk::Adjustment::new(
        current as f64,
        0.0,
        max as f64,
        (max as f64 / 100.0).max(1.0),
        (max as f64 / 10.0).max(1.0),
        0.0,
    );
    let (row, _scale) = ui::scale_row("Brightness", None, &adjustment);
    let pending = ui::debouncer();
    adjustment.connect_value_changed(clone!(
        #[weak]
        overlay,
        move |adjustment| {
            let value = adjustment.value().round() as u32;
            let device = device.clone();
            let overlay = overlay.clone();
            ui::debounce(&pending, 150, move || {
                gtk::gio::spawn_blocking(move || {
                    if let Err(error) = system::set_backlight(&device, value) {
                        eprintln!("{error}");
                    }
                });
                let _ = &overlay; // brightness is silent; no toast per tick
            });
        }
    ));
    group.add(&row);
    Some(group)
}

fn external_display_group(
    overlay: &adw::ToastOverlay,
    externals: &[&(String, Vec<system::DisplayMode>)],
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("External displays")
        .description("Refresh rate applies instantly to the selected display.")
        .build();
    for (name, modes) in externals {
        let (candidates, initial) = system::rate_candidates(modes);
        let resolution = modes
            .iter()
            .find(|mode| mode.current)
            .or(modes.first())
            .and_then(|mode| system::split_mode_label(&mode.label))
            .map(|(resolution, _)| resolution.to_owned())
            .unwrap_or_default();
        let (row, _) = rate_row(
            overlay,
            name,
            &resolution,
            name,
            Rc::new(candidates),
            initial,
        );
        group.add(&row);
    }
    group
}

fn ddc_group(overlay: &adw::ToastOverlay) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("External monitor (DDC/CI)")
        .description("Brightness and color presets sent over DDC/CI via ddcutil.")
        .build();

    let adjustment = gtk::Adjustment::new(75.0, 0.0, 100.0, 5.0, 10.0, 0.0);
    let (row, _scale) = ui::scale_row("Brightness", None, &adjustment);
    let pending = ui::debouncer();
    adjustment.connect_value_changed(clone!(
        #[weak]
        overlay,
        move |adjustment| {
            let value = adjustment.value().round() as u32;
            let overlay = overlay.clone();
            ui::debounce(&pending, 300, move || {
                client::spawn_result(
                    &overlay,
                    move || system::ddc_set("10", &value.to_string()),
                    |_| {},
                );
            });
        }
    ));
    group.add(&row);

    // VCP 0x14 select-color-preset codes: 4000/5000K warm 0x04, 6500K 0x05,
    // 9300K 0x08.
    let temperature_row = adw::ComboRow::builder()
        .title("Color temperature")
        .model(&gtk::StringList::new(&[
            "Warm (5000 K)",
            "sRGB (6500 K)",
            "Cool (9300 K)",
        ]))
        .selected(1)
        .build();
    temperature_row.connect_selected_notify(clone!(
        #[weak]
        overlay,
        move |row| {
            let code = ["04", "05", "08"][row.selected() as usize];
            client::spawn_result(&overlay, move || system::ddc_set("14", code), |_| {});
        }
    ));
    group.add(&temperature_row);
    group
}

fn color_group(overlay: &adw::ToastOverlay) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Color profile")
        .description("ICC profiles and calibration are managed by the desktop's color settings.")
        .build();
    let row = adw::ActionRow::builder()
        .title("Open color settings")
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
    row.connect_activated(clone!(
        #[weak]
        overlay,
        move |_| {
            client::spawn_result(
                &overlay,
                || {
                    if client::is_mock() {
                        return Ok("Color settings opened (simulated)".to_owned());
                    }
                    std::process::Command::new("systemsettings")
                        .arg("kcm_colord")
                        .spawn()
                        .map(|_| "Opening color settings".to_owned())
                        .map_err(|error| format!("cannot open systemsettings: {error}"))
                },
                |_| {},
            );
        }
    ));
    group.add(&row);
    group
}
