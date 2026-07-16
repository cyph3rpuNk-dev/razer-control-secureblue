//! GTK4/libadwaita UI. Linux-only; see `main.rs` for the platform gate.

// adw::prelude re-exports gtk::prelude, so this covers both toolkits' traits.
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

const APP_ID: &str = "dev.cyph3rpunk.razer-control";

// Verified fan range and Razer Battery Health Optimizer limits for the Blade 14
// (2023). The daemon re-checks every value; these only bound the widgets so the
// UI cannot even offer an out-of-policy request.
const FAN_MIN_RPM: f64 = 2000.0;
const FAN_MAX_RPM: f64 = 5400.0;
const BHO_MIN_PERCENT: f64 = 50.0;
const BHO_MAX_PERCENT: f64 = 80.0;

pub fn run() -> std::process::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);
    let status = app.run();
    std::process::ExitCode::from(if status == gtk::glib::ExitCode::SUCCESS {
        0
    } else {
        1
    })
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("../resources/style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_ui(app: &adw::Application) {
    // Synapse reads as a dark UI; force it rather than following the desktop.
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let toast_overlay = adw::ToastOverlay::new();

    let (cooling, fan_combo, rpm_scale) = build_cooling_group(&toast_overlay);
    let (battery, bho_switch, limit_scale) = build_battery_group(&toast_overlay);
    let (status, status_row, power_row, transport_row) = build_status_group();

    // Fang-style shell: sidebar navigation, dashboard first.  GPU & Display
    // and Lighting pages join the list when the daemon grows those features;
    // no placeholder pages for capabilities that do not exist yet.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_named(&dashboard_page(), Some("dashboard"));
    stack.add_named(&preferences_page(&[&cooling, &status]), Some("performance"));
    stack.add_named(&preferences_page(&[&battery]), Some("battery"));
    stack.add_named(&display_page(&toast_overlay), Some("display"));
    toast_overlay.set_child(Some(&stack));

    // Fill the status rows once at launch.
    refresh(&status_row, &power_row, &transport_row);

    // Keep the manual-only controls insensitive until the relevant mode is on.
    rpm_scale.set_sensitive(fan_combo.selected() == 1);
    limit_scale.set_sensitive(bho_switch.is_active());

    let sidebar_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .vexpand(true)
        .build();
    for (title, icon) in [
        ("Dashboard", "view-grid-symbolic"),
        ("Performance", "power-profile-performance-symbolic"),
        ("Battery", "battery-good-symbolic"),
        ("Display", "video-display-symbolic"),
    ] {
        sidebar_list.append(&sidebar_row(title, icon));
    }
    let stack_pages = ["dashboard", "performance", "battery", "display"];
    sidebar_list.connect_row_selected(clone!(
        #[weak]
        stack,
        move |_, row| {
            if let Some(row) = row
                && let Some(name) = stack_pages.get(row.index() as usize)
            {
                stack.set_visible_child_name(name);
            }
        }
    ));
    sidebar_list.select_row(sidebar_list.row_at_index(0).as_ref());

    let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_box.append(&sidebar_list);
    sidebar_box.append(&sidebar_footer());

    let sidebar_toolbar = adw::ToolbarView::new();
    sidebar_toolbar.add_top_bar(&adw::HeaderBar::new());
    sidebar_toolbar.set_content(Some(&sidebar_box));

    let content_toolbar = adw::ToolbarView::new();
    content_toolbar.add_top_bar(&adw::HeaderBar::new());
    content_toolbar.set_content(Some(&toast_overlay));

    let split = adw::NavigationSplitView::builder()
        .sidebar(
            &adw::NavigationPage::builder()
                .title("Razer Control")
                .child(&sidebar_toolbar)
                .build(),
        )
        .content(
            &adw::NavigationPage::builder()
                .title("Razer Control")
                .child(&content_toolbar)
                .build(),
        )
        .min_sidebar_width(200.0)
        .max_sidebar_width(220.0)
        .build();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Razer Control")
        .default_width(980)
        .default_height(640)
        .content(&split)
        .build();
    window.present();
}

fn sidebar_row(title: &str, icon: &str) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.set_margin_start(6);
    row_box.append(&gtk::Image::from_icon_name(icon));
    row_box.append(&gtk::Label::new(Some(title)));
    gtk::ListBoxRow::builder().child(&row_box).build()
}

/// Device and daemon identity, pinned to the sidebar's bottom like Fang's
/// status footer.  The subtitle names the transport so a mock session can
/// never be mistaken for the real daemon.
fn sidebar_footer() -> gtk::Box {
    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .css_classes(["sidebar-footer"])
        .build();
    let dot = gtk::Label::builder()
        .label("\u{25CF}")
        .css_classes(["status-dot"])
        .valign(gtk::Align::Start)
        .build();
    let name = gtk::Label::builder()
        .label(razer_control_secureblue::BLADE_14_2023.name)
        .halign(gtk::Align::Start)
        .css_classes(["footer-title"])
        .build();
    let transport_label = gtk::Label::builder()
        .label(transport::label())
        .halign(gtk::Align::Start)
        .css_classes(["footer-subtitle"])
        .build();
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.append(&name);
    text.append(&transport_label);
    footer.append(&dot);
    footer.append(&text);
    footer
}

/// One page: an `adw::PreferencesPage` (which supplies Synapse's centered
/// column via its built-in clamp) holding the given card groups in order.
fn preferences_page(groups: &[&adw::PreferencesGroup]) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    for group in groups {
        page.add(*group);
    }
    page.upcast()
}

fn build_cooling_group(
    overlay: &adw::ToastOverlay,
) -> (adw::PreferencesGroup, adw::ComboRow, gtk::Scale) {
    let group = adw::PreferencesGroup::builder()
        .title("Cooling")
        .description("Manual fan control reverts to automatic on logout")
        .build();

    let fan_combo = adw::ComboRow::builder().title("Fan mode").build();
    fan_combo.set_model(Some(&gtk::StringList::new(&["Automatic", "Manual"])));
    apply_synapse_option_factory(&fan_combo);

    let rpm_scale = value_scale(FAN_MIN_RPM, FAN_MAX_RPM, 50.0, 3000.0);
    let rpm_box = labelled_scale("Manual speed (RPM)", &rpm_scale);

    let apply = accent_button("Apply");
    group.set_header_suffix(Some(&apply));
    group.add(&fan_combo);
    group.add(&rpm_box);

    fan_combo.connect_selected_notify(clone!(
        #[weak]
        rpm_scale,
        move |combo| rpm_scale.set_sensitive(combo.selected() == 1)
    ));
    apply.connect_clicked(clone!(
        #[weak]
        fan_combo,
        #[weak]
        rpm_scale,
        #[weak]
        overlay,
        move |_| {
            let line = if fan_combo.selected() == 0 {
                "fan auto".to_owned()
            } else {
                format!("fan manual {}", rpm_scale.value().round() as u16)
            };
            feedback(&overlay, transport::request(&line));
        }
    ));

    (group, fan_combo, rpm_scale)
}

fn build_battery_group(
    overlay: &adw::ToastOverlay,
) -> (adw::PreferencesGroup, adw::SwitchRow, gtk::Scale) {
    let group = adw::PreferencesGroup::builder().title("Battery").build();

    let bho_switch = adw::SwitchRow::builder()
        .title("Battery Health Optimizer")
        .subtitle("Cap charging to protect long-term battery health")
        .build();

    let limit_scale = value_scale(BHO_MIN_PERCENT, BHO_MAX_PERCENT, 5.0, 80.0);
    let limit_box = labelled_scale("Charge limit (%)", &limit_scale);

    let apply = accent_button("Apply");
    group.set_header_suffix(Some(&apply));
    group.add(&bho_switch);
    group.add(&limit_box);

    bho_switch.connect_active_notify(clone!(
        #[weak]
        limit_scale,
        move |sw| limit_scale.set_sensitive(sw.is_active())
    ));
    apply.connect_clicked(clone!(
        #[weak]
        bho_switch,
        #[weak]
        limit_scale,
        #[weak]
        overlay,
        move |_| {
            let line = if bho_switch.is_active() {
                format!("bho {}", limit_scale.value().round() as u8)
            } else {
                "bho off".to_owned()
            };
            feedback(&overlay, transport::request(&line));
        }
    ));

    (group, bho_switch, limit_scale)
}

fn build_status_group() -> (
    adw::PreferencesGroup,
    adw::ActionRow,
    adw::ActionRow,
    adw::ActionRow,
) {
    let group = adw::PreferencesGroup::builder()
        .title("System status")
        .build();

    let status_row = adw::ActionRow::builder()
        .title("Daemon")
        .subtitle("—")
        .build();
    let power_row = adw::ActionRow::builder()
        .title("Power source")
        .subtitle("—")
        .build();
    let transport_row = adw::ActionRow::builder()
        .title("Transport")
        .subtitle("—")
        .build();

    let refresh_button = gtk::Button::with_label("Refresh");
    group.set_header_suffix(Some(&refresh_button));
    group.add(&status_row);
    group.add(&power_row);
    group.add(&transport_row);

    refresh_button.connect_clicked(clone!(
        #[weak]
        status_row,
        #[weak]
        power_row,
        #[weak]
        transport_row,
        move |_| refresh(&status_row, &power_row, &transport_row)
    ));

    (group, status_row, power_row, transport_row)
}

/// Display page: session-level controls that never touch the daemon.
/// Refresh rate goes through kscreen-doctor (KDE); panel brightness through
/// logind's SetBrightness D-Bus call, which grants the seat owner write
/// access without any privilege escalation.  Each group appears only when
/// its mechanism exists on this session.
fn display_page(overlay: &adw::ToastOverlay) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    let mut any = false;
    if let Some(group) = refresh_rate_group(overlay) {
        page.add(&group);
        any = true;
    }
    if let Some(group) = brightness_group() {
        page.add(&group);
        any = true;
    }
    if !any {
        page.add(
            &adw::PreferencesGroup::builder()
                .title("Display")
                .description(
                    "No controllable displays in this session: kscreen-doctor (KDE) \
                     and /sys/class/backlight are both unavailable.",
                )
                .build(),
        );
    }
    page.upcast()
}

struct DisplayMode {
    id: String,
    label: String,
    current: bool,
}

/// `kscreen-doctor -o` for the first enabled output: mode tokens look like
/// `id:WxH@Hz` with `*` marking the current mode and `!` the preferred one.
fn kscreen_modes() -> Option<(String, Vec<DisplayMode>)> {
    let output = std::process::Command::new("kscreen-doctor")
        .arg("-o")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.first() != Some(&"Output:") || !line.contains("enabled") {
            continue;
        }
        let name = (*tokens.get(2)?).to_owned();
        let mut modes = Vec::new();
        let mut in_modes = false;
        for token in &tokens {
            if *token == "Modes:" {
                in_modes = true;
                continue;
            }
            if in_modes {
                if !token.contains('@') || !token.contains(':') {
                    break;
                }
                let current = token.contains('*');
                let cleaned = token.replace(['*', '!'], "");
                let (id, label) = cleaned.split_once(':')?;
                modes.push(DisplayMode {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    current,
                });
            }
        }
        if !modes.is_empty() {
            return Some((name, modes));
        }
    }
    None
}

fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            if character.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if character == '\u{1b}' {
            in_escape = true;
        } else {
            result.push(character);
        }
    }
    result
}

fn refresh_rate_group(overlay: &adw::ToastOverlay) -> Option<adw::PreferencesGroup> {
    let (output_name, modes) = kscreen_modes()?;
    let group = adw::PreferencesGroup::builder()
        .title("Refresh rate")
        .description(format!("Active display: {output_name}"))
        .build();

    let labels: Vec<&str> = modes.iter().map(|mode| mode.label.as_str()).collect();
    let combo = adw::ComboRow::builder().title("Mode").build();
    combo.set_model(Some(&gtk::StringList::new(&labels)));
    apply_synapse_option_factory(&combo);
    if let Some(current) = modes.iter().position(|mode| mode.current) {
        combo.set_selected(current as u32);
    }

    let apply = accent_button("Apply");
    group.set_header_suffix(Some(&apply));
    group.add(&combo);

    let mode_ids: Vec<String> = modes.iter().map(|mode| mode.id.clone()).collect();
    apply.connect_clicked(clone!(
        #[weak]
        combo,
        #[weak]
        overlay,
        move |_| {
            let Some(id) = mode_ids.get(combo.selected() as usize) else {
                return;
            };
            let result = std::process::Command::new("kscreen-doctor")
                .arg(format!("output.{output_name}.mode.{id}"))
                .status();
            feedback(
                &overlay,
                match result {
                    Ok(status) if status.success() => Ok("ok refresh rate applied".to_owned()),
                    Ok(status) => Err(format!("kscreen-doctor exited with {status}")),
                    Err(error) => Err(format!("cannot run kscreen-doctor: {error}")),
                },
            );
        }
    ));
    Some(group)
}

/// First backlight device: (name, current, max).
fn backlight_device() -> Option<(String, u32, u32)> {
    let entry = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()?;
    let name = entry.file_name().to_string_lossy().into_owned();
    let read_u32 = |file: &str| -> Option<u32> {
        std::fs::read_to_string(entry.path().join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some((name, read_u32("brightness")?, read_u32("max_brightness")?))
}

fn brightness_group() -> Option<adw::PreferencesGroup> {
    let (device, current, max) = backlight_device()?;
    let group = adw::PreferencesGroup::builder()
        .title("Panel brightness")
        .build();
    let scale = value_scale(
        0.0,
        max as f64,
        (max as f64 / 100.0).max(1.0),
        current as f64,
    );
    group.add(&labelled_scale("Backlight", &scale));

    scale.connect_value_changed(move |scale| {
        let value = scale.value().round() as u32;
        let connection =
            match gtk::gio::bus_get_sync(gtk::gio::BusType::System, gtk::gio::Cancellable::NONE) {
                Ok(connection) => connection,
                Err(error) => {
                    eprintln!("system bus unavailable: {error}");
                    return;
                }
            };
        if let Err(error) = connection.call_sync(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            Some(&("backlight", device.as_str(), value).to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            1000,
            gtk::gio::Cancellable::NONE,
        ) {
            eprintln!("SetBrightness failed: {error}");
        }
    });
    Some(group)
}

const SPARKLINE_CAPACITY: usize = 90;
const GAUGE_MIN_C: f64 = 30.0;
const GAUGE_MAX_C: f64 = 100.0;

/// Fang-style dashboard: gauge and stat cards over a 90-second history
/// chart and an active-profile bar, fed by the daemon's read-only
/// `telemetry` request at 1 Hz.  Pure display — no control lives here.
fn dashboard_page() -> gtk::Widget {
    let cpu_value = Rc::new(Cell::new(None::<f64>));
    let history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(
        SPARKLINE_CAPACITY,
    )));

    let (gauge_card, gauge_area, gauge_value_label) = build_gauge_card(Rc::clone(&cpu_value));
    let (fan_card, fan_value_label) = build_stat_card("FAN TARGET", "RPM");
    let (power_card, power_value_label) = build_stat_card("POWER SOURCE", "");

    let cards = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();
    cards.append(&gauge_card);
    cards.append(&fan_card);
    cards.append(&power_card);

    let (chart_card, chart_area, chart_value_label) =
        build_sparkline_card("CPU TEMPERATURE — 90 S", Rc::clone(&history));

    let (profile_bar, profile_value_label, profile_detail_label) = build_profile_bar();

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    page.append(&cards);
    page.append(&chart_card);
    page.append(&profile_bar);

    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let telemetry = request_fields("telemetry");
        let status = request_fields("status");

        let cpu = telemetry
            .get("cpu_temp")
            .and_then(|value| value.parse::<f64>().ok());
        cpu_value.set(cpu);
        gauge_value_label.set_text(&cpu.map_or("—".to_owned(), |t| format!("{t:.0}")));
        if let Some(temperature) = cpu {
            let mut samples = history.borrow_mut();
            if samples.len() == SPARKLINE_CAPACITY {
                samples.pop_front();
            }
            samples.push_back(temperature);
            chart_value_label.set_text(&format!("{temperature:.0} °C"));
        }
        fan_value_label.set_text(
            telemetry
                .get("fan_rpm")
                .filter(|value| *value != "none")
                .map_or("—", String::as_str),
        );
        power_value_label.set_text(match telemetry.get("power").map(String::as_str) {
            Some("ac") => "Plugged in",
            Some("battery") => "On battery",
            _ => "—",
        });
        profile_value_label.set_text(&describe_status_fan(status.get("fan")));
        profile_detail_label.set_text(&format!(
            "backend: {}{}",
            status.get("backend").map_or("—", String::as_str),
            if telemetry.get("simulated").map(String::as_str) == Some("true") {
                " · simulated"
            } else {
                ""
            }
        ));
        gauge_area.queue_draw();
        chart_area.queue_draw();
        glib::ControlFlow::Continue
    });

    page.upcast()
}

/// One daemon request parsed into its `key=value` fields; empty on error.
fn request_fields(request: &str) -> HashMap<String, String> {
    let Ok(line) = transport::request(request) else {
        return HashMap::new();
    };
    line.trim_start_matches("ok ")
        .split_whitespace()
        .filter_map(|token| {
            token
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn describe_status_fan(fan: Option<&String>) -> String {
    match fan.map(String::as_str) {
        Some("auto") => "Fan: automatic".to_owned(),
        Some(manual) if manual.starts_with("manual:") => {
            format!("Fan: manual {} RPM", manual.trim_start_matches("manual:"))
        }
        _ => "Fan: —".to_owned(),
    }
}

fn dash_card() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["dash-card"])
        .build()
}

fn dash_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Center)
        .css_classes(["dash-label"])
        .build()
}

/// Segmented arc gauge in the Fang style: 28 ticks over 270 degrees, lit
/// green up to the current fraction of the 30–100 °C span.
fn build_gauge_card(value: Rc<Cell<Option<f64>>>) -> (gtk::Box, gtk::DrawingArea, gtk::Label) {
    let area = gtk::DrawingArea::builder()
        .content_width(150)
        .content_height(150)
        .halign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |_, cr, width, height| {
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        let radius = width.min(height) as f64 / 2.0 - 8.0;
        let fraction = value
            .get()
            .map(|t| ((t - GAUGE_MIN_C) / (GAUGE_MAX_C - GAUGE_MIN_C)).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let ticks = 28;
        let start = 0.75 * std::f64::consts::PI;
        let sweep = 1.5 * std::f64::consts::PI;
        cr.set_line_width(7.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        for tick in 0..ticks {
            let position = tick as f64 / (ticks - 1) as f64;
            let angle = start + position * sweep;
            if position <= fraction && value.get().is_some() {
                cr.set_source_rgb(0.27, 0.84, 0.17);
            } else {
                cr.set_source_rgb(0.16, 0.16, 0.16);
            }
            let inner = radius - 9.0;
            cr.move_to(
                center_x + inner * angle.cos(),
                center_y + inner * angle.sin(),
            );
            cr.line_to(
                center_x + radius * angle.cos(),
                center_y + radius * angle.sin(),
            );
            let _ = cr.stroke();
        }
    });

    let value_label = gtk::Label::builder()
        .label("—")
        .css_classes(["dash-value"])
        .build();
    let unit_label = gtk::Label::builder()
        .label("°C")
        .css_classes(["dash-unit"])
        .build();
    let center = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();
    center.append(&value_label);
    center.append(&unit_label);

    let overlay = gtk::Overlay::builder().child(&area).build();
    overlay.add_overlay(&center);

    let card = dash_card();
    card.append(&overlay);
    card.append(&dash_label("CPU PACKAGE"));
    (card, area, value_label)
}

fn build_stat_card(caption: &str, unit: &str) -> (gtk::Box, gtk::Label) {
    let value_label = gtk::Label::builder()
        .label("—")
        .css_classes(["dash-value"])
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let card = dash_card();
    card.append(&value_label);
    if !unit.is_empty() {
        card.append(&dash_label(unit));
    }
    card.append(&dash_label(caption));
    (card, value_label)
}

/// 90-sample line chart drawn with cairo; scales to the data's own range
/// with a little headroom so idle temperatures do not flatline at the edge.
fn build_sparkline_card(
    caption: &str,
    history: Rc<RefCell<VecDeque<f64>>>,
) -> (gtk::Box, gtk::DrawingArea, gtk::Label) {
    let area = gtk::DrawingArea::builder()
        .content_height(90)
        .hexpand(true)
        .build();
    area.set_draw_func(move |_, cr, width, height| {
        let samples = history.borrow();
        if samples.len() < 2 {
            return;
        }
        let low = samples.iter().cloned().fold(f64::INFINITY, f64::min) - 2.0;
        let high = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 2.0;
        let x_step = width as f64 / (SPARKLINE_CAPACITY - 1) as f64;
        let project = |sample: f64| {
            let normalised = (sample - low) / (high - low);
            height as f64 - normalised * (height as f64 - 8.0) - 4.0
        };
        cr.set_source_rgba(0.27, 0.84, 0.17, 0.15);
        cr.move_to(0.0, height as f64);
        for (index, sample) in samples.iter().enumerate() {
            cr.line_to(index as f64 * x_step, project(*sample));
        }
        cr.line_to((samples.len() - 1) as f64 * x_step, height as f64);
        let _ = cr.fill();
        cr.set_source_rgb(0.27, 0.84, 0.17);
        cr.set_line_width(2.0);
        for (index, sample) in samples.iter().enumerate() {
            let x = index as f64 * x_step;
            let y = project(*sample);
            if index == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        let _ = cr.stroke();
    });

    let caption_label = dash_label(caption);
    caption_label.set_halign(gtk::Align::Start);
    let value_label = gtk::Label::builder()
        .label("—")
        .halign(gtk::Align::End)
        .hexpand(true)
        .css_classes(["chart-value"])
        .build();
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.append(&caption_label);
    header.append(&value_label);

    let card = dash_card();
    card.append(&header);
    card.append(&area);
    (card, area, value_label)
}

fn build_profile_bar() -> (gtk::Box, gtk::Label, gtk::Label) {
    let caption = dash_label("ACTIVE PROFILE");
    caption.set_halign(gtk::Align::Start);
    let value_label = gtk::Label::builder()
        .label("—")
        .css_classes(["profile-value"])
        .build();
    let detail_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::End)
        .hexpand(true)
        .css_classes(["footer-subtitle"])
        .build();
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .css_classes(["dash-card"])
        .build();
    bar.append(&caption);
    bar.append(&value_label);
    bar.append(&detail_label);
    (bar, value_label, detail_label)
}

/// Synapse dropdowns mark the chosen option with a green row, not a
/// checkmark.  GTK's `:selected` in a ComboRow popover tracks the list
/// cursor rather than the chosen value (verified with a widget-tree dump),
/// so a stylesheet alone cannot express this: instead the popover gets a
/// custom factory that tags the chosen row with the `chosen-option` class,
/// which the stylesheet paints green.
fn apply_synapse_option_factory(combo: &adw::ComboRow) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, object| {
        let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["option-label"])
            .build();
        item.set_child(Some(&label));
    });
    let combo_weak = combo.downgrade();
    factory.connect_bind(move |_, object| {
        let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(option) = item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let Some(label) = item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        label.set_text(&option.string());
        if let Some(combo) = combo_weak.upgrade() {
            mark_option_row(&label, combo.selected() == item.position());
        }
    });
    combo.set_list_factory(Some(&factory));
    // Rows stay realised between popover openings, so re-tag them whenever
    // the choice changes.
    combo.connect_selected_notify(resync_option_rows);
}

fn mark_option_row(label: &gtk::Label, chosen: bool) {
    if let Some(row) = label.parent() {
        if chosen {
            row.add_css_class("chosen-option");
        } else {
            row.remove_css_class("chosen-option");
        }
    }
}

fn resync_option_rows(combo: &adw::ComboRow) {
    let chosen = combo
        .selected_item()
        .and_downcast::<gtk::StringObject>()
        .map(|object| object.string());
    let mut stack = vec![combo.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && label.has_css_class("option-label")
        {
            mark_option_row(label, chosen.as_deref() == Some(label.text().as_str()));
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            stack.push(current.clone());
            child = current.next_sibling();
        }
    }
}

fn value_scale(min: f64, max: f64, step: f64, initial: f64) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    scale.set_value(initial);
    scale.set_draw_value(true);
    scale.set_round_digits(0);
    scale.set_hexpand(true);
    scale
}

fn labelled_scale(label: &str, scale: &gtk::Scale) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 6);
    container.set_margin_top(6);
    container.set_margin_bottom(6);
    container.set_margin_start(12);
    container.set_margin_end(12);
    let caption = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .build();
    caption.add_css_class("dim-label");
    container.append(&caption);
    container.append(scale);
    container
}

fn accent_button(label: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("suggested-action");
    button.set_valign(gtk::Align::Center);
    button
}

fn refresh(
    status_row: &adw::ActionRow,
    power_row: &adw::ActionRow,
    transport_row: &adw::ActionRow,
) {
    let status = match transport::request("status") {
        Ok(response) => response,
        Err(error) => format!("err {error}"),
    };
    status_row.set_subtitle(&status);
    power_row.set_subtitle(power_source());
    transport_row.set_subtitle(transport::label());
}

fn feedback(overlay: &adw::ToastOverlay, result: Result<String, String>) {
    let text = match result {
        Ok(response) => response,
        Err(error) => format!("err {error}"),
    };
    overlay.add_toast(adw::Toast::new(&text));
}

/// Current power source, read the same way the Tauri shell did. Read-only:
/// following AC/battery transitions with profiles is daemon work, not the GUI's.
fn power_source() -> &'static str {
    let Ok(manager) = starship_battery::Manager::new() else {
        return "Unknown";
    };
    let Ok(batteries) = manager.batteries() else {
        return "Unknown";
    };
    for battery in batteries.flatten() {
        use starship_battery::State;
        return match battery.state() {
            State::Discharging | State::Empty => "On battery",
            State::Charging | State::Full => "Plugged in",
            _ => "Unknown",
        };
    }
    "Unknown"
}

/// IPC transport: one request line, one response line. Talks to the real
/// per-user daemon socket unless `RAZER_CONTROL_MOCK=1` is set, in which case it
/// drives an in-process copy of the identical daemon core with the dry-run
/// backend — exactly the split the Tauri shell used.
mod transport {
    use razer_control_secureblue::BLADE_14_2023;
    use razer_control_secureblue::backend::DryRunBackend;
    use razer_control_secureblue::daemon::Daemon;
    use std::sync::{Mutex, OnceLock};

    pub fn request(line: &str) -> Result<String, String> {
        if std::env::var_os("RAZER_CONTROL_MOCK").is_none() {
            return razer_control_secureblue::daemon_unix::send(line);
        }
        mock(line)
    }

    pub fn label() -> &'static str {
        if std::env::var_os("RAZER_CONTROL_MOCK").is_none() {
            "daemon socket"
        } else {
            "in-process dry run"
        }
    }

    fn mock(line: &str) -> Result<String, String> {
        static MOCK: OnceLock<Mutex<Daemon<DryRunBackend>>> = OnceLock::new();
        let daemon = MOCK.get_or_init(|| {
            Mutex::new(
                Daemon::new(BLADE_14_2023, DryRunBackend::default(), false)
                    .with_simulated_telemetry(),
            )
        });
        Ok(daemon
            .lock()
            .map_err(|error| error.to_string())?
            .handle_line(line))
    }
}
