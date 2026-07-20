//! The one background poll loop: every 2 seconds fetch `status` and
//! `telemetry` off-thread, then fan the parsed snapshot out to whichever
//! pages subscribed.  Pages use it for live display only — controls are
//! seeded once at build time and never overwritten by a poll, so a slider
//! mid-drag is never yanked away.

use super::client;
use gtk::glib;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Default, Clone)]
pub struct Snapshot {
    /// False when the daemon socket could not be reached.
    pub reachable: bool,
    pub status: HashMap<String, String>,
    pub telemetry: HashMap<String, String>,
}

impl Snapshot {
    /// Blocking fetch; run on a worker (or once during startup).
    pub fn fetch_blocking() -> Self {
        let status = client::request_blocking("status");
        let telemetry = client::request_blocking("telemetry");
        Snapshot {
            reachable: status.is_ok(),
            status: status
                .as_deref()
                .map(client::parse_fields)
                .unwrap_or_default(),
            telemetry: telemetry
                .as_deref()
                .map(client::parse_fields)
                .unwrap_or_default(),
        }
    }

    pub fn status_is(&self, key: &str, value: &str) -> bool {
        self.status.get(key).map(String::as_str) == Some(value)
    }
}

type Listener = Box<dyn Fn(&Snapshot)>;

/// Fan-out point for snapshots.  Lives on the main thread (`Rc`).
#[derive(Default)]
pub struct Poller {
    listeners: RefCell<Vec<Listener>>,
}

impl Poller {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn subscribe(&self, listener: impl Fn(&Snapshot) + 'static) {
        self.listeners.borrow_mut().push(Box::new(listener));
    }

    fn notify(&self, snapshot: &Snapshot) {
        for listener in self.listeners.borrow().iter() {
            listener(snapshot);
        }
    }

    /// Start the 2 s loop.  `seed` is the snapshot the pages were built from;
    /// it is delivered immediately so live labels fill before the first tick.
    pub fn start(self: &Rc<Self>, seed: Snapshot) {
        self.notify(&seed);
        let poller = Rc::clone(self);
        glib::spawn_future_local(async move {
            loop {
                glib::timeout_future_seconds(2).await;
                let snapshot = gtk::gio::spawn_blocking(Snapshot::fetch_blocking)
                    .await
                    .unwrap_or_default();
                poller.notify(&snapshot);
            }
        });
    }
}
