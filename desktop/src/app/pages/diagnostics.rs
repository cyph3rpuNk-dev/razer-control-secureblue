//! Diagnostics: how the GUI reaches the daemon, and what it has said.
//! The request log shows every user-initiated IPC line and the daemon's
//! verbatim reply — the fastest way to see why a request was rejected.

use crate::app::poll::Poller;
use crate::app::{client, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use std::rc::Rc;

pub fn page(poller: &Rc<Poller>) -> gtk::Widget {
    let connection_group = adw::PreferencesGroup::builder().title("Connection").build();

    let (transport_row, transport_value) = ui::value_row("Transport", None);
    transport_value.set_text(if client::is_mock() {
        "In-process mock daemon (RAZER_CONTROL_MOCK=1)"
    } else {
        "Daemon socket"
    });
    connection_group.add(&transport_row);

    let socket_path = if client::is_mock() {
        "none — requests never leave the process".to_owned()
    } else {
        razer_control_secureblue::daemon_unix::client_socket_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| error)
    };
    let socket_row = adw::ActionRow::builder()
        .title("Socket path")
        .subtitle(&socket_path)
        .css_classes(["property"])
        .build();
    let copy_path = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy path")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    copy_path.connect_clicked(clone!(
        #[strong]
        socket_path,
        move |button| {
            button.clipboard().set_text(&socket_path);
        }
    ));
    socket_row.add_suffix(&copy_path);
    connection_group.add(&socket_row);

    let (daemon_row, daemon_value) = ui::value_row("Daemon", None);
    connection_group.add(&daemon_row);
    let (backend_row, backend_value) = ui::value_row("Backend", None);
    connection_group.add(&backend_row);

    // Request log: monospace text view inside a card, newest at the bottom.
    let log_group = adw::PreferencesGroup::builder()
        .title("Recent requests")
        .description(
            "Every request this app sent and the daemon's reply. Replies starting \
             with \u{201c}err\u{201d} are policy rejections.",
        )
        .build();
    let log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(12)
        .bottom_margin(12)
        .build();
    let log_scroll = gtk::ScrolledWindow::builder()
        .child(&log_view)
        .min_content_height(220)
        .max_content_height(320)
        .propagate_natural_height(true)
        .css_classes(["card"])
        .build();
    log_group.add(&log_scroll);

    let copy_log = gtk::Button::builder()
        .label("Copy log")
        .halign(gtk::Align::End)
        .margin_top(6)
        .build();
    copy_log.connect_clicked(clone!(
        #[weak]
        log_view,
        move |button| {
            let buffer = log_view.buffer();
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            button.clipboard().set_text(&text);
        }
    ));
    log_group.add(&copy_log);

    // Refresh the connection rows and the log on every poll tick.
    poller.subscribe(move |snapshot| {
        daemon_value.set_text(if snapshot.reachable {
            "Reachable"
        } else {
            "Not reachable — enable with: systemctl --user enable --now razer-control.socket"
        });
        backend_value.set_text(match snapshot.status.get("backend").map(String::as_str) {
            Some("dry-run") => "Dry run — no hardware writes",
            Some("hidraw") => "Hardware (hidraw)",
            Some(other) => other,
            None => "—",
        });

        let entries = client::log_entries();
        let text = if entries.is_empty() {
            "No requests sent yet.".to_owned()
        } else {
            entries
                .iter()
                .map(|entry| {
                    format!(
                        "{}  → {}\n{}  ← {}\n",
                        entry.time, entry.request, entry.time, entry.response
                    )
                })
                .collect::<String>()
        };
        let buffer = log_view.buffer();
        // Avoid resetting the buffer (and the user's scroll/selection) when
        // nothing changed.
        let current = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        if current != text {
            buffer.set_text(&text);
        }
    });

    let page = adw::PreferencesPage::new();
    page.add(&connection_group);
    page.add(&log_group);
    page.upcast()
}
