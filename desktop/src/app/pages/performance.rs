//! Performance: the overall power/acoustics profile, CPU and GPU power
//! levels when the Custom profile is active, plus the fan controls and
//! power-source fan rules from the cooling/automation modules.  The profile
//! groups are experimental-gated: the daemon rejects them without
//! `--experimental`, and the UI locks them with an explanatory banner.  The
//! fan groups are verified and always available.

use super::{EXPERIMENTAL_LOCKED, automation, cooling};
use crate::app::poll::{Poller, Snapshot};
use crate::app::{client, ui};
use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use razer_control_secureblue::ipc::parse_profile;
use razer_control_secureblue::{BoostLevel, Profile};
use std::rc::Rc;

const PROFILES: [(&str, &str, &str); 4] = [
    ("Silent", "Lowest fan noise, capped power", "profile silent"),
    (
        "Balanced",
        "The everyday default — sensible power, sensible acoustics",
        "profile balanced",
    ),
    (
        "Gaming",
        "Maximum sustained CPU and GPU power",
        "profile gaming",
    ),
    ("Custom", "Pick CPU and GPU power levels yourself", ""),
];
const LEVEL_TOKENS: [&str; 4] = ["low", "medium", "high", "boost"];

pub fn page(seed: &Snapshot, overlay: &adw::ToastOverlay, poller: &Rc<Poller>) -> gtk::Widget {
    let experimental = seed.status_is("experimental", "true");
    let current = seed
        .status
        .get("profile")
        .map_or("balanced", String::as_str);
    let parsed = parse_profile(current);
    let initial_profile = match parsed {
        Some(Profile::Silent) => 0,
        Some(Profile::Gaming) => 2,
        Some(Profile::Custom { .. }) => 3,
        _ => 1,
    };

    let banner = adw::Banner::builder()
        .title(EXPERIMENTAL_LOCKED)
        .revealed(!experimental)
        .build();

    // Profile selection as radio rows.
    let profile_group = adw::PreferencesGroup::builder()
        .title("PERFORMANCE MODES")
        .description("Switches the EC's power and acoustics profile.")
        .build();
    let mut checks: Vec<gtk::CheckButton> = Vec::with_capacity(PROFILES.len());
    for (title, subtitle, _) in PROFILES {
        let (row, check) = ui::radio_row(title, subtitle, checks.first());
        profile_group.add(&row);
        checks.push(check);
    }

    // CPU/GPU power levels, active only on the Custom profile.
    let custom_group = adw::PreferencesGroup::builder()
        .title("CUSTOM POWER LEVELS")
        .description(
            "Higher levels raise sustained power and heat. Applied with the Custom \
             profile.",
        )
        .build();
    let cpu_row = adw::ComboRow::builder()
        .title("CPU power")
        .model(&gtk::StringList::new(&["Low", "Medium", "High", "Boost"]))
        .build();
    let gpu_row = adw::ComboRow::builder()
        .title("GPU power")
        .model(&gtk::StringList::new(&["Low", "Medium", "High"]))
        .build();
    custom_group.add(&cpu_row);
    custom_group.add(&gpu_row);

    // Initial selection mirrors the daemon before any handler is connected.
    if let Some(Profile::Custom { cpu, gpu }) = parsed {
        cpu_row.set_selected(u32::from(cpu.wire_value()));
        gpu_row.set_selected(u32::from(gpu.wire_value()));
    } else {
        cpu_row.set_selected(BoostLevel::Medium.wire_value().into());
        gpu_row.set_selected(BoostLevel::Medium.wire_value().into());
    }
    checks[initial_profile].set_active(true);
    profile_group.set_sensitive(experimental);
    custom_group.set_sensitive(experimental && initial_profile == 3);

    let compose_custom = {
        let cpu_row = cpu_row.clone();
        let gpu_row = gpu_row.clone();
        move || {
            format!(
                "profile custom cpu {} gpu {}",
                LEVEL_TOKENS[cpu_row.selected() as usize],
                LEVEL_TOKENS[gpu_row.selected() as usize]
            )
        }
    };

    let custom_check = checks[3].clone();
    for (index, check) in checks.iter().enumerate() {
        let compose_custom = compose_custom.clone();
        check.connect_toggled(clone!(
            #[weak]
            overlay,
            #[weak]
            custom_group,
            #[weak]
            custom_check,
            move |check| {
                custom_group.set_sensitive(experimental && custom_check.is_active());
                if !check.is_active() {
                    return;
                }
                let line = if index == 3 {
                    compose_custom()
                } else {
                    PROFILES[index].2.to_owned()
                };
                client::send(&overlay, line, |_| {});
            }
        ));
    }
    for combo in [&cpu_row, &gpu_row] {
        let compose_custom = compose_custom.clone();
        combo.connect_selected_notify(clone!(
            #[weak]
            overlay,
            #[weak]
            custom_check,
            move |_| {
                if custom_check.is_active() {
                    client::send(&overlay, compose_custom(), |_| {});
                }
            }
        ));
    }

    let page = adw::PreferencesPage::new();
    page.add(&profile_group);
    page.add(&custom_group);
    page.add(&cooling::group(seed, overlay));
    page.add(&automation::group(seed, overlay, poller));
    page.set_vexpand(true);

    // Banner above the scrolled content, full width, HIG-style.
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.append(&banner);
    container.append(&page);
    container.upcast()
}
