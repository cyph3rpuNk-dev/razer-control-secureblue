//! Overview: device identity card, live telemetry, and daemon status.
//! Pure display — every control lives on its own page.

use crate::app::poll::{Poller, Snapshot};
use crate::app::ui;
use crate::app::{client, poll};
use adw::prelude::*;
use gtk::glib;
use std::collections::HashMap;
use std::rc::Rc;

/// Rendered height of the device portrait, in pixels.
const PORTRAIT_HEIGHT: i32 = 180;

pub fn page(poller: &Rc<Poller>) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();

    content.append(&device_card());

    // Live telemetry, fed by the shared 2 s poll.
    let telemetry_group = adw::PreferencesGroup::builder().title("Telemetry").build();
    let (cpu_row, cpu_value) = ui::value_row("CPU", None);
    let (gpu_row, gpu_value) = ui::value_row("GPU", None);
    let (fan_row, fan_value) = ui::value_row("Fan", None);
    let (memory_row, memory_value) = ui::value_row("Memory", None);
    for row in [&cpu_row, &gpu_row, &fan_row, &memory_row] {
        telemetry_group.add(row);
    }
    content.append(&telemetry_group);

    // Daemon status: what the GUI is talking to and what state it holds.
    let status_group = adw::PreferencesGroup::builder().title("Status").build();
    let (connection_row, connection_value) = ui::value_row("Connection", None);
    let (backend_row, backend_value) = ui::value_row("Backend", None);
    let (experimental_row, experimental_value) = ui::value_row("Experimental controls", None);
    let (power_row, power_value) = ui::value_row("Power source", None);
    let (profile_row, profile_value) = ui::value_row("Performance profile", None);
    let (fan_mode_row, fan_mode_value) = ui::value_row("Fan mode", None);
    for row in [
        &connection_row,
        &backend_row,
        &experimental_row,
        &power_row,
        &profile_row,
        &fan_mode_row,
    ] {
        status_group.add(row);
    }
    content.append(&status_group);

    // Static machine identity, read once (does not change while running).
    let sysinfo = client::request_sysinfo_blocking();
    let has_dgpu = sysinfo.get("gpu_dgpu").is_some_and(|value| value != "none");
    if let Some(group) = hardware_group(&sysinfo) {
        content.append(&group);
    }

    poller.subscribe(move |snapshot: &Snapshot| {
        let number = |map: &HashMap<String, String>, key: &str| {
            map.get(key).and_then(|value| value.parse::<f64>().ok())
        };
        let telemetry = &snapshot.telemetry;

        // CPU: temperature headline, utilisation · clock underneath.
        cpu_value.set_text(&match number(telemetry, "cpu_temp") {
            Some(temp) => format!("{temp:.0} °C"),
            None => "—".to_owned(),
        });
        let mut cpu_parts = Vec::new();
        if let Some(util) = number(telemetry, "cpu_util") {
            cpu_parts.push(format!("{util:.0}% load"));
        }
        if let Some(mhz) = number(telemetry, "cpu_freq") {
            cpu_parts.push(format!("{:.2} GHz", mhz / 1000.0));
        }
        cpu_row.set_subtitle(&ui::join_or_dash(&cpu_parts));

        // GPU: temperature, utilisation · VRAM, plus a dGPU-asleep note when
        // telemetry has fallen back to the integrated GPU.
        gpu_value.set_text(&match number(telemetry, "gpu_temp") {
            Some(temp) => format!("{temp:.0} °C"),
            None => "—".to_owned(),
        });
        let mut gpu_parts = Vec::new();
        if let Some(util) = number(telemetry, "gpu_util") {
            gpu_parts.push(format!("{util:.0}% load"));
        }
        if let (Some(used), Some(total)) = (
            number(telemetry, "gpu_mem_used"),
            number(telemetry, "gpu_mem_total"),
        ) {
            gpu_parts.push(format!(
                "{:.1} / {:.1} GB VRAM",
                used / 1024.0,
                total / 1024.0
            ));
        }
        let mut gpu_text = ui::join_or_dash(&gpu_parts);
        if telemetry.get("gpu_source").map(String::as_str) == Some("igpu") && has_dgpu {
            gpu_text.push_str(" · dGPU asleep");
        }
        gpu_row.set_subtitle(&gpu_text);

        fan_value.set_text(&match number(telemetry, "fan_rpm") {
            Some(rpm) => format!("{rpm:.0} RPM"),
            None => "—".to_owned(),
        });

        memory_value.set_text(&match (
            number(telemetry, "mem_used"),
            number(telemetry, "mem_total"),
        ) {
            (Some(used), Some(total)) if total > 0.0 => format!(
                "{:.1} / {:.1} GB ({:.0}%)",
                used / 1_048_576.0,
                total / 1_048_576.0,
                used / total * 100.0
            ),
            _ => "—".to_owned(),
        });

        update_status(
            snapshot,
            &connection_value,
            &backend_value,
            &experimental_value,
            &power_value,
            &profile_value,
            &fan_mode_value,
        );
    });

    scrolled(content)
}

fn update_status(
    snapshot: &poll::Snapshot,
    connection: &gtk::Label,
    backend: &gtk::Label,
    experimental: &gtk::Label,
    power: &gtk::Label,
    profile: &gtk::Label,
    fan_mode: &gtk::Label,
) {
    connection.set_text(if client::is_mock() {
        "In-process mock (simulated data)"
    } else if snapshot.reachable {
        "Daemon socket"
    } else {
        "Daemon not reachable"
    });
    let status = &snapshot.status;
    backend.set_text(match status.get("backend").map(String::as_str) {
        Some("dry-run") => "Dry run — no hardware writes",
        Some("hidraw") => "Hardware (hidraw)",
        Some(other) => other,
        None => "—",
    });
    experimental.set_text(match status.get("experimental").map(String::as_str) {
        Some("true") => "Enabled",
        Some("false") => "Disabled",
        _ => "—",
    });
    power.set_text(match snapshot.telemetry.get("power").map(String::as_str) {
        Some("ac") => "Plugged in",
        Some("battery") => "On battery",
        _ => "—",
    });
    profile.set_text(&status.get("profile").map_or_else(
        || "—".to_owned(),
        |name| {
            let mut chars = name.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        },
    ));
    fan_mode.set_text(&match status.get("fan").map(String::as_str) {
        Some("auto") => "Automatic".to_owned(),
        Some(manual) if manual.starts_with("manual:") => {
            format!("Manual, {} RPM", manual.trim_start_matches("manual:"))
        }
        _ => "—".to_owned(),
    });
}

/// The device identity card: portrait, model name, USB id.  Uses Adwaita's
/// built-in `card` style; the portrait is painted with Cairo from the
/// full-resolution embedded PNG so it stays sharp at any scale factor.
fn device_card() -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["card"])
        .build();

    if let Some(portrait) = device_portrait() {
        portrait.set_margin_top(18);
        card.append(&portrait);
    }

    let name = gtk::Label::builder()
        .label(razer_control_secureblue::BLADE_14_2023.name)
        .css_classes(["title-2"])
        .build();
    let id = gtk::Label::builder()
        .label("USB 1532:029d")
        .css_classes(["dim-label", "numeric"])
        .margin_bottom(18)
        .build();
    card.append(&name);
    card.append(&id);
    card.upcast()
}

/// The Blade portrait, decoded once at full resolution and painted into a
/// fixed-height `DrawingArea`.  Cairo resamples the source at the real
/// device-pixel size on every draw, so the image stays crisp on HiDPI and
/// fractional scales where a pre-scaled `GtkPicture` raster would blur.
fn device_portrait() -> Option<gtk::DrawingArea> {
    use gtk::gdk::prelude::GdkCairoContextExt;

    let bytes = glib::Bytes::from_static(include_bytes!("../../../resources/1532-029d.png"));
    let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
    let pixbuf = match gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE) {
        Ok(pixbuf) => Rc::new(pixbuf),
        // A decode failure is cosmetic: keep the card text, drop the image.
        Err(error) => {
            eprintln!("overview: could not load device image: {error}");
            return None;
        }
    };
    let (source_w, source_h) = (pixbuf.width(), pixbuf.height());
    let content_w = (PORTRAIT_HEIGHT as f64 * source_w as f64 / source_h as f64).round() as i32;
    let area = gtk::DrawingArea::new();
    area.set_content_height(PORTRAIT_HEIGHT);
    area.set_content_width(content_w);
    area.set_halign(gtk::Align::Center);
    area.set_draw_func(move |_, cr, width, height| {
        let scale = (width as f64 / source_w as f64).min(height as f64 / source_h as f64);
        let (draw_w, draw_h) = (source_w as f64 * scale, source_h as f64 * scale);
        cr.translate(
            (width as f64 - draw_w) / 2.0,
            (height as f64 - draw_h) / 2.0,
        );
        cr.scale(scale, scale);
        cr.set_source_pixbuf(&pixbuf, 0.0, 0.0);
        cr.source().set_filter(gtk::cairo::Filter::Good);
        let _ = cr.paint();
    });
    Some(area)
}

fn hardware_group(sysinfo: &HashMap<String, String>) -> Option<adw::PreferencesGroup> {
    let get = |key: &str| {
        sysinfo
            .get(key)
            .map(String::as_str)
            .filter(|value| *value != "none")
    };
    let cpu = get("cpu_model")?;
    let group = adw::PreferencesGroup::builder().title("Hardware").build();

    let subtitle = match (get("cpu_cores"), get("cpu_threads")) {
        (Some(cores), Some(threads)) => format!("{cores} cores, {threads} threads"),
        _ => String::new(),
    };
    let cpu_row = adw::ActionRow::builder()
        .title("Processor")
        .subtitle(cpu)
        .build();
    if !subtitle.is_empty() {
        let detail = gtk::Label::builder()
            .label(&subtitle)
            .css_classes(["dim-label"])
            .build();
        cpu_row.add_suffix(&detail);
    }
    group.add(&cpu_row);

    if let Some(dgpu) = get("gpu_dgpu") {
        group.add(
            &adw::ActionRow::builder()
                .title("Dedicated GPU")
                .subtitle(dgpu)
                .build(),
        );
    }
    if let Some(igpu) = get("gpu_igpu") {
        group.add(
            &adw::ActionRow::builder()
                .title("Integrated GPU")
                .subtitle(igpu)
                .build(),
        );
    }
    Some(group)
}

/// Wrap page content in the standard clamp + scroll, matching
/// `AdwPreferencesPage` metrics.
fn scrolled(content: gtk::Box) -> gtk::Widget {
    let clamp = adw::Clamp::builder()
        .maximum_size(600)
        .tightening_threshold(400)
        .child(&content)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build()
        .upcast()
}
