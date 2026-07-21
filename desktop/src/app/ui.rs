//! Small builders shared by the pages.  Everything here composes stock
//! GTK/libadwaita widgets with their built-in style classes (`dim-label`,
//! `numeric`, `card`, …) plus the accent-derived classes documented in
//! `resources/style.css` (`hero-card`, `stat-tile`, `chip`) — no custom
//! fonts, no hardcoded colors.

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

/// The tint of a status chip, mapped to Adwaita's semantic colors.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    Neutral,
    Accent,
    Success,
    Warning,
}

impl ChipKind {
    fn class(self) -> Option<&'static str> {
        match self {
            Self::Neutral => None,
            Self::Accent => Some("chip-accent"),
            Self::Success => Some("chip-success"),
            Self::Warning => Some("chip-warning"),
        }
    }
}

/// A pill "chip" label, tinted from the Adwaita named colors via the
/// `chip` classes in style.css.  Update live with [`set_chip`].
pub fn chip(text: &str, kind: ChipKind) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .css_classes(["chip", "caption"])
        .valign(gtk::Align::Center)
        .build();
    if let Some(class) = kind.class() {
        label.add_css_class(class);
    }
    label
}

/// Retext and retint an existing chip.
pub fn set_chip(label: &gtk::Label, text: &str, kind: ChipKind) {
    label.set_text(text);
    for class in ["chip-accent", "chip-success", "chip-warning"] {
        label.remove_css_class(class);
    }
    if let Some(class) = kind.class() {
        label.add_css_class(class);
    }
}

/// The hero card returned by [`hero`]: append chips to `chips`, keep
/// `subtitle` for live status text.
pub struct Hero {
    pub root: gtk::Widget,
    pub subtitle: gtk::Label,
    pub chips: gtk::Box,
}

/// A hero header card: big title, dim subtitle, and a chip row on the
/// left; an optional caller-supplied widget (device portrait) trailing.
/// The accent wash and padding come from the `hero-card` class.
pub fn hero(title: &str, trailing: Option<&gtk::Widget>) -> Hero {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(24)
        .css_classes(["card", "hero-card"])
        .build();

    let text_column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title = gtk::Label::builder()
        .label(title)
        .css_classes(["title-1"])
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let subtitle = gtk::Label::builder()
        .label("—")
        .css_classes(["dim-label"])
        .halign(gtk::Align::Start)
        .build();
    let chips = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(6)
        .build();
    text_column.append(&title);
    text_column.append(&subtitle);
    text_column.append(&chips);
    root.append(&text_column);

    if let Some(widget) = trailing {
        widget.set_halign(gtk::Align::End);
        widget.set_valign(gtk::Align::Center);
        root.append(widget);
    }

    Hero {
        root: root.upcast(),
        subtitle,
        chips,
    }
}

/// A stat tile for an overview grid: title, then an accent-coloured icon
/// and value on one line, and an optional detail line that stays hidden
/// until it is given text.  Returns `(tile, icon, value, detail)`; the
/// icon is returned so state tiles can swap it live.
pub fn stat_tile(title: &str, icon_name: &str) -> (gtk::Box, gtk::Image, gtk::Label, gtk::Label) {
    let tile = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .css_classes(["stat-tile"])
        .build();
    let title = gtk::Label::builder()
        .label(title)
        .css_classes(["dim-label"])
        .halign(gtk::Align::Start)
        .build();
    let icon = gtk::Image::builder()
        .icon_name(icon_name)
        .css_classes(["accent"])
        .build();
    let value = gtk::Label::builder()
        .label("—")
        .css_classes(["title-3", "numeric", "accent"])
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let reading = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Start)
        .build();
    reading.append(&icon);
    reading.append(&value);
    let detail = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .visible(false)
        .build();
    tile.append(&title);
    tile.append(&reading);
    tile.append(&detail);
    (tile, icon, value, detail)
}

/// Join non-empty parts with " · ", or "—" when there is nothing to show.
pub fn join_or_dash(parts: &[String]) -> String {
    if parts.is_empty() {
        "—".to_owned()
    } else {
        parts.join(" · ")
    }
}
