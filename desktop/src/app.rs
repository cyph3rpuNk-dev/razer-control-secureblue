//! GTK4/libadwaita UI shell.  Linux-only; see `main.rs` for the platform
//! gate.
//!
//! The app follows the GNOME HIG: five top-level views in an `AdwViewStack`
//! switched from the header bar (Synapse-style), pages built from stock
//! libadwaita widgets, the system light/dark scheme and fonts, and global
//! banners for the two states a user must never mistake — the daemon being
//! unreachable, and a backend that does not write to hardware.  Diagnostics
//! lives in the primary menu as its own window.  Every control is a thin
//! IPC client; policy lives in the daemon.

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

    // Five top-level views, Synapse-style.  Pages that drive the daemon are
    // disabled while it is unreachable; Overview, Display, and the
    // Diagnostics window stay usable.
    let stack = adw::ViewStack::new();
    stack.add_titled_with_icon(
        &pages::overview::page(&poller),
        Some("overview"),
        "Overview",
        "go-home-symbolic",
    );
    let performance_page = stack.add_titled_with_icon(
        &pages::performance::page(&seed, &toast_overlay, &poller),
        Some("performance"),
        "Performance",
        "power-profile-performance-symbolic",
    );
    stack.add_titled_with_icon(
        &pages::display::page(&toast_overlay, &poller),
        Some("display"),
        "Display",
        "video-display-symbolic",
    );
    let battery_page = stack.add_titled_with_icon(
        &pages::battery::page(&seed, &toast_overlay),
        Some("battery"),
        "Battery",
        "battery-good-symbolic",
    );
    let lighting_page = stack.add_titled_with_icon(
        &pages::lighting::page(&seed, &toast_overlay, &poller),
        Some("lighting"),
        "Lighting",
        "input-keyboard-symbolic",
    );
    let daemon_pages = [performance_page, battery_page, lighting_page].map(|page| page.child());

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

    // Header bar: the view switcher as the title widget (Bazaar/Synapse
    // style), the primary menu on the right.
    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Diagnostics"), Some("app.diagnostics"));
    menu.append(Some("About Razer Control"), Some("app.about"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);

    // On narrow windows the switcher moves to a bottom bar; the breakpoint
    // below swaps the header title and reveals it.
    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&stack).build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast_overlay));
    toolbar.add_bottom_bar(&switcher_bar);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Razer Control")
        .icon_name("razer-control-desktop")
        .default_width(920)
        .default_height(700)
        .content(&toolbar)
        .build();

    // Keep the window freely resizable: below 620 px the five header pills
    // no longer fit, so the switcher drops to the bottom bar and the header
    // shows the plain app title.
    let narrow_title = adw::WindowTitle::new("Razer Control", "");
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        620.0,
        adw::LengthUnit::Px,
    ));
    narrow.add_setter(&header, "title-widget", Some(&narrow_title.to_value()));
    narrow.add_setter(&switcher_bar, "reveal", Some(&true.to_value()));
    window.add_breakpoint(narrow);

    // Diagnostics: secondary tooling, one hide-on-close window behind the
    // primary menu so its poller subscription is created exactly once.
    let diagnostics_toolbar = adw::ToolbarView::new();
    diagnostics_toolbar.add_top_bar(&adw::HeaderBar::new());
    diagnostics_toolbar.set_content(Some(&pages::diagnostics::page(&poller)));
    let diagnostics_window = adw::Window::builder()
        .title("Diagnostics")
        .default_width(640)
        .default_height(560)
        .hide_on_close(true)
        .content(&diagnostics_toolbar)
        .build();
    diagnostics_window.set_transient_for(Some(&window));

    let about = gtk::gio::ActionEntry::builder("about")
        .activate(clone!(
            #[weak]
            window,
            move |_: &adw::Application, _, _| show_about(&window)
        ))
        .build();
    let diagnostics = gtk::gio::ActionEntry::builder("diagnostics")
        .activate(clone!(
            #[weak]
            diagnostics_window,
            move |_: &adw::Application, _, _| diagnostics_window.present()
        ))
        .build();
    app.add_action_entries([about, diagnostics]);

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
