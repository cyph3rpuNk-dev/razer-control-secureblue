//! Small builders shared by the pages.  Everything here composes stock
//! GTK/libadwaita widgets with their built-in style classes (`dim-label`,
//! `numeric`, `card`, …) — no custom fonts, no custom colors.

use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// An `AdwActionRow` whose suffix is a value label the caller updates live.
/// The label starts as an em dash and uses tabular figures so ticking
/// numbers don't wobble.
pub fn value_row(title: &str, subtitle: Option<&str>) -> (adw::ActionRow, gtk::Label) {
    let value = gtk::Label::builder()
        .label("—")
        .css_classes(["dim-label", "numeric"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let row = adw::ActionRow::builder().title(title).build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.add_suffix(&value);
    (row, value)
}

/// An `AdwActionRow` with a horizontal `GtkScale` suffix — the HIG pattern
/// for an in-row slider (brightness and the like).
pub fn scale_row(
    title: &str,
    subtitle: Option<&str>,
    adjustment: &gtk::Adjustment,
) -> (adw::ActionRow, gtk::Scale) {
    let scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .adjustment(adjustment)
        .valign(gtk::Align::Center)
        .width_request(220)
        .draw_value(false)
        .build();
    let row = adw::ActionRow::builder().title(title).build();
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.add_suffix(&scale);
    (row, scale)
}

/// A radio `AdwActionRow`: check-button prefix, whole row activatable.
/// Group each subsequent row with the first via `set_group`.
pub fn radio_row(
    title: &str,
    subtitle: &str,
    group: Option<&gtk::CheckButton>,
) -> (adw::ActionRow, gtk::CheckButton) {
    let check = gtk::CheckButton::builder()
        .valign(gtk::Align::Center)
        .build();
    if let Some(group) = group {
        check.set_group(Some(group));
    }
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable_widget(&check)
        .build();
    row.add_prefix(&check);
    (row, check)
}

/// Cancels the previous pending run and schedules `action` after `delay_ms`
/// — the debounce for sliders whose every tick would otherwise become an IPC
/// request or an external command.
pub fn debounce(
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

/// Fresh debounce handle.
pub fn debouncer() -> Rc<RefCell<Option<glib::SourceId>>> {
    Rc::new(RefCell::new(None))
}

/// Join non-empty parts with " · ", or "—" when there is nothing to show.
pub fn join_or_dash(parts: &[String]) -> String {
    if parts.is_empty() {
        "—".to_owned()
    } else {
        parts.join(" · ")
    }
}
