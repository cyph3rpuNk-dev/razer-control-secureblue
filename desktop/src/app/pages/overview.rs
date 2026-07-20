//! Overview: hero device card with status chips, a dashboard row of
//! CPU/GPU gauge cards and the spinning fan, and a stat-tile grid — all
//! live from the shared poll.  Pure display — every control lives on its
//! own page.

use crate::app::poll::{Poller, Snapshot};
use crate::app::ui;
use crate::app::{client, poll};
use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::rc::Rc;

/// Rendered height of the device portrait, in pixels.
const PORTRAIT_HEIGHT: i32 = 140;

pub fn page(poller: &Rc<Poller>) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();

    content.append(&hero_card(poller));

    // Gauge cards: CPU and GPU temperature arcs plus the fan — the
    // Synapse-style dashboard row, drawn theme-native and fed by the
    // shared 2 s poll.
    let gauges = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(3)
        .row_spacing(12)
        .column_spacing(12)
        .build();
    // Ring colours sampled from the user's AIO pump-screen references:
    // CPU periwinkle over teal, GPU lime over dark olive.
    let cpu = gauge_card("CPU", (0x7c, 0x95, 0xe5), (0x1e, 0x6c, 0x67));
    let gpu = gauge_card("GPU", (0xcb, 0xf1, 0x69), (0x6e, 0x5d, 0x08));

    // Fan card: the spinning glyph over the live RPM readout.
    let fan_rpm = Rc::new(Cell::new(0.0_f64));
    let fan_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["stat-tile"])
        .build();
    let glyph = fan_glyph(&fan_rpm);
    glyph.set_content_width(144);
    glyph.set_content_height(144);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_margin_top(8);
    fan_card.append(&glyph);
    let fan_value = gtk::Label::builder()
        .label("—")
        .css_classes(["caption", "dim-label", "numeric"])
        .build();
    fan_card.append(&fan_value);

    for card in [&fan_card, &cpu.root, &gpu.root] {
        gauges.insert(card, -1);
        // The wrapping FlowBoxChild must not draw a focus ring: the cards
        // are display-only.
        if let Some(child) = card.parent() {
            child.set_focusable(false);
        }
    }
    content.append(&gauges);

    // Stat tiles: the at-a-glance state.
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(4)
        .row_spacing(12)
        .column_spacing(12)
        .build();
    let (power_tile, power_value, _) = ui::stat_tile("Power source");
    let (profile_tile, profile_value, _) = ui::stat_tile("Profile");
    let (backend_tile, backend_value, _) = ui::stat_tile("Backend");
    let (memory_tile, memory_value, memory_detail) = ui::stat_tile("Memory");
    for tile in [&power_tile, &profile_tile, &backend_tile, &memory_tile] {
        grid.insert(tile, -1);
        if let Some(child) = tile.parent() {
            child.set_focusable(false);
        }
    }
    content.append(&grid);

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

        // CPU gauge: temperature on the arc, utilisation · clock underneath.
        let cpu_temp = number(telemetry, "cpu_temp");
        cpu.fraction.set(cpu_temp.unwrap_or(0.0) / 100.0);
        cpu.area.queue_draw();
        cpu.value
            .set_text(&cpu_temp.map_or_else(|| "—".to_owned(), |temp| format!("{temp:.0}°C")));
        let mut cpu_parts = Vec::new();
        if let Some(util) = number(telemetry, "cpu_util") {
            cpu_parts.push(format!("{util:.0}% load"));
        }
        if let Some(mhz) = number(telemetry, "cpu_freq") {
            cpu_parts.push(format!("{:.2} GHz", mhz / 1000.0));
        }
        cpu.detail.set_text(&ui::join_or_dash(&cpu_parts));

        // GPU gauge: temperature, utilisation · VRAM, plus a dGPU-asleep
        // note when telemetry has fallen back to the integrated GPU.
        let gpu_temp = number(telemetry, "gpu_temp");
        gpu.fraction.set(gpu_temp.unwrap_or(0.0) / 100.0);
        gpu.area.queue_draw();
        gpu.value
            .set_text(&gpu_temp.map_or_else(|| "—".to_owned(), |temp| format!("{temp:.0}°C")));
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
        gpu.detail.set_text(&gpu_text);

        let rpm_now = number(telemetry, "fan_rpm");
        fan_rpm.set(rpm_now.unwrap_or(0.0));
        fan_value.set_text(&match rpm_now {
            Some(rpm) => format!("Fan Speed · {rpm:.0} RPM"),
            None => "Fan Speed · —".to_owned(),
        });

        match (
            number(telemetry, "mem_used"),
            number(telemetry, "mem_total"),
        ) {
            (Some(used), Some(total)) if total > 0.0 => {
                memory_value.set_text(&format!("{:.0}%", used / total * 100.0));
                memory_detail.set_text(&format!(
                    "{:.1} / {:.1} GB",
                    used / 1_048_576.0,
                    total / 1_048_576.0
                ));
                memory_detail.set_visible(true);
            }
            _ => {
                memory_value.set_text("—");
                memory_detail.set_visible(false);
            }
        }

        let status = &snapshot.status;
        power_value.set_text(match snapshot.telemetry.get("power").map(String::as_str) {
            Some("ac") => "Plugged in",
            Some("battery") => "On battery",
            _ => "—",
        });
        profile_value.set_text(&status.get("profile").map_or_else(
            || "—".to_owned(),
            |name| {
                let mut chars = name.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                })
            },
        ));
        backend_value.set_text(match status.get("backend").map(String::as_str) {
            Some("dry-run") => "Dry run",
            Some("hidraw") => "Hardware",
            Some(other) => other,
            None => "—",
        });
    });

    scrolled(content)
}

/// The hero card: device portrait, model name, USB id, and live status
/// chips (connection, backend, experimental).  The portrait is painted
/// with Cairo from the full-resolution embedded PNG so it stays sharp at
/// any scale factor.
fn hero_card(poller: &Rc<Poller>) -> gtk::Widget {
    let portrait = device_portrait().map(gtk::Widget::from);
    let hero = ui::hero(
        razer_control_secureblue::BLADE_14_2023.name,
        portrait.as_ref(),
    );
    hero.subtitle.set_text("USB 1532:029d");
    hero.subtitle.add_css_class("numeric");

    let connection = ui::chip("—", ui::ChipKind::Neutral);
    let backend = ui::chip("—", ui::ChipKind::Neutral);
    let experimental = ui::chip("Experimental", ui::ChipKind::Warning);
    experimental.set_visible(false);
    for chip in [&connection, &backend, &experimental] {
        hero.chips.append(chip);
    }

    poller.subscribe(move |snapshot: &poll::Snapshot| {
        if client::is_mock() {
            ui::set_chip(&connection, "Simulated", ui::ChipKind::Accent);
        } else if snapshot.reachable {
            ui::set_chip(&connection, "Connected", ui::ChipKind::Success);
        } else {
            ui::set_chip(&connection, "Disconnected", ui::ChipKind::Warning);
        }
        match snapshot.status.get("backend").map(String::as_str) {
            Some("hidraw") => ui::set_chip(&backend, "Hardware", ui::ChipKind::Success),
            Some("dry-run") => ui::set_chip(&backend, "Dry-run", ui::ChipKind::Neutral),
            Some(other) => ui::set_chip(&backend, other, ui::ChipKind::Neutral),
            None => ui::set_chip(&backend, "—", ui::ChipKind::Neutral),
        }
        experimental
            .set_visible(snapshot.status.get("experimental").map(String::as_str) == Some("true"));
    });

    hero.root
}

/// One dashboard gauge card, built by [`gauge_card`].  The subscribe
/// closure sets `value`/`detail` text, stores 0–1 in `fraction`, and
/// queues a redraw on `area`.
struct GaugeCard {
    root: gtk::Box,
    fraction: Rc<Cell<f64>>,
    area: gtk::DrawingArea,
    value: gtk::Label,
    detail: gtk::Label,
}

/// Sets an exact 8-bit-per-channel colour as the Cairo source.
fn set_source_rgb8(cr: &gtk::cairo::Context, (red, green, blue): (u8, u8, u8)) {
    cr.set_source_rgb(
        f64::from(red) / 255.0,
        f64::from(green) / 255.0,
        f64::from(blue) / 255.0,
    );
}

/// A dashboard gauge card in the AIO-cooler style: a full smooth
/// temperature ring — a `track`-coloured circle under a `fill`-coloured
/// arc sweeping clockwise from 12 o'clock — around a centred stack of
/// component name, value ("55°C"), and a TEMP caption, with a detail line
/// under the card.  The ring colours are exact values sampled from the
/// AIO pump-screen reference images — content, not theming, like the fan
/// green and the black faces; the labels stay in system fonts.
fn gauge_card(name: &str, fill: (u8, u8, u8), track: (u8, u8, u8)) -> GaugeCard {
    let fraction = Rc::new(Cell::new(0.0_f64));
    let area = gtk::DrawingArea::new();
    area.set_content_width(144);
    area.set_content_height(144);
    area.set_draw_func({
        let fraction = Rc::clone(&fraction);
        move |_, cr, width, height| {
            let radius = f64::from(width.min(height)) / 2.0;
            cr.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
            // Black dial face, like the AIO pump screen this mirrors —
            // content, not theming, so it stays black in both schemes
            // (the centre text is white via `.gauge-centre`).
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.arc(0.0, 0.0, radius * 0.78, 0.0, TAU);
            let _ = cr.fill();
            let ring = radius * 0.86;
            cr.set_line_width(radius * 0.22);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            // Track first, then the fill sweeping from 12 o'clock, in the
            // dial's own two colours — sampled from the AIO reference.
            set_source_rgb8(cr, track);
            cr.arc(0.0, 0.0, ring, 0.0, TAU);
            let _ = cr.stroke();
            let filled = fraction.get().clamp(0.0, 1.0);
            if filled > 0.0 {
                set_source_rgb8(cr, fill);
                let start = -TAU / 4.0;
                cr.arc(0.0, 0.0, ring, start, start + filled * TAU);
                let _ = cr.stroke();
            }
        }
    });

    let name_label = gtk::Label::builder()
        .label(name)
        .css_classes(["caption"])
        .build();
    let value = gtk::Label::builder()
        .label("—")
        .css_classes(["title-1", "numeric"])
        .build();
    let temp_label = gtk::Label::builder()
        .label("TEMP")
        .css_classes(["caption"])
        .build();
    let centre = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["gauge-centre"])
        .build();
    centre.append(&name_label);
    centre.append(&value);
    centre.append(&temp_label);
    let overlay = gtk::Overlay::builder().child(&area).build();
    overlay.add_overlay(&centre);
    overlay.set_halign(gtk::Align::Center);
    overlay.set_margin_top(8);

    let detail = gtk::Label::builder()
        .label("—")
        .css_classes(["caption", "dim-label"])
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["stat-tile"])
        .build();
    root.append(&overlay);
    root.append(&detail);
    GaugeCard {
        root,
        fraction,
        area,
        value,
        detail,
    }
}

/// A stylised fan drawn with Cairo — a seven-blade rotor with an open
/// donut hub spinning inside a static outer ring — that turns at a rate
/// proportional to the live RPM in `rpm` (one turn on screen per ~2,400
/// real revolutions, so the motion reads as speed without blurring).
/// Purely informative motion: it stands still when the speed is unknown
/// and honours the system-wide animations toggle.  Coloured Razer green
/// by the `fan-glyph` class in style.css — brand content, like the
/// Lighting swatches, not theming.
fn fan_glyph(rpm: &Rc<Cell<f64>>) -> gtk::DrawingArea {
    const SIZE: i32 = 28;
    let area = gtk::DrawingArea::new();
    area.set_content_width(SIZE);
    area.set_content_height(SIZE);
    area.set_halign(gtk::Align::Start);
    area.set_margin_top(3);
    area.add_css_class("fan-glyph");

    let angle = Rc::new(Cell::new(0.0_f64));
    area.set_draw_func({
        let angle = Rc::clone(&angle);
        move |area, cr, width, height| {
            let color = area.style_context().color();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64,
            );
            let radius = f64::from(width.min(height)) / 2.0;
            cr.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
            // Black face behind the rotor, matching the gauge dials
            // (content, not theming — see `.gauge-centre` in style.css).
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.arc(0.0, 0.0, radius * 0.88, 0.0, TAU);
            let _ = cr.fill();
            cr.set_source_rgba(
                color.red() as f64,
                color.green() as f64,
                color.blue() as f64,
                color.alpha() as f64,
            );
            // The outer ring stays still; only the rotor turns.
            cr.set_line_width(radius * 0.09);
            cr.arc(0.0, 0.0, radius * 0.93, 0.0, TAU);
            let _ = cr.stroke();
            cr.rotate(angle.get());
            cr.arc(0.0, 0.0, radius * 0.15, 0.0, TAU);
            let _ = cr.stroke();
            let r = radius * 0.72;
            for _ in 0..7 {
                cr.rotate(TAU / 7.0);
                cr.move_to(0.0, -r * 0.38);
                cr.curve_to(-r * 0.42, -r * 0.52, -r * 0.55, -r * 0.80, -r * 0.20, -r);
                cr.curve_to(r * 0.04, -r * 0.80, r * 0.05, -r * 0.55, 0.0, -r * 0.38);
                cr.close_path();
                let _ = cr.fill();
            }
        }
    });

    let animate =
        gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations());
    if animate {
        let rpm = Rc::clone(rpm);
        let last_frame = Cell::new(None::<i64>);
        area.add_tick_callback(move |area, clock| {
            let now = clock.frame_time();
            let rpm_now = rpm.get();
            if rpm_now > 0.0
                && let Some(previous) = last_frame.get()
            {
                let elapsed = (now - previous) as f64 / 1_000_000.0;
                angle.set((angle.get() + elapsed * rpm_now / 2400.0 * TAU) % TAU);
                area.queue_draw();
            }
            last_frame.set(Some(now));
            glib::ControlFlow::Continue
        });
    }
    area
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

/// Wrap page content in a clamp + scroll.  Wider than the standard
/// `AdwPreferencesPage` 600 so the hero and the tile grid can breathe;
/// Overview-only — the control pages keep the preferences metrics.
fn scrolled(content: gtk::Box) -> gtk::Widget {
    let clamp = adw::Clamp::builder()
        .maximum_size(800)
        .tightening_threshold(600)
        .child(&content)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build()
        .upcast()
}
