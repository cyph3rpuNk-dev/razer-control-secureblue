//! KDE Plasma tray (StatusNotifierItem via ksni).  A third thin client:
//! every menu action is one line of the daemon IPC protocol.

#[cfg(unix)]
fn main() {
    unix::run();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("razer-control-tray requires Linux (StatusNotifierItem/D-Bus)");
    std::process::exit(1);
}

#[cfg(unix)]
mod unix {
    use ksni::menu::{MenuItem, StandardItem};
    use ksni::{Tray, TrayService};
    use razer_control_secureblue::daemon_unix::send;

    #[derive(Default)]
    pub struct RazerTray {
        last_response: String,
    }

    impl RazerTray {
        fn request(&mut self, line: &str) {
            self.last_response = match send(line) {
                Ok(response) => response,
                Err(error) => format!("err {error}"),
            };
            eprintln!("{line} -> {}", self.last_response);
        }
    }

    impl Tray for RazerTray {
        fn id(&self) -> String {
            "razer-control-secureblue".into()
        }

        fn icon_name(&self) -> String {
            "razer-control-gui".into()
        }

        fn title(&self) -> String {
            "Razer Control".into()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Razer Control".into(),
                description: if self.last_response.is_empty() {
                    "Razer Blade 14 (2023) — dry-run".into()
                } else {
                    self.last_response.clone()
                },
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Open Razer Control".into(),
                    icon_name: "razer-control-gui".into(),
                    activate: Box::new(|_: &mut Self| {
                        let _ = std::process::Command::new("razer-control-gui").spawn();
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Fan: Automatic".into(),
                    activate: Box::new(|tray: &mut Self| tray.request("fan auto")),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Fan: 3000 RPM".into(),
                    activate: Box::new(|tray: &mut Self| tray.request("fan manual 3000")),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "Charge limit: 80%".into(),
                    activate: Box::new(|tray: &mut Self| tray.request("bho 80")),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|_: &mut Self| std::process::exit(0)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn run() {
        let service = TrayService::new(RazerTray::default());
        service.spawn();
        loop {
            std::thread::park();
        }
    }
}
