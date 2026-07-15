//! GTK4/libadwaita UI. Linux-only; see `main.rs` for the platform gate.

use adw::prelude::*;
use gtk::glib::clone;
use gtk::prelude::*;

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
    let page = adw::PreferencesPage::new();

    let (cooling, fan_combo, rpm_scale) = build_cooling_group(&toast_overlay);
    let (battery, bho_switch, limit_scale) = build_battery_group(&toast_overlay);
    let (status, status_row, power_row, transport_row) = build_status_group();

    page.add(&cooling);
    page.add(&battery);
    page.add(&status);
    toast_overlay.set_child(Some(&page));

    // Fill the status rows once at launch.
    refresh(&status_row, &power_row, &transport_row);

    // Keep the manual-only controls insensitive until the relevant mode is on.
    rpm_scale.set_sensitive(fan_combo.selected() == 1);
    limit_scale.set_sensitive(bho_switch.is_active());

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Razer Control")
        .default_width(440)
        .default_height(700)
        .content(&toolbar)
        .build();
    window.present();
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
            Mutex::new(Daemon::new(BLADE_14_2023, DryRunBackend::default(), false))
        });
        Ok(daemon
            .lock()
            .map_err(|error| error.to_string())?
            .handle_line(line))
    }
}
