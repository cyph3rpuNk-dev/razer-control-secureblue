//! GTK4/libadwaita UI. Linux-only; see `main.rs` for the platform gate.
//!
//! The visual language follows fang-razer-linux (GPL-2.0): near-black
//! panes, hairline-bordered cards, Razer-green accents, and monospace
//! uppercase captions. Every panel is a "Fang card" under a small
//! section label; choices are segmented button groups, not dropdowns.

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

// Cairo colors for the custom-drawn widgets, matching the stylesheet:
// Razer green #44d62c on near-black card grey.
const ACCENT: (f64, f64, f64) = (0.267, 0.839, 0.173);
const TICK_OFF: (f64, f64, f64) = (0.13, 0.16, 0.14);

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

        // Installed builds find the icon in hicolor; development builds
        // (cargo run) get the repo's packaging copy added to the search
        // path, so the window and taskbar icon work in both.
        let icon_theme = gtk::IconTheme::for_display(&display);
        let dev_icons = concat!(env!("CARGO_MANIFEST_DIR"), "/../packaging/icons");
        if std::path::Path::new(dev_icons).is_dir() {
            icon_theme.add_search_path(dev_icons);
        }
    }
    gtk::Window::set_default_icon_name("razer-control-desktop");
}

fn build_ui(app: &adw::Application) {
    // Fang is a dark UI, full stop; force it rather than following the desktop.
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);

    let toast_overlay = adw::ToastOverlay::new();

    // Fang-style shell: sidebar navigation, dashboard first.  GPU & Display
    // and Lighting pages join the list when the daemon grows those features;
    // no placeholder pages for capabilities that do not exist yet.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    stack.add_named(&dashboard_page(), Some("dashboard"));
    stack.add_named(&performance_page(&toast_overlay), Some("performance"));
    stack.add_named(&battery_page(&toast_overlay), Some("battery"));
    stack.add_named(&display_page(&toast_overlay), Some("display"));
    stack.add_named(&lighting_page(&toast_overlay), Some("lighting"));
    toast_overlay.set_child(Some(&stack));

    let pages: [(&str, &str, &str); 5] = [
        ("dashboard", "Dashboard", "view-grid-symbolic"),
        (
            "performance",
            "Performance",
            "power-profile-performance-symbolic",
        ),
        ("battery", "Battery", "battery-good-symbolic"),
        ("display", "Display & GPU", "video-display-symbolic"),
        ("lighting", "Lighting", "input-keyboard-symbolic"),
    ];

    let sidebar_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .vexpand(true)
        .build();
    for (_, title, icon) in pages {
        sidebar_list.append(&sidebar_row(title, icon));
    }
    sidebar_list.select_row(sidebar_list.row_at_index(0).as_ref());

    let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar_box.append(&sidebar_list);
    sidebar_box.append(&sidebar_footer());

    // Fang shows no titlebar text: the sidebar header carries the brand
    // mark and the content header carries the uppercase page title.
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_show_title(false);
    sidebar_header.pack_start(&brand_mark());

    let page_title = gtk::Label::builder()
        .label("DASHBOARD")
        .css_classes(["page-title"])
        .build();
    let content_header = adw::HeaderBar::new();
    content_header.set_show_title(false);
    content_header.pack_start(&page_title);

    let sidebar_toolbar = adw::ToolbarView::builder()
        .css_classes(["fang-sidebar"])
        .build();
    sidebar_toolbar.add_top_bar(&sidebar_header);
    sidebar_toolbar.set_content(Some(&sidebar_box));

    let content_toolbar = adw::ToolbarView::builder()
        .css_classes(["fang-content"])
        .build();
    content_toolbar.add_top_bar(&content_header);
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

    // Row selection switches the page and retitles the content header; when
    // the split view is collapsed (narrow window) it must also push the
    // content pane into view.
    sidebar_list.connect_row_selected(clone!(
        #[weak]
        stack,
        #[weak]
        split,
        #[weak]
        page_title,
        move |_, row| {
            if let Some(row) = row
                && let Some((name, title, _)) = pages.get(row.index() as usize)
            {
                stack.set_visible_child_name(name);
                page_title.set_text(&title.to_uppercase());
                if split.is_collapsed() {
                    split.set_show_content(true);
                }
            }
        }
    ));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Razer Control")
        .icon_name("razer-control-desktop")
        .default_width(1100)
        .default_height(680)
        .content(&split)
        .build();

    // Keep the window freely resizable: below 760 px the sidebar collapses
    // into an overlay instead of forcing a large minimum width.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        760.0,
        adw::LengthUnit::Px,
    ));
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    window.present();

    // WSLg's window manager ignores the initial-size request and maps every
    // window at 640x480, which lands under the breakpoint and shows only the
    // collapsed sidebar.  Real desktops honor the request; under WSL maximize
    // instead — and only after present(): maximizing the unmapped window
    // makes WSLg's compositor kill the connection and the app exits with 1.
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        window.maximize();
    }
}

/// App icon plus wide-tracked wordmark, top-left like Fang's "V FANG".
fn brand_mark() -> gtk::Box {
    let brand = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name("razer-control-desktop");
    icon.set_pixel_size(18);
    let name = gtk::Label::builder()
        .label("RAZER CONTROL")
        .css_classes(["brand-name"])
        .build();
    brand.append(&icon);
    brand.append(&name);
    brand
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

/// One content page: section labels and cards in a scrollable column with
/// Fang's 24 px gutters.
fn fang_page(children: &[gtk::Widget]) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    for child in children {
        page.append(child);
    }
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&page)
        .build()
        .upcast()
}

/// Uppercase mono section header above a card ("FAN MODE").
fn section_label(text: &str) -> gtk::Widget {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .css_classes(["section-label"])
        .build()
        .upcast()
}

fn fang_card(spacing: i32) -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(spacing)
        .css_classes(["fang-card"])
        .build()
}

/// Muted explanatory line inside a card.
fn note_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["note-label"])
        .build()
}

/// Fang's segmented choice group: linked toggle buttons acting as radios,
/// with `initial` checked before any handler can run.
fn segmented(options: &[&str], initial: usize) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let container = gtk::Box::builder()
        .css_classes(["linked", "fang-seg"])
        .halign(gtk::Align::Start)
        .build();
    let mut buttons: Vec<gtk::ToggleButton> = Vec::with_capacity(options.len());
    for option in options {
        let button = gtk::ToggleButton::with_label(option);
        if let Some(first) = buttons.first() {
            button.set_group(Some(first));
        }
        container.append(&button);
        buttons.push(button);
    }
    if let Some(button) = buttons.get(initial) {
        button.set_active(true);
    }
    (container, buttons)
}

/// Performance page, in Fang's order: profile cards, CPU/GPU power levels,
/// fan mode, power automation, system status.  The profile controls stay
/// insensitive unless the daemon reports `experimental=true` — they drive
/// EC commands that Phase 3 has not yet verified on hardware.
fn performance_page(overlay: &adw::ToastOverlay) -> gtk::Widget {
    let status = request_fields("status");
    let experimental = status.get("experimental").map(String::as_str) == Some("true");
    let current_profile = status
        .get("profile")
        .map_or("balanced", String::as_str)
        .to_owned();
    let (profile_row, power_card) = build_profile_ui(overlay, experimental, &current_profile);
    let fan_card = fang_card(14);
    let (seg_box, seg) = segmented(&["AUTO", "MANUAL"], 0);
    let rpm_scale = value_scale(FAN_MIN_RPM, FAN_MAX_RPM, 50.0, 3000.0);
    rpm_scale.set_sensitive(false);
    let rpm_box = labelled_scale("MANUAL SPEED — RPM", &rpm_scale);
    let apply = accent_button("APPLY");
    apply.set_halign(gtk::Align::End);
    fan_card.append(&seg_box);
    fan_card.append(&rpm_box);
    fan_card.append(&note_label(
        "Manual fan control reverts to automatic on logout.",
    ));
    fan_card.append(&apply);

    let manual = seg[1].clone();
    manual.connect_toggled(clone!(
        #[weak]
        rpm_scale,
        move |button| rpm_scale.set_sensitive(button.is_active())
    ));
    apply.connect_clicked(clone!(
        #[weak]
        manual,
        #[weak]
        rpm_scale,
        #[weak]
        overlay,
        move |_| {
            let line = if manual.is_active() {
                format!("fan manual {}", rpm_scale.value().round() as u16)
            } else {
                "fan auto".to_owned()
            };
            feedback(&overlay, transport::request(&line));
        }
    ));

    let automation_card = build_automation_card(overlay);

    let (status_card, daemon_value, power_value, transport_value) = build_status_card();
    refresh(&daemon_value, &power_value, &transport_value);

    let mut children: Vec<gtk::Widget> = vec![section_label("PROFILE"), profile_row.upcast()];
    if !experimental {
        children.push(
            note_label(
                "Profiles are locked: they send EC commands not yet verified on this \
                 machine (Phase 3). Start the daemon with --experimental to enable them.",
            )
            .upcast(),
        );
    }
    children.extend([
        power_card.upcast(),
        section_label("FAN MODE"),
        fan_card.upcast(),
        section_label("POWER AUTOMATION"),
        automation_card.upcast(),
        section_label("SYSTEM STATUS"),
        status_card.upcast(),
    ]);
    fang_page(&children)
}

/// Fang's profile row and CPU/GPU power card.  Every click sends one
/// `profile …` request; the daemon validates and the dry-run or hidraw
/// backend does the rest.  The power card only responds while Custom is the
/// active profile, exactly like Fang.
fn build_profile_ui(
    overlay: &adw::ToastOverlay,
    experimental: bool,
    current_profile: &str,
) -> (gtk::Box, gtk::Box) {
    const CARDS: [(&str, &str, &str); 4] = [
        (
            "Silent",
            "Lowest fan noise, capped power. For late nights and libraries.",
            "power-profile-power-saver-symbolic",
        ),
        (
            "Balanced",
            "The everyday default. Sensible power, sensible acoustics.",
            "power-profile-balanced-symbolic",
        ),
        (
            "Gaming",
            "Full tilt. Maximum sustained CPU + GPU power.",
            "power-profile-performance-symbolic",
        ),
        (
            "Custom",
            "Pick CPU and GPU power levels yourself.",
            "emblem-system-symbolic",
        ),
    ];
    const LEVEL_TOKENS: [&str; 4] = ["low", "medium", "high", "boost"];

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .build();
    let mut cards: Vec<gtk::ToggleButton> = Vec::with_capacity(CARDS.len());
    for (title, description, icon) in CARDS {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.append(&gtk::Image::from_icon_name(icon));
        let dot = gtk::Label::builder()
            .label("\u{25CF}")
            .halign(gtk::Align::End)
            .hexpand(true)
            .valign(gtk::Align::Start)
            .css_classes(["profile-dot"])
            .build();
        header.append(&dot);
        let title_label = gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["profile-title"])
            .build();
        let desc_label = gtk::Label::builder()
            .label(description)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["profile-desc"])
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&header);
        content.append(&title_label);
        content.append(&desc_label);
        let card = gtk::ToggleButton::builder()
            .child(&content)
            .css_classes(["profile-card"])
            .build();
        if let Some(first) = cards.first() {
            card.set_group(Some(first));
        }
        row.append(&card);
        cards.push(card);
    }

    let cpu_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    cpu_col.append(&{
        let caption = dash_label("CPU POWER");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    let (cpu_seg_box, cpu_seg) = segmented(&["LOW", "MEDIUM", "HIGH", "BOOST"], 1);
    cpu_col.append(&cpu_seg_box);
    let gpu_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    gpu_col.append(&{
        let caption = dash_label("GPU POWER");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    let (gpu_seg_box, gpu_seg) = segmented(&["LOW", "MEDIUM", "HIGH"], 1);
    gpu_col.append(&gpu_seg_box);
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 48);
    columns.append(&cpu_col);
    columns.append(&gpu_col);
    let power_card = fang_card(10);
    power_card.append(&columns);

    // Initial selection mirrors the daemon's current profile, boost
    // segments included, before any handler is connected.
    let initial = match current_profile {
        "silent" => 0,
        "gaming" => 2,
        custom if custom.starts_with("custom") => 3,
        _ => 1,
    };
    if let Some(razer_control_secureblue::Profile::Custom { cpu, gpu }) =
        razer_control_secureblue::ipc::parse_profile(current_profile)
    {
        cpu_seg[cpu.wire_value() as usize].set_active(true);
        gpu_seg[gpu.wire_value() as usize].set_active(true);
    }
    cards[initial].set_active(true);
    row.set_sensitive(experimental);
    power_card.set_sensitive(experimental && initial == 3);

    let checked = |buttons: &[gtk::ToggleButton]| {
        buttons
            .iter()
            .position(|button| button.is_active())
            .unwrap_or(0)
    };
    let compose_custom = {
        let cpu_seg = cpu_seg.clone();
        let gpu_seg = gpu_seg.clone();
        move || {
            format!(
                "profile custom cpu {} gpu {}",
                LEVEL_TOKENS[checked(&cpu_seg)],
                LEVEL_TOKENS[checked(&gpu_seg)]
            )
        }
    };

    let custom_card = cards[3].clone();
    for (index, card) in cards.iter().enumerate() {
        let compose_custom = compose_custom.clone();
        card.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            power_card,
            #[weak]
            custom_card,
            move |card| {
                power_card.set_sensitive(experimental && custom_card.is_active());
                if !card.is_active() {
                    return;
                }
                let line = match index {
                    0 => "profile silent".to_owned(),
                    1 => "profile balanced".to_owned(),
                    2 => "profile gaming".to_owned(),
                    _ => compose_custom(),
                };
                feedback(&overlay, transport::request(&line));
            }
        ));
    }
    for segment in cpu_seg.iter().chain(gpu_seg.iter()) {
        let compose_custom = compose_custom.clone();
        segment.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            custom_card,
            move |segment| {
                if segment.is_active() && custom_card.is_active() {
                    feedback(&overlay, transport::request(&compose_custom()));
                }
            }
        ));
    }

    (row, power_card)
}

/// Fang's power-automation card: a master OFF|ON, then one row per power
/// source with a live NOW badge and a FAN AUTO|QUIET choice.  QUIET pins
/// the fans at the verified 2000 RPM floor; every click applies instantly
/// through the daemon's `automation` rules, which persist across restarts.
fn build_automation_card(overlay: &adw::ToastOverlay) -> gtk::Box {
    let card = fang_card(14);

    // Current rules, so reopening the app shows what the daemon will do.
    let status = request_fields("status");
    let rule = |key: &str| status.get(key).map_or("off", String::as_str).to_owned();
    let ac_rule = rule("automation_ac");
    let battery_rule = rule("automation_battery");
    let enabled = ac_rule != "off" || battery_rule != "off";

    let description = note_label("Switch fan behaviour automatically when you plug in or unplug.");
    let (master_box, master) = segmented(&["OFF", "ON"], usize::from(enabled));
    master_box.set_halign(gtk::Align::End);
    master_box.set_hexpand(true);
    master_box.set_valign(gtk::Align::Start);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.append(&description);
    header.append(&master_box);
    card.append(&header);

    // One row: "ON AC  [NOW]        FAN  AUTO|QUIET".
    let source_row = |title: &str, initial_quiet: bool| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let caption = gtk::Label::builder()
            .label(title)
            .css_classes(["dash-label"])
            .build();
        let badge = gtk::Label::builder()
            .label("NOW")
            .css_classes(["now-badge"])
            .visible(false)
            .build();
        let fan_caption = gtk::Label::builder()
            .label("FAN")
            .halign(gtk::Align::End)
            .hexpand(true)
            .css_classes(["dash-label"])
            .build();
        let (seg_box, seg) = segmented(&["AUTO", "QUIET"], usize::from(initial_quiet));
        row.append(&caption);
        row.append(&badge);
        row.append(&fan_caption);
        row.append(&seg_box);
        row.set_sensitive(enabled);
        (row, seg, badge)
    };
    let (ac_row, ac_seg, ac_badge) = source_row("ON AC", ac_rule.starts_with("manual"));
    let (battery_row, battery_seg, battery_badge) =
        source_row("ON BATTERY", battery_rule.starts_with("manual"));
    card.append(&ac_row);
    card.append(&battery_row);
    card.append(&note_label(
        "Quiet pins the fans at the verified 2000 RPM floor.",
    ));

    let send_rule = |overlay: &adw::ToastOverlay, source: &str, quiet: bool| {
        let line = if quiet {
            format!("automation {source} fan manual {}", FAN_MIN_RPM as u16)
        } else {
            format!("automation {source} fan auto")
        };
        feedback(overlay, transport::request(&line));
    };

    // Per-source segments apply instantly, but only while automation is on.
    let master_on = master[1].clone();
    for (seg, source) in [(&ac_seg, "ac"), (&battery_seg, "battery")] {
        for (button, quiet) in [(&seg[0], false), (&seg[1], true)] {
            let source = source.to_owned();
            button.connect_toggled(clone!(
                #[weak]
                overlay,
                #[weak]
                master_on,
                move |button| {
                    if button.is_active() && master_on.is_active() {
                        send_rule(&overlay, &source, quiet);
                    }
                }
            ));
        }
    }

    // Master OFF clears both rules; ON re-applies the rows' selections.
    master[0].connect_toggled(clone!(
        #[weak]
        overlay,
        #[weak]
        ac_row,
        #[weak]
        battery_row,
        move |button| {
            if !button.is_active() {
                return;
            }
            ac_row.set_sensitive(false);
            battery_row.set_sensitive(false);
            let off = transport::request("automation ac off")
                .and_then(|_| transport::request("automation battery off"));
            feedback(&overlay, off);
        }
    ));
    let ac_quiet = ac_seg[1].clone();
    let battery_quiet = battery_seg[1].clone();
    master[1].connect_toggled(clone!(
        #[weak]
        overlay,
        #[weak]
        ac_row,
        #[weak]
        battery_row,
        #[weak]
        ac_quiet,
        #[weak]
        battery_quiet,
        move |button| {
            if !button.is_active() {
                return;
            }
            ac_row.set_sensitive(true);
            battery_row.set_sensitive(true);
            send_rule(&overlay, "ac", ac_quiet.is_active());
            send_rule(&overlay, "battery", battery_quiet.is_active());
        }
    ));

    // The NOW badge follows the live power source.
    glib::timeout_add_seconds_local(2, move || {
        let power = request_fields("telemetry");
        let on_ac = power.get("power").map(String::as_str) == Some("ac");
        ac_badge.set_visible(on_ac);
        battery_badge.set_visible(power.get("power").map(String::as_str) == Some("battery"));
        glib::ControlFlow::Continue
    });

    card
}

/// Battery page: Battery Health Optimizer as a Fang card — OFF|ON segments
/// and the charge-limit slider.
fn battery_page(overlay: &adw::ToastOverlay) -> gtk::Widget {
    let card = fang_card(14);
    let (seg_box, seg) = segmented(&["OFF", "ON"], 0);
    let limit_scale = value_scale(BHO_MIN_PERCENT, BHO_MAX_PERCENT, 5.0, 80.0);
    limit_scale.set_sensitive(false);
    let limit_box = labelled_scale("CHARGE LIMIT — %", &limit_scale);
    let apply = accent_button("APPLY");
    apply.set_halign(gtk::Align::End);
    card.append(&seg_box);
    card.append(&limit_box);
    card.append(&note_label(
        "Caps charging to protect long-term battery health.",
    ));
    card.append(&apply);

    let on_button = seg[1].clone();
    on_button.connect_toggled(clone!(
        #[weak]
        limit_scale,
        move |button| limit_scale.set_sensitive(button.is_active())
    ));
    apply.connect_clicked(clone!(
        #[weak]
        on_button,
        #[weak]
        limit_scale,
        #[weak]
        overlay,
        move |_| {
            let line = if on_button.is_active() {
                format!("bho {}", limit_scale.value().round() as u8)
            } else {
                "bho off".to_owned()
            };
            feedback(&overlay, transport::request(&line));
        }
    ));

    fang_page(&[section_label("BATTERY HEALTH OPTIMIZER"), card.upcast()])
}

/// Key/value rows for daemon, power source, and transport, with a quiet
/// REFRESH button — the one card that polls nothing on its own.
fn build_status_card() -> (gtk::Box, gtk::Label, gtk::Label, gtk::Label) {
    let card = fang_card(0);

    let row = |caption: &str| -> (gtk::Box, gtk::Label) {
        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        let caption = gtk::Label::builder()
            .label(caption)
            .halign(gtk::Align::Start)
            .css_classes(["dash-label"])
            .build();
        let value = gtk::Label::builder()
            .label("—")
            .halign(gtk::Align::End)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["status-value"])
            .build();
        container.append(&caption);
        container.append(&value);
        (container, value)
    };

    let (daemon_row, daemon_value) = row("DAEMON");
    let (power_row, power_value) = row("POWER SOURCE");
    let (transport_row, transport_value) = row("TRANSPORT");

    let refresh_button = gtk::Button::builder()
        .label("REFRESH")
        .halign(gtk::Align::End)
        .margin_top(8)
        .css_classes(["fang-ghost"])
        .build();

    card.append(&daemon_row);
    card.append(&power_row);
    card.append(&transport_row);
    card.append(&refresh_button);

    refresh_button.connect_clicked(clone!(
        #[weak]
        daemon_value,
        #[weak]
        power_value,
        #[weak]
        transport_value,
        move |_| refresh(&daemon_value, &power_value, &transport_value)
    ));

    (card, daemon_value, power_value, transport_value)
}

/// GPU-mode switching, in Fang's manner: delegate to whichever supported
/// tool is installed.  supergfxctl talks to its own privileged daemon;
/// prime-select and envycontrol need root, which the app requests per
/// switch through pkexec (polkit) — this codebase never runs a root
/// daemon of its own.  Under RAZER_CONTROL_MOCK=1 an in-memory tool lets
/// the cards be exercised with zero system effect.
mod gpu {
    use std::process::Command;
    use std::sync::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Mode {
        Integrated,
        Hybrid,
        Dedicated,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Tool {
        SuperGfx,
        PrimeSelect,
        EnvyControl,
        Mock,
    }

    static MOCK_MODE: Mutex<Mode> = Mutex::new(Mode::Hybrid);

    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn parse_mode(text: &str) -> Option<Mode> {
        let text = text.to_lowercase();
        if text.contains("integrated") || text.contains("intel") {
            Some(Mode::Integrated)
        } else if text.contains("hybrid") || text.contains("on-demand") {
            Some(Mode::Hybrid)
        } else if text.contains("nvidia") || text.contains("dgpu") {
            Some(Mode::Dedicated)
        } else {
            None
        }
    }

    /// First installed tool and the mode it reports.
    pub fn detect() -> Option<(Tool, Mode)> {
        if std::env::var_os("RAZER_CONTROL_MOCK").is_some() {
            return Some((Tool::Mock, *MOCK_MODE.lock().unwrap()));
        }
        if let Some(mode) = run("supergfxctl", &["-g"]).as_deref().and_then(parse_mode) {
            return Some((Tool::SuperGfx, mode));
        }
        if let Some(mode) = run("prime-select", &["query"])
            .as_deref()
            .and_then(parse_mode)
        {
            return Some((Tool::PrimeSelect, mode));
        }
        if let Some(mode) = run("envycontrol", &["--query"])
            .as_deref()
            .and_then(parse_mode)
        {
            return Some((Tool::EnvyControl, mode));
        }
        None
    }

    /// supergfxctl exposes a plain dGPU mode only on ASUS MUX hardware, so
    /// the dGPU card stays locked under it.
    pub fn supports_dedicated(tool: Tool) -> bool {
        !matches!(tool, Tool::SuperGfx)
    }

    /// Blocking; run off the main loop.  pkexec pops the polkit dialog for
    /// the tools that need root.
    pub fn switch(tool: Tool, mode: Mode) -> Result<String, String> {
        let run_checked = |program: &str, args: &[&str]| {
            let output = Command::new(program)
                .args(args)
                .output()
                .map_err(|error| format!("cannot run {program}: {error}"))?;
            if output.status.success() {
                Ok(format!(
                    "ok GPU mode set — log out or reboot to apply ({program})"
                ))
            } else {
                Err(format!(
                    "{program} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        };
        match tool {
            Tool::Mock => {
                *MOCK_MODE.lock().unwrap() = mode;
                Ok("ok GPU mode set (simulated) — log out or reboot to apply".to_owned())
            }
            Tool::SuperGfx => {
                let name = match mode {
                    Mode::Integrated => "Integrated",
                    Mode::Hybrid => "Hybrid",
                    Mode::Dedicated => {
                        return Err("dGPU mode is not supported by supergfxctl here".to_owned());
                    }
                };
                run_checked("supergfxctl", &["-m", name])
            }
            Tool::PrimeSelect => {
                let name = match mode {
                    Mode::Integrated => "intel",
                    Mode::Hybrid => "on-demand",
                    Mode::Dedicated => "nvidia",
                };
                run_checked("pkexec", &["prime-select", name])
            }
            Tool::EnvyControl => {
                let name = match mode {
                    Mode::Integrated => "integrated",
                    Mode::Hybrid => "hybrid",
                    Mode::Dedicated => "nvidia",
                };
                run_checked("pkexec", &["envycontrol", "-s", name])
            }
        }
    }
}

/// Display & GPU page, presented after Synapse 4's DISPLAY section:
/// two columns of cards with green in-card headings.  Left: laptop
/// display (resolution, refresh, battery 60 Hz rule), color profile,
/// panel brightness.  Right: GPU mode as a radio list, external
/// displays.  Refresh rate goes through kscreen-doctor (KDE); panel
/// brightness through logind's SetBrightness D-Bus call — the seat owner
/// may write it without any privilege escalation.
fn display_page(overlay: &adw::ToastOverlay) -> gtk::Widget {
    let outputs = display_outputs();
    let laptop_index = outputs
        .iter()
        .position(|(name, _)| name.starts_with("eDP"))
        .or(if outputs.is_empty() { None } else { Some(0) });

    let left = gtk::Box::new(gtk::Orientation::Vertical, 12);
    match laptop_index {
        Some(index) => {
            let (name, modes) = &outputs[index];
            left.append(&laptop_display_card(overlay, name, modes));
        }
        None => {
            let card = fang_card(10);
            card.append(&card_heading("LAPTOP DISPLAY"));
            card.append(&note_label(
                "No controllable display in this session: kscreen-doctor (KDE) \
                 is unavailable.",
            ));
            left.append(&card);
        }
    }
    left.append(&color_profile_card(overlay));

    let right = gtk::Box::new(gtk::Orientation::Vertical, 12);
    right.append(&gpu_mode_card(overlay, gpu::detect()));
    let externals: Vec<&(String, Vec<DisplayMode>)> = outputs
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != laptop_index)
        .map(|(_, output)| output)
        .collect();
    if !externals.is_empty() {
        right.append(&external_display_card(overlay, &externals));
    }

    let columns = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();
    columns.append(&left);
    columns.append(&right);
    fang_page(&[columns.upcast()])
}

/// Green in-card heading, Synapse-style ("LAPTOP DISPLAY", "GPU MODE").
fn card_heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .css_classes(["card-heading"])
        .build()
}

/// Synapse's GPU MODE card: one radio row per mode with a description.
/// With no switching tool installed the list stays locked under an
/// explanatory note; a successful switch shows the pending note (a GPU
/// mode change never applies live).
fn gpu_mode_card(
    overlay: &adw::ToastOverlay,
    detected: Option<(gpu::Tool, gpu::Mode)>,
) -> gtk::Box {
    const OPTIONS: [(&str, &str, gpu::Mode); 3] = [
        (
            "NVIDIA Optimus (Hybrid)",
            "Integrated and dedicated GPU switching for optimal performance \
             and battery life (PRIME offload).",
            gpu::Mode::Hybrid,
        ),
        (
            "Dedicated GPU only",
            "Directly drive graphics through the dedicated GPU for lower \
             latency and maximum performance.",
            gpu::Mode::Dedicated,
        ),
        (
            "Integrated only",
            "The NVIDIA GPU powers down completely. Maximum battery life.",
            gpu::Mode::Integrated,
        ),
    ];
    let row_index = |mode: gpu::Mode| {
        OPTIONS
            .iter()
            .position(|(_, _, candidate)| *candidate == mode)
            .unwrap_or(0)
    };

    let card = fang_card(12);
    card.append(&card_heading("GPU MODE"));

    let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let mut radios: Vec<gtk::CheckButton> = Vec::with_capacity(OPTIONS.len());
    let mut rows: Vec<gtk::Box> = Vec::with_capacity(OPTIONS.len());
    for (title, description, _) in OPTIONS {
        let radio = gtk::CheckButton::builder()
            .valign(gtk::Align::Start)
            .build();
        if let Some(first) = radios.first() {
            radio.set_group(Some(first));
        }
        let title_label = gtk::Label::builder()
            .label(title)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build();
        let desc_label = gtk::Label::builder()
            .label(description)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["profile-desc"])
            .build();
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.append(&title_label);
        text.append(&desc_label);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.append(&radio);
        row.append(&text);
        // Clicking anywhere on the row selects its radio, like Synapse.
        let click = gtk::GestureClick::new();
        click.connect_released(clone!(
            #[weak]
            radio,
            move |_, _, _, _| {
                if radio.is_sensitive() {
                    radio.set_active(true);
                }
            }
        ));
        row.add_controller(click);
        list.append(&row);
        radios.push(radio);
        rows.push(row);
    }
    card.append(&list);

    let Some((tool, current)) = detected else {
        list.set_sensitive(false);
        card.append(&note_label(
            "Locked: no supported GPU switching tool found. Install supergfxctl \
             (Fedora), prime-select (Ubuntu), or envycontrol.",
        ));
        return card;
    };

    radios[row_index(current)].set_active(true);
    if !gpu::supports_dedicated(tool) {
        rows[row_index(gpu::Mode::Dedicated)].set_sensitive(false);
    }

    let pending = note_label("GPU mode changed — log out or reboot to apply.");
    pending.set_visible(false);
    card.append(&pending);

    let last_applied = Rc::new(Cell::new(current));
    let all_radios = radios.clone();
    for (index, radio) in radios.iter().enumerate() {
        let mode = OPTIONS[index].2;
        let all_radios = all_radios.clone();
        radio.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            pending,
            #[strong]
            last_applied,
            move |radio| {
                if !radio.is_active() || last_applied.get() == mode {
                    return;
                }
                // Blocking call (pkexec may prompt); acceptable for the
                // seconds it takes, and the mock path is instant.
                let result = gpu::switch(tool, mode);
                match &result {
                    Ok(_) => {
                        last_applied.set(mode);
                        pending.set_visible(true);
                    }
                    Err(_) => {
                        // Put the selection back on the mode that is
                        // actually active; the guard above keeps this
                        // from re-triggering a switch.
                        all_radios[row_index(last_applied.get())].set_active(true);
                    }
                }
                feedback(&overlay, result);
            }
        ));
    }
    card
}

/// All enabled outputs and their modes: kscreen-doctor normally, or a
/// simulated laptop panel plus one external monitor under
/// RAZER_CONTROL_MOCK=1 so every card can be exercised where kscreen
/// does not exist (WSLg) — always labelled simulated, like the daemon
/// mock.
fn display_outputs() -> Vec<(String, Vec<DisplayMode>)> {
    if std::env::var_os("RAZER_CONTROL_MOCK").is_some() {
        let simulated = |resolution: &str, rates: &[&str], current: &str| -> Vec<DisplayMode> {
            rates
                .iter()
                .map(|hz| DisplayMode {
                    id: format!("{resolution}@{hz}"),
                    label: format!("{resolution}@{hz}"),
                    current: hz == &current,
                })
                .collect()
        };
        return vec![
            (
                "eDP-1 (simulated)".to_owned(),
                simulated("2560x1600", &["60", "120", "240"], "240"),
            ),
            (
                "HDMI-1 (simulated)".to_owned(),
                simulated("1920x1080", &["60", "144"], "144"),
            ),
        ];
    }
    kscreen_outputs()
}

struct DisplayMode {
    id: String,
    label: String,
    current: bool,
}

/// `kscreen-doctor -o` for every enabled output: mode tokens look like
/// `id:WxH@Hz` with `*` marking the current mode and `!` the preferred one.
fn kscreen_outputs() -> Vec<(String, Vec<DisplayMode>)> {
    let Some(output) = std::process::Command::new("kscreen-doctor")
        .arg("-o")
        .output()
        .ok()
        .filter(|output| output.status.success())
    else {
        return Vec::new();
    };
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let mut outputs = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.first() != Some(&"Output:") || !line.contains("enabled") {
            continue;
        }
        let Some(name) = tokens.get(2) else {
            continue;
        };
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
                let Some((id, label)) = cleaned.split_once(':') else {
                    continue;
                };
                modes.push(DisplayMode {
                    id: id.to_owned(),
                    label: label.to_owned(),
                    current,
                });
            }
        }
        if !modes.is_empty() {
            outputs.push(((*name).to_owned(), modes));
        }
    }
    outputs
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

/// `WxH@Hz` → (`WxH`, `Hz` rounded for display).
fn split_mode_label(label: &str) -> Option<(&str, String)> {
    let (resolution, hz) = label.split_once('@')?;
    let display = hz
        .parse::<f64>()
        .map_or_else(|_| hz.to_owned(), |value| format!("{value:.0}"));
    Some((resolution, display))
}

/// Sets one output's mode via kscreen-doctor; simulated outputs succeed
/// without touching anything, always saying so.
fn apply_display_mode(output_name: &str, mode_id: &str) -> Result<String, String> {
    if output_name.contains("(simulated)") {
        return Ok("ok refresh rate applied (simulated)".to_owned());
    }
    let result = std::process::Command::new("kscreen-doctor")
        .arg(format!("output.{output_name}.mode.{mode_id}"))
        .status();
    match result {
        Ok(status) if status.success() => Ok("ok refresh rate applied".to_owned()),
        Ok(status) => Err(format!("kscreen-doctor exited with {status}")),
        Err(error) => Err(format!("cannot run kscreen-doctor: {error}")),
    }
}

/// One `(hz, mode id)` candidate per distinct rate at the output's
/// current resolution, ascending like Synapse's 60 | 240 row, plus the
/// index of the active one.
fn rate_candidates(modes: &[DisplayMode]) -> (Vec<(String, String)>, usize) {
    let current_index = modes.iter().position(|mode| mode.current).unwrap_or(0);
    let resolution = split_mode_label(&modes[current_index].label)
        .map(|(resolution, _)| resolution.to_owned())
        .unwrap_or_default();
    let mut candidates: Vec<(String, String, bool)> = Vec::new();
    for mode in modes {
        let Some((mode_resolution, hz)) = split_mode_label(&mode.label) else {
            continue;
        };
        if mode_resolution == resolution
            && !candidates.iter().any(|(existing, _, _)| *existing == hz)
        {
            candidates.push((hz, mode.id.clone(), mode.current));
        }
    }
    candidates.sort_by(|a, b| {
        a.0.parse::<f64>()
            .unwrap_or(0.0)
            .total_cmp(&b.0.parse::<f64>().unwrap_or(0.0))
    });
    let initial = candidates
        .iter()
        .position(|(_, _, current)| *current)
        .unwrap_or(0);
    (
        candidates.into_iter().map(|(hz, id, _)| (hz, id)).collect(),
        initial,
    )
}

/// Instant-apply rate segments for one output.  Returns the segment box
/// and the index of the user's current choice, which the battery rule
/// uses as its restore target.
fn rate_segments(
    overlay: &adw::ToastOverlay,
    output_name: &str,
    candidates: &[(String, String)],
    initial: usize,
) -> (gtk::Box, Rc<Cell<usize>>) {
    let labels: Vec<String> = candidates
        .iter()
        .map(|(hz, _)| format!("{hz} Hz"))
        .collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let (seg_box, seg) = segmented(&label_refs, initial);
    let chosen = Rc::new(Cell::new(initial));
    // Wire after the initial set_active so startup applies nothing.
    for (index, (button, (_, mode_id))) in seg.iter().zip(candidates).enumerate() {
        let mode_id = mode_id.clone();
        let output_name = output_name.to_owned();
        button.connect_toggled(clone!(
            #[weak]
            overlay,
            #[strong]
            chosen,
            move |button| {
                if !button.is_active() {
                    return;
                }
                chosen.set(index);
                feedback(&overlay, apply_display_mode(&output_name, &mode_id));
            }
        ));
    }
    (seg_box, chosen)
}

/// Synapse's LAPTOP DISPLAY card: current resolution, refresh-rate
/// segments, and the working "60Hz on battery" rule.  The rule watches
/// the daemon's power telemetry: dropping to battery applies the lowest
/// rate directly (the segments keep showing your choice), and returning
/// to AC re-applies the chosen segment.
fn laptop_display_card(
    overlay: &adw::ToastOverlay,
    output_name: &str,
    modes: &[DisplayMode],
) -> gtk::Box {
    let (candidates, initial) = rate_candidates(modes);
    let resolution = modes
        .iter()
        .find(|mode| mode.current)
        .or(modes.first())
        .and_then(|mode| split_mode_label(&mode.label))
        .map(|(resolution, _)| resolution.replace('x', " x "))
        .unwrap_or_default();

    let card = fang_card(12);
    card.append(&card_heading("LAPTOP DISPLAY"));
    card.append(&{
        let caption = dash_label("CURRENT RESOLUTION");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    card.append(
        &gtk::Label::builder()
            .label(&resolution)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build(),
    );
    card.append(&{
        let caption = dash_label("CURRENT REFRESH RATE");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    let (seg_box, chosen) = rate_segments(overlay, output_name, &candidates, initial);
    card.append(&seg_box);

    let battery_rule = gtk::CheckButton::builder()
        .label("Switch laptop screen refresh rate to 60Hz when on battery.")
        .active(load_battery_60hz())
        .build();
    battery_rule.connect_toggled(|check| save_battery_60hz(check.is_active()));
    card.append(&{
        let caption = dash_label("BATTERY REFRESH RATE");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    card.append(&battery_rule);

    // The rule itself: act on power-source transitions only.
    let output_name = output_name.to_owned();
    let last_power = Cell::new(None::<bool>);
    glib::timeout_add_seconds_local(2, move || {
        let on_ac = match request_fields("telemetry").get("power").map(String::as_str) {
            Some("ac") => true,
            Some("battery") => false,
            _ => return glib::ControlFlow::Continue,
        };
        let previous = last_power.replace(Some(on_ac));
        if previous == Some(on_ac) || previous.is_none() || !battery_rule.is_active() {
            return glib::ControlFlow::Continue;
        }
        let target = if on_ac {
            candidates.get(chosen.get())
        } else {
            candidates.first() // lowest rate
        };
        if let Some((_, mode_id)) = target
            && let Err(error) = apply_display_mode(&output_name, mode_id)
        {
            eprintln!("battery refresh-rate rule: {error}");
        }
        glib::ControlFlow::Continue
    });
    card
}

/// GUI-only settings file (the daemon's state file stays daemon-owned).
/// The name predates the Lighting page; it now holds every GUI rule.
fn gui_config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("razer-control").join("display.conf"))
}

fn gui_config_get(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(gui_config_path()?).ok()?;
    text.lines().find_map(|line| {
        line.trim()
            .split_once('=')
            .filter(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value.to_owned())
    })
}

/// Read-modify-write so each setting keeps the others intact.
fn gui_config_set(key: &str, value: &str) {
    let Some(path) = gui_config_path() else {
        return;
    };
    let mut entries: Vec<(String, String)> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            line.trim()
                .split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .filter(|(k, _)| k != key)
        .collect();
    entries.push((key.to_owned(), value.to_owned()));
    entries.sort();
    let text = entries
        .iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect::<String>();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, text) {
        eprintln!("cannot save GUI settings: {error}");
    }
}

fn load_battery_60hz() -> bool {
    gui_config_get("battery_60hz").as_deref() == Some("true")
}

fn save_battery_60hz(on: bool) {
    gui_config_set("battery_60hz", if on { "true" } else { "false" });
}

/// Synapse's COLOR PROFILE card, pointed at KDE's color management.
fn color_profile_card(overlay: &adw::ToastOverlay) -> gtk::Box {
    let card = fang_card(10);
    card.append(&card_heading("COLOR PROFILE"));
    card.append(&note_label(
        "ICC profiles and calibration are managed by KDE's color settings.",
    ));
    let open = gtk::Button::builder()
        .label("OPEN COLOR SETTINGS")
        .halign(gtk::Align::Start)
        .css_classes(["fang-ghost"])
        .build();
    open.connect_clicked(clone!(
        #[weak]
        overlay,
        move |_| {
            if std::env::var_os("RAZER_CONTROL_MOCK").is_some() {
                feedback(
                    &overlay,
                    Ok("ok color settings opened (simulated)".to_owned()),
                );
                return;
            }
            let result = std::process::Command::new("systemsettings")
                .arg("kcm_colord")
                .spawn();
            feedback(
                &overlay,
                match result {
                    Ok(_) => Ok("ok opening color settings".to_owned()),
                    Err(error) => Err(format!("cannot open systemsettings: {error}")),
                },
            );
        }
    ));
    card.append(&open);
    card
}

/// One row per connected external output, each with its own instant
/// refresh-rate segments — the part Synapse can only point at the
/// Windows control panel for.
fn external_display_card(
    overlay: &adw::ToastOverlay,
    externals: &[&(String, Vec<DisplayMode>)],
) -> gtk::Box {
    let card = fang_card(12);
    card.append(&card_heading("EXTERNAL DISPLAY"));
    for (name, modes) in externals {
        let (candidates, initial) = rate_candidates(modes);
        let resolution = modes
            .iter()
            .find(|mode| mode.current)
            .or(modes.first())
            .and_then(|mode| split_mode_label(&mode.label))
            .map(|(resolution, _)| resolution.to_owned())
            .unwrap_or_default();
        let title = gtk::Label::builder()
            .label(name)
            .halign(gtk::Align::Start)
            .css_classes(["card-title"])
            .build();
        let subtitle = gtk::Label::builder()
            .label(&resolution)
            .halign(gtk::Align::Start)
            .css_classes(["card-subtitle"])
            .build();
        let identity = gtk::Box::new(gtk::Orientation::Vertical, 2);
        identity.append(&title);
        identity.append(&subtitle);
        let (seg_box, _) = rate_segments(overlay, name, &candidates, initial);
        seg_box.set_halign(gtk::Align::End);
        seg_box.set_hexpand(true);
        seg_box.set_valign(gtk::Align::Center);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.append(&identity);
        row.append(&seg_box);
        card.append(&row);
    }
    card.append(&note_label(
        "Applies instantly to the selected display; no reboot needed.",
    ));
    card
}

/// First backlight device: (name, current, max).  Simulated under
/// RAZER_CONTROL_MOCK=1 so the card renders where no backlight exists.
fn backlight_device() -> Option<(String, u32, u32)> {
    if std::env::var_os("RAZER_CONTROL_MOCK").is_some() {
        return Some(("intel_backlight (simulated)".to_owned(), 80, 100));
    }
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

fn brightness_card() -> Option<gtk::Box> {
    let (device, current, max) = backlight_device()?;
    let percent_of = move |value: f64| ((value / max as f64) * 100.0).round();

    let percent_label = gtk::Label::builder()
        .label(format!("{:.0}", percent_of(current as f64)))
        .css_classes(["dash-value-medium"])
        .build();
    let unit = gtk::Label::builder()
        .label("% BRIGHTNESS")
        .css_classes(["dash-label"])
        .valign(gtk::Align::End)
        .margin_bottom(3)
        .build();
    let readout = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    readout.append(&percent_label);
    readout.append(&unit);

    let scale = value_scale(
        0.0,
        max as f64,
        (max as f64 / 100.0).max(1.0),
        current as f64,
    );
    scale.set_draw_value(false);

    let card = fang_card(10);
    card.append(&card_heading("LAPTOP PANEL BRIGHTNESS"));
    card.append(&readout);
    card.append(&scale);
    card.append(&note_label(
        "The built-in screen's backlight — applies instantly.",
    ));

    scale.connect_value_changed(clone!(
        #[weak]
        percent_label,
        move |scale| {
            let value = scale.value().round() as u32;
            percent_label.set_text(&format!("{:.0}", percent_of(scale.value())));
            if device.contains("(simulated)") {
                return;
            }
            let connection = match gtk::gio::bus_get_sync(
                gtk::gio::BusType::System,
                gtk::gio::Cancellable::NONE,
            ) {
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
        }
    ));
    Some(card)
}

/// Cancels the previous pending run and schedules `action` after
/// `delay_ms` — the debounce for sliders whose every tick would
/// otherwise become an IPC request or an external command.
fn debounce(
    pending: &Rc<RefCell<Option<glib::SourceId>>>,
    delay_ms: u64,
    action: impl FnOnce() + 'static,
) {
    if let Some(source) = pending.borrow_mut().take() {
        source.remove();
    }
    let cleared = Rc::clone(pending);
    let id = glib::timeout_add_local_once(std::time::Duration::from_millis(delay_ms), move || {
        *cleared.borrow_mut() = None;
        action();
    });
    *pending.borrow_mut() = Some(id);
}

/// Lighting page: best of Fang's Lighting page and Synapse 4's Lighting
/// section.  Keyboard backlight (per-power-source brightness like
/// Synapse, Fang's four effects with color swatches), the idle
/// switch-off rule, the lid logo, the laptop panel backlight (moved here
/// from Display & GPU), and Fang's DDC external-monitor card.
fn lighting_page(overlay: &adw::ToastOverlay) -> gtk::Widget {
    let status = request_fields("status");
    let experimental = status.get("experimental").map(String::as_str) == Some("true");

    let left = gtk::Box::new(gtk::Orientation::Vertical, 12);
    left.append(&keyboard_backlight_card(overlay, experimental, &status));
    left.append(&switch_off_card());

    let right = gtk::Box::new(gtk::Orientation::Vertical, 12);
    right.append(&logo_card(overlay, experimental, &status));
    if let Some(card) = brightness_card() {
        right.append(&card);
    }
    if let Some(card) = ddc_monitor_card(overlay) {
        right.append(&card);
    }

    let columns = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();
    columns.append(&left);
    columns.append(&right);
    fang_page(&[columns.upcast()])
}

/// Fixed color presets for the static effect; class names match the
/// stylesheet's swatch entries.
const SWATCHES: [&str; 6] = ["44d62c", "ffffff", "ff3a3a", "2c7bd6", "a02cd6", "2cd6c8"];

fn keyboard_backlight_card(
    overlay: &adw::ToastOverlay,
    experimental: bool,
    status: &HashMap<String, String>,
) -> gtk::Box {
    let card = fang_card(12);
    card.append(&card_heading("KEYBOARD BACKLIGHT"));

    // Per-power-source brightness (Synapse).  Rules come from the daemon;
    // an unset rule falls back to the current brightness, then 60%.
    let fallback = status
        .get("kbd")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(60.0);
    let rule = |key: &str| {
        status
            .get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(fallback)
    };
    let values = Rc::new(RefCell::new([rule("kbd_ac"), rule("kbd_battery")]));

    let (tabs_box, tabs) = segmented(&["PLUGGED IN", "ON BATTERY"], 0);
    card.append(&tabs_box);

    let scale = value_scale(0.0, 100.0, 5.0, values.borrow()[0]);
    let brightness_box = labelled_scale("BRIGHTNESS — %", &scale);
    card.append(&brightness_box);

    // Guard so loading a tab's value into the scale does not re-send it.
    let updating = Rc::new(Cell::new(false));
    let active_tab = Rc::new(Cell::new(0usize));
    for (index, tab) in tabs.iter().enumerate() {
        tab.connect_toggled(clone!(
            #[weak]
            scale,
            #[strong]
            values,
            #[strong]
            updating,
            #[strong]
            active_tab,
            move |tab| {
                if !tab.is_active() {
                    return;
                }
                active_tab.set(index);
                updating.set(true);
                scale.set_value(values.borrow()[index]);
                updating.set(false);
            }
        ));
    }

    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));
    scale.connect_value_changed(clone!(
        #[weak]
        overlay,
        #[strong]
        values,
        #[strong]
        updating,
        #[strong]
        active_tab,
        #[strong]
        pending,
        move |scale| {
            if updating.get() {
                return;
            }
            let tab = active_tab.get();
            let percent = scale.value().round();
            values.borrow_mut()[tab] = percent;
            let overlay = overlay.clone();
            debounce(&pending, 250, move || {
                let source = if tab == 0 { "ac" } else { "battery" };
                let rule = transport::request(&format!("kbd-automation {source} {percent}"));
                // Apply live when this tab matches the current power source.
                let live =
                    request_fields("telemetry").get("power").map(String::as_str) == Some(source);
                let result = if live {
                    rule.and_then(|_| transport::request(&format!("kbd brightness {percent}")))
                } else {
                    rule
                };
                feedback(&overlay, result);
            });
        }
    ));

    // Effect segments + color swatches (Fang).
    let effect_value = status.get("kbd_effect").map_or("unset", String::as_str);
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

    card.append(&{
        let caption = dash_label("EFFECT");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    let (effect_box, effects) = segmented(&["OFF", "STATIC", "SPECTRUM", "WAVE"], initial_effect);
    card.append(&effect_box);

    let color_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let color_caption = gtk::Label::builder()
        .label("COLOR")
        .css_classes(["dash-label"])
        .build();
    color_row.append(&color_caption);
    let mut swatches: Vec<gtk::ToggleButton> = Vec::with_capacity(SWATCHES.len());
    for (index, _) in SWATCHES.iter().enumerate() {
        let swatch = gtk::ToggleButton::builder()
            .css_classes(["swatch", &format!("swatch-{index}")[..]])
            .build();
        if let Some(first) = swatches.first() {
            swatch.set_group(Some(first));
        }
        color_row.append(&swatch);
        swatches.push(swatch);
    }
    let hex_label = gtk::Label::builder()
        .label(format!("#{}", SWATCHES[initial_swatch]))
        .css_classes(["card-subtitle"])
        .build();
    color_row.append(&hex_label);
    swatches[initial_swatch].set_active(true);
    color_row.set_visible(initial_effect == 1);
    card.append(&color_row);

    let chosen_swatch = Rc::new(Cell::new(initial_swatch));
    let static_button = effects[1].clone();
    let send_effect = {
        let chosen_swatch = Rc::clone(&chosen_swatch);
        move |overlay: &adw::ToastOverlay, index: usize| {
            let line = match index {
                0 => "kbd effect off".to_owned(),
                2 => "kbd effect spectrum".to_owned(),
                3 => "kbd effect wave".to_owned(),
                _ => format!("kbd effect static {}", SWATCHES[chosen_swatch.get()]),
            };
            feedback(overlay, transport::request(&line));
        }
    };
    for (index, button) in effects.iter().enumerate() {
        let send_effect = send_effect.clone();
        button.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            color_row,
            move |button| {
                if button.is_active() {
                    color_row.set_visible(index == 1);
                    send_effect(&overlay, index);
                }
            }
        ));
    }
    for (index, swatch) in swatches.iter().enumerate() {
        let send_effect = send_effect.clone();
        swatch.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            hex_label,
            #[weak]
            static_button,
            #[strong]
            chosen_swatch,
            move |swatch| {
                if !swatch.is_active() {
                    return;
                }
                chosen_swatch.set(index);
                hex_label.set_text(&format!("#{}", SWATCHES[index]));
                if static_button.is_active() {
                    send_effect(&overlay, 1);
                }
            }
        ));
    }

    if !experimental {
        card.set_sensitive(false);
        card.append(&note_label(
            "Locked: keyboard lighting sends EC commands not yet verified on \
             this machine (Phase 3). Start the daemon with --experimental to \
             enable it.",
        ));
    }
    card
}

/// Synapse's switch-off rule, idle-only: KDE's ScreenSaver bus exposes
/// the session idle time; past the threshold the keyboard backlight goes
/// to 0%, and activity restores the active power source's brightness.
fn switch_off_card() -> gtk::Box {
    let card = fang_card(12);
    card.append(&card_heading("SWITCH OFF LIGHTING"));

    let enabled = gtk::CheckButton::builder()
        .label("Turn keyboard lighting off when idle for (minutes):")
        .active(gui_config_get("kbd_idle_off").as_deref() == Some("true"))
        .build();
    enabled.connect_toggled(|check| {
        gui_config_set(
            "kbd_idle_off",
            if check.is_active() { "true" } else { "false" },
        );
    });
    card.append(&enabled);

    let minutes = gui_config_get("kbd_idle_minutes")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(10.0);
    let scale = value_scale(1.0, 60.0, 1.0, minutes);
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));
    scale.connect_value_changed(clone!(
        #[strong]
        pending,
        move |scale| {
            let value = scale.value().round() as u32;
            debounce(&pending, 300, move || {
                gui_config_set("kbd_idle_minutes", &value.to_string());
            });
        }
    ));
    card.append(&scale);
    card.append(&note_label(
        "Uses the desktop's idle timer (KDE). Lighting comes back on \
         activity at the brightness configured for the current power source.",
    ));

    // The rule: poll session idle time every 30 s.  Absent bus or method
    // (WSL, non-KDE) the poll silently does nothing.
    let dimmed = Rc::new(Cell::new(false));
    glib::timeout_add_seconds_local(
        30,
        clone!(
            #[weak(rename_to = enabled_check)]
            enabled,
            #[weak]
            scale,
            #[strong]
            dimmed,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                if !enabled_check.is_active() {
                    return glib::ControlFlow::Continue;
                }
                let Some(idle_seconds) = session_idle_seconds() else {
                    return glib::ControlFlow::Continue;
                };
                let threshold = scale.value().round() as u64 * 60;
                if idle_seconds >= threshold && !dimmed.get() {
                    if transport::request("kbd brightness 0").is_ok() {
                        dimmed.set(true);
                    }
                } else if idle_seconds < threshold && dimmed.get() {
                    let source = request_fields("telemetry")
                        .get("power")
                        .map_or("ac".to_owned(), String::clone);
                    let key = if source == "battery" {
                        "kbd_battery"
                    } else {
                        "kbd_ac"
                    };
                    let restore = request_fields("status")
                        .get(key)
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(60);
                    if transport::request(&format!("kbd brightness {restore}")).is_ok() {
                        dimmed.set(false);
                    }
                }
                glib::ControlFlow::Continue
            }
        ),
    );
    card
}

/// Session idle time in seconds via org.freedesktop.ScreenSaver (KDE
/// implements GetSessionIdleTime); None where the bus or method is absent.
fn session_idle_seconds() -> Option<u64> {
    let connection =
        gtk::gio::bus_get_sync(gtk::gio::BusType::Session, gtk::gio::Cancellable::NONE).ok()?;
    let reply = connection
        .call_sync(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            "GetSessionIdleTime",
            None,
            None,
            gtk::gio::DBusCallFlags::NONE,
            1000,
            gtk::gio::Cancellable::NONE,
        )
        .ok()?;
    reply.child_value(0).get::<u32>().map(u64::from)
}

/// Fang's LID LOGO card: OFF | STATIC | BREATHING.
fn logo_card(
    overlay: &adw::ToastOverlay,
    experimental: bool,
    status: &HashMap<String, String>,
) -> gtk::Box {
    let card = fang_card(12);
    card.append(&card_heading("LID LOGO"));
    let initial = match status.get("logo").map(String::as_str) {
        Some("off") => 0,
        Some("breathing") => 2,
        _ => 1,
    };
    let (seg_box, seg) = segmented(&["OFF", "STATIC", "BREATHING"], initial);
    card.append(&seg_box);
    card.append(&note_label(
        "The snake on the lid. Static keeps it lit; Breathing pulses it slowly.",
    ));
    for (index, button) in seg.iter().enumerate() {
        let line = ["logo off", "logo static", "logo breathing"][index];
        button.connect_toggled(clone!(
            #[weak]
            overlay,
            move |button| {
                if button.is_active() {
                    feedback(&overlay, transport::request(line));
                }
            }
        ));
    }
    if !experimental {
        card.set_sensitive(false);
        card.append(&note_label(
            "Locked until the daemon runs with --experimental (Phase 3).",
        ));
    }
    card
}

/// Fang's EXTERNAL MONITOR card: brightness and colour-temperature
/// presets over DDC/CI via ddcutil.  Simulated under the mock; hidden
/// when ddcutil or a DDC display is absent.
fn ddc_monitor_card(overlay: &adw::ToastOverlay) -> Option<gtk::Box> {
    let simulated = std::env::var_os("RAZER_CONTROL_MOCK").is_some();
    if !simulated {
        let detect = std::process::Command::new("ddcutil")
            .args(["detect", "--terse"])
            .output()
            .ok()?;
        if !detect.status.success() || !String::from_utf8_lossy(&detect.stdout).contains("Display")
        {
            return None;
        }
    }

    let card = fang_card(12);
    card.append(&card_heading("EXTERNAL MONITOR"));
    if simulated {
        card.append(&note_label("DDC (simulated)"));
    }

    let scale = value_scale(0.0, 100.0, 5.0, 75.0);
    let brightness_box = labelled_scale("BRIGHTNESS — %", &scale);
    card.append(&brightness_box);
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));
    scale.connect_value_changed(clone!(
        #[weak]
        overlay,
        #[strong]
        pending,
        move |scale| {
            let value = scale.value().round() as u32;
            let overlay = overlay.clone();
            debounce(&pending, 300, move || {
                feedback(&overlay, ddc_set(simulated, "10", &value.to_string()));
            });
        }
    ));

    card.append(&{
        let caption = dash_label("COLOR TEMPERATURE");
        caption.set_halign(gtk::Align::Start);
        caption
    });
    // VCP 0x14 select-color-preset codes: 4000/5000K warm 0x04,
    // 6500K 0x05, 9300K 0x08.
    let (seg_box, seg) = segmented(&["WARM (5000K)", "SRGB (6500K)", "COOL (9300K)"], 1);
    card.append(&seg_box);
    for (button, code) in seg.iter().zip(["04", "05", "08"]) {
        button.connect_toggled(clone!(
            #[weak]
            overlay,
            move |button| {
                if button.is_active() {
                    feedback(&overlay, ddc_set(simulated, "14", code));
                }
            }
        ));
    }
    card.append(&note_label(
        "Brightness and color presets on the external monitor, sent over \
         DDC/CI. The laptop panel can't be color-managed this way.",
    ));
    Some(card)
}

/// One `ddcutil setvcp` invocation, or a simulated success.
fn ddc_set(simulated: bool, feature: &str, value: &str) -> Result<String, String> {
    if simulated {
        return Ok(format!("ok ddc setvcp {feature}={value} (simulated)"));
    }
    let result = std::process::Command::new("ddcutil")
        .args(["setvcp", feature, value])
        .output();
    match result {
        Ok(output) if output.status.success() => Ok("ok monitor updated".to_owned()),
        Ok(output) => Err(format!(
            "ddcutil failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("cannot run ddcutil: {error}")),
    }
}

const SPARKLINE_CAPACITY: usize = 90;
const GAUGE_MIN_C: f64 = 30.0;
const GAUGE_MAX_C: f64 = 100.0;

/// Fang-style dashboard: gauge, fan, and power cards over two side-by-side
/// 90-second history charts and an active-profile bar, fed by the daemon's
/// read-only `telemetry` request at 1 Hz.  Pure display — no control lives
/// here.
fn dashboard_page() -> gtk::Widget {
    let cpu_value = Rc::new(Cell::new(None::<f64>));
    let gpu_value = Rc::new(Cell::new(None::<f64>));
    let fan_value = Rc::new(Cell::new(None::<f64>));
    let cpu_history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(
        SPARKLINE_CAPACITY,
    )));
    let fan_history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(
        SPARKLINE_CAPACITY,
    )));

    // Fang's top row: CPU PACKAGE and GPU CORE gauges, then the fan card.
    let (cpu_card, cpu_area, cpu_value_label) =
        build_gauge_card("CPU PACKAGE", Rc::clone(&cpu_value));
    let (gpu_card, gpu_area, gpu_value_label) = build_gauge_card("GPU CORE", Rc::clone(&gpu_value));
    let (fan_card, fan_value_label) = build_fan_card(Rc::clone(&fan_value));

    let cards = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        // Children use vexpand to centre themselves inside the row; an
        // explicit false here stops that propagating up and stretching the
        // whole row down the page.
        .vexpand(false)
        .build();
    cards.append(&cpu_card);
    cards.append(&gpu_card);
    cards.append(&fan_card);

    let (cpu_chart_card, cpu_chart_area, cpu_chart_value) =
        build_sparkline_card("CPU TEMPERATURE — 90 S", Rc::clone(&cpu_history));
    let (fan_chart_card, fan_chart_area, fan_chart_value) =
        build_sparkline_card("FAN SPEED — 90 S", Rc::clone(&fan_history));
    let charts = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();
    charts.append(&cpu_chart_card);
    charts.append(&fan_chart_card);

    let (profile_bar, profile_value_label, profile_detail_label) = build_profile_bar();

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(16)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    page.append(&cards);
    page.append(&charts);
    page.append(&profile_bar);

    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let telemetry = request_fields("telemetry");
        let status = request_fields("status");

        let cpu = telemetry
            .get("cpu_temp")
            .and_then(|value| value.parse::<f64>().ok());
        cpu_value.set(cpu);
        cpu_value_label.set_text(&cpu.map_or("—".to_owned(), |t| format!("{t:.0}")));
        if let Some(temperature) = cpu {
            push_sample(&cpu_history, temperature);
            cpu_chart_value.set_text(&format!("{temperature:.0} °C"));
        }

        let gpu = telemetry
            .get("gpu_temp")
            .and_then(|value| value.parse::<f64>().ok());
        gpu_value.set(gpu);
        gpu_value_label.set_text(&gpu.map_or("—".to_owned(), |t| format!("{t:.0}")));

        let fan = telemetry
            .get("fan_rpm")
            .and_then(|value| value.parse::<f64>().ok());
        fan_value.set(fan);
        fan_value_label.set_text(&fan.map_or("—".to_owned(), |rpm| format!("{rpm:.0}")));
        if let Some(rpm) = fan {
            push_sample(&fan_history, rpm);
            fan_chart_value.set_text(&format!("{rpm:.0} rpm"));
        }

        // Power source moved off the card row (Fang has no power card
        // there); it lives in the profile bar's detail text instead.
        let power = match telemetry.get("power").map(String::as_str) {
            Some("ac") => "plugged in",
            Some("battery") => "on battery",
            _ => "power unknown",
        };
        profile_value_label.set_text(&describe_status_fan(status.get("fan")));
        profile_detail_label.set_text(&format!(
            "{power} · backend: {}{}",
            status.get("backend").map_or("—", String::as_str),
            if telemetry.get("simulated").map(String::as_str) == Some("true") {
                " · simulated"
            } else {
                ""
            }
        ));
        cpu_area.queue_draw();
        gpu_area.queue_draw();
        cpu_chart_area.queue_draw();
        fan_chart_area.queue_draw();
        glib::ControlFlow::Continue
    });

    // Scrollable so the window can shrink below the cards' natural height.
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&page)
        .build()
        .upcast()
}

fn push_sample(history: &Rc<RefCell<VecDeque<f64>>>, sample: f64) {
    let mut samples = history.borrow_mut();
    if samples.len() == SPARKLINE_CAPACITY {
        samples.pop_front();
    }
    samples.push_back(sample);
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

fn dash_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Center)
        .css_classes(["dash-label"])
        .build()
}

/// Segmented arc gauge in the Fang style: 28 ticks over 270 degrees, lit
/// green up to the current fraction of the 30–100 °C span.
fn build_gauge_card(
    caption: &str,
    value: Rc<Cell<Option<f64>>>,
) -> (gtk::Box, gtk::DrawingArea, gtk::Label) {
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
                cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
            } else {
                cr.set_source_rgb(TICK_OFF.0, TICK_OFF.1, TICK_OFF.2);
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

    let card = fang_card(6);
    card.append(&overlay);
    card.append(&dash_label(caption));
    (card, area, value_label)
}

/// Fang's fan card: a nine-petal rosette with a green hub over the RPM
/// target in accent green.  The rosette spins at 1/40th of the reported
/// RPM — real speed is a blur at 60 fps — and freezes when telemetry has
/// no fan value.
fn build_fan_card(rpm: Rc<Cell<Option<f64>>>) -> (gtk::Box, gtk::Label) {
    let area = gtk::DrawingArea::builder()
        .content_width(120)
        .content_height(120)
        .halign(gtk::Align::Center)
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let angle = Rc::new(Cell::new(0.0_f64));
    let last_frame = Cell::new(None::<i64>);
    area.add_tick_callback(clone!(
        #[strong]
        angle,
        move |area, clock| {
            let now = clock.frame_time(); // microseconds
            let elapsed = last_frame
                .replace(Some(now))
                .map_or(0.0, |previous| (now - previous) as f64 / 1_000_000.0);
            if let Some(rpm) = rpm.get() {
                let revolutions_per_second = rpm / 60.0 / 40.0;
                let step = revolutions_per_second * 2.0 * std::f64::consts::PI * elapsed;
                angle.set((angle.get() + step).rem_euclid(2.0 * std::f64::consts::PI));
                area.queue_draw();
            }
            glib::ControlFlow::Continue
        }
    ));

    let draw_angle = Rc::clone(&angle);
    area.set_draw_func(move |_, cr, width, height| {
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        let radius = width.min(height) as f64 / 2.0 - 4.0;

        // Outer ring.
        cr.set_source_rgb(TICK_OFF.0, TICK_OFF.1, TICK_OFF.2);
        cr.set_line_width(1.5);
        cr.arc(center_x, center_y, radius, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        // Nine petals, teardrops from hub to rim.
        cr.set_source_rgb(0.15, 0.18, 0.16);
        for petal in 0..9 {
            let _ = cr.save();
            cr.translate(center_x, center_y);
            cr.rotate(draw_angle.get() + petal as f64 / 9.0 * 2.0 * std::f64::consts::PI);
            cr.move_to(0.0, 0.0);
            cr.curve_to(
                radius * 0.35,
                -radius * 0.28,
                radius * 0.9,
                -radius * 0.22,
                radius * 0.82,
                0.0,
            );
            cr.curve_to(
                radius * 0.7,
                radius * 0.16,
                radius * 0.3,
                radius * 0.12,
                0.0,
                0.0,
            );
            let _ = cr.fill();
            let _ = cr.restore();
        }

        // Hub: dark disc, thin green ring, green dot.
        cr.set_source_rgb(0.05, 0.07, 0.06);
        cr.arc(
            center_x,
            center_y,
            radius * 0.24,
            0.0,
            2.0 * std::f64::consts::PI,
        );
        let _ = cr.fill();
        cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
        cr.set_line_width(1.5);
        cr.arc(
            center_x,
            center_y,
            radius * 0.24,
            0.0,
            2.0 * std::f64::consts::PI,
        );
        let _ = cr.stroke();
        cr.arc(
            center_x,
            center_y,
            radius * 0.09,
            0.0,
            2.0 * std::f64::consts::PI,
        );
        let _ = cr.fill();
    });

    let value_label = gtk::Label::builder()
        .label("—")
        .css_classes(["dash-value-accent"])
        .build();
    let unit_label = gtk::Label::builder()
        .label("RPM")
        .css_classes(["dash-unit"])
        .valign(gtk::Align::End)
        .margin_bottom(5)
        .build();
    let readout = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    readout.append(&value_label);
    readout.append(&unit_label);

    let card = fang_card(6);
    card.append(&area);
    card.append(&readout);
    card.append(&dash_label("FAN TARGET"));
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
        cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.15);
        cr.move_to(0.0, height as f64);
        for (index, sample) in samples.iter().enumerate() {
            cr.line_to(index as f64 * x_step, project(*sample));
        }
        cr.line_to((samples.len() - 1) as f64 * x_step, height as f64);
        let _ = cr.fill();
        cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
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

    let card = fang_card(6);
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
        .css_classes(["fang-card"])
        .build();
    bar.append(&caption);
    bar.append(&value_label);
    bar.append(&detail_label);
    (bar, value_label, detail_label)
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
    let caption = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .css_classes(["dash-label"])
        .build();
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

fn refresh(daemon_value: &gtk::Label, power_value: &gtk::Label, transport_value: &gtk::Label) {
    let status = match transport::request("status") {
        Ok(response) => response,
        Err(error) => format!("err {error}"),
    };
    daemon_value.set_text(&status);
    power_value.set_text(power_source());
    transport_value.set_text(transport::label());
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
            // Experimental is on in the mock: the backend is a dry run, so
            // the profile UI can be exercised with zero hardware risk.  The
            // real daemon still defaults to locked.
            Mutex::new(
                Daemon::new(BLADE_14_2023, DryRunBackend::default(), true)
                    .with_simulated_telemetry(),
            )
        });
        Ok(daemon
            .lock()
            .map_err(|error| error.to_string())?
            .handle_line(line))
    }
}
