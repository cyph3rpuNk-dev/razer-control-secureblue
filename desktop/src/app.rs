//! GTK4/libadwaita UI shell.  Linux-only; see `main.rs` for the platform
//! gate.
//!
//! The app follows the GNOME HIG: an `AdwNavigationSplitView` with a sidebar
//! of pages built from stock libadwaita widgets, the system light/dark
//! scheme and fonts, and global banners for the two states a user must
//! never mistake — the daemon being unreachable, and a backend that does
//! not write to hardware.  Every control is a thin IPC client; policy lives
//! in the daemon.

mod client;
mod pages;
mod poll;
mod system;
mod ui;

use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use poll::{Poller, Snapshot};

const APP_ID: &str = "dev.cyph3rpunk.razer-control";
const REPOSITORY_URL: &str = "https://github.com/cyph3rpuNk-dev/razer-control-secureblue";

pub fn run() -> std::process::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| load_css());
    app.connect_activate(build_ui);
    let status = app.run();
    std::process::ExitCode::from(if status == glib::ExitCode::SUCCESS {
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
    let toast_overlay = adw::ToastOverlay::new();

    // One startup round-trip seeds every page's initial control state; the
    // poller then delivers live snapshots every 2 s.
    let seed = Snapshot::fetch_blocking();
    let poller = Poller::new();

    // Pages that drive the daemon are disabled while it is unreachable;
    // Overview, Display & GPU, and Diagnostics stay usable.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .build();
    let daemon_pages = [
        stack.add_titled(
            &pages::performance::page(&seed, &toast_overlay),
            Some("performance"),
            "Performance",
        ),
        stack.add_titled(
            &pages::cooling::page(&seed, &toast_overlay, &poller),
            Some("cooling"),
            "Cooling",
        ),
        stack.add_titled(
            &pages::battery::page(&seed, &toast_overlay),
            Some("battery"),
            "Battery",
        ),
        stack.add_titled(
            &pages::lighting::page(&seed, &toast_overlay, &poller),
            Some("lighting"),
            "Lighting",
        ),
        stack.add_titled(
            &pages::automation::page(&seed, &toast_overlay, &poller),
            Some("automation"),
            "Automation",
        ),
    ]
    .map(|page| page.child());
    stack.add_titled(
        &pages::overview::page(&poller),
        Some("overview"),
        "Overview",
    );
    stack.add_titled(
        &pages::display::page(&toast_overlay, &poller),
        Some("display"),
        "Display & GPU",
    );
    stack.add_titled(
        &pages::diagnostics::page(&poller),
        Some("diagnostics"),
        "Diagnostics",
    );

    // Global state banners, above whichever page is visible.
    let unreachable_banner = adw::Banner::builder()
        .title(
            "Daemon not reachable — enable it with: systemctl --user enable --now \
             razer-control.socket",
        )
        .revealed(false)
        .build();
    let backend_banner = adw::Banner::new("");
    let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content_box.append(&unreachable_banner);
    content_box.append(&backend_banner);
    stack.set_vexpand(true);
    content_box.append(&stack);
    toast_overlay.set_child(Some(&content_box));

    poller.subscribe(clone!(
        #[weak]
        unreachable_banner,
        #[weak]
        backend_banner,
        move |snapshot: &Snapshot| {
            unreachable_banner.set_revealed(!snapshot.reachable);
            for page in &daemon_pages {
                page.set_sensitive(snapshot.reachable);
            }
            if client::is_mock() {
                backend_banner
                    .set_title("Simulated session — mock daemon, dry-run backend, no hardware");
                backend_banner.set_revealed(true);
            } else if snapshot.status.get("backend").map(String::as_str) == Some("dry-run") {
                backend_banner.set_title(
                    "Dry-run backend — requests are validated and logged, never written to \
                     hardware",
                );
                backend_banner.set_revealed(true);
            } else {
                backend_banner.set_revealed(false);
            }
        }
    ));

    // Sidebar: one row per page, HIG navigation-sidebar styling.
    let sidebar_entries: [(&str, &str, &str); 8] = [
        ("overview", "Overview", "go-home-symbolic"),
        (
            "performance",
            "Performance",
            "power-profile-performance-symbolic",
        ),
        ("cooling", "Cooling", "weather-windy-symbolic"),
        ("battery", "Battery", "battery-good-symbolic"),
        ("lighting", "Lighting", "input-keyboard-symbolic"),
        ("automation", "Automation", "system-run-symbolic"),
        ("display", "Display & GPU", "video-display-symbolic"),
        ("diagnostics", "Diagnostics", "utilities-terminal-symbolic"),
    ];
    let sidebar_list = gtk::ListBox::builder()
        .css_classes(["navigation-sidebar"])
        .build();
    for (_, title, icon) in sidebar_entries {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(6);
        row_box.append(&gtk::Image::from_icon_name(icon));
        row_box.append(&gtk::Label::new(Some(title)));
        sidebar_list.append(&gtk::ListBoxRow::builder().child(&row_box).build());
    }
    sidebar_list.select_row(sidebar_list.row_at_index(0).as_ref());
    stack.set_visible_child_name(sidebar_entries[0].0);

    let sidebar_header = adw::HeaderBar::new();
    let sidebar_toolbar = adw::ToolbarView::new();
    sidebar_toolbar.add_top_bar(&sidebar_header);
    sidebar_toolbar.set_content(Some(&sidebar_list));

    // Content header: current page title plus the primary menu.
    let window_title = adw::WindowTitle::new(sidebar_entries[0].1, "");
    let content_header = adw::HeaderBar::builder()
        .title_widget(&window_title)
        .build();
    let menu = gtk::gio::Menu::new();
    menu.append(Some("About Razer Control"), Some("app.about"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    content_header.pack_end(&menu_button);

    let content_toolbar = adw::ToolbarView::new();
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
        .min_sidebar_width(180.0)
        .max_sidebar_width(240.0)
        .build();

    // Row selection switches the page; when the split view is collapsed
    // (narrow window) it must also push the content pane into view.
    sidebar_list.connect_row_selected(clone!(
        #[weak]
        stack,
        #[weak]
        split,
        #[weak]
        window_title,
        move |_, row| {
            if let Some(row) = row
                && let Some((name, title, _)) = sidebar_entries.get(row.index() as usize)
            {
                stack.set_visible_child_name(name);
                window_title.set_title(title);
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
        .default_width(920)
        .default_height(700)
        .content(&split)
        .build();

    // Keep the window freely resizable: below 620 px the sidebar collapses
    // into an overlay instead of forcing a large minimum width.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        620.0,
        adw::LengthUnit::Px,
    ));
    narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    let about = gtk::gio::ActionEntry::builder("about")
        .activate(clone!(
            #[weak]
            window,
            move |_: &adw::Application, _, _| show_about(&window)
        ))
        .build();
    app.add_action_entries([about]);

    window.present();

    // WSLg's window manager ignores the initial-size request and maps every
    // window at 640x480.  Re-assert the size after present() (before that,
    // the surface is unmapped and WSLg drops the request).  Real desktops
    // never take this branch — they honor default_width/height directly.
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        window.set_default_size(920, 700);
    }

    poller.start(seed);
}

fn show_about(parent: &adw::ApplicationWindow) {
    adw::AboutWindow::builder()
        .transient_for(parent)
        .application_name("Razer Control")
        .application_icon("razer-control-desktop")
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name("razer-control-secureblue")
        .license_type(gtk::License::Gpl20Only)
        .website(REPOSITORY_URL)
        .issue_url(concat!(
            "https://github.com/cyph3rpuNk-dev/razer-control-secureblue",
            "/issues"
        ))
        .comments(
            "Safety-first controller for the Razer Blade 14 (2023) on Atomic/Secureblue \
             Linux. Every request is validated by the daemon against a compiled-in \
             capability table before anything can reach hardware.",
        )
        .build()
        .present();
}
