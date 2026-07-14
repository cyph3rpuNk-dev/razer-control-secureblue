//! Synapse-styled control GUI.  Still a thin client: every action is one
//! line of the daemon IPC protocol and all safety decisions stay in the
//! daemon.  The GUI holds no policy and no privileged code.

mod transport;

use iced::border;
use iced::theme::Palette;
use iced::widget::{button, column, container, horizontal_rule, row, slider, text};
use iced::{Background, Color, Element, Length, Size, Task, Theme};
use razer_control_secureblue::BLADE_14_2023;

use transport::Transport;

const GREEN: Color = Color::from_rgb(
    0x44 as f32 / 255.0,
    0xd6 as f32 / 255.0,
    0x2c as f32 / 255.0,
);
const PAGE_BG: Color = Color::from_rgb(
    0x1a as f32 / 255.0,
    0x1a as f32 / 255.0,
    0x1a as f32 / 255.0,
);
const CARD_BG: Color = Color::from_rgb(
    0x10 as f32 / 255.0,
    0x10 as f32 / 255.0,
    0x10 as f32 / 255.0,
);
const CARD_INNER: Color = Color::from_rgb(
    0x1c as f32 / 255.0,
    0x1c as f32 / 255.0,
    0x1c as f32 / 255.0,
);
const TEXT: Color = Color::from_rgb(
    0xd0 as f32 / 255.0,
    0xd0 as f32 / 255.0,
    0xd0 as f32 / 255.0,
);
const DIM: Color = Color::from_rgb(
    0x8a as f32 / 255.0,
    0x8a as f32 / 255.0,
    0x8a as f32 / 255.0,
);

fn main() -> iced::Result {
    let force_mock = std::env::args().any(|argument| argument == "--mock");
    iced::application("Razer Control Secureblue", App::update, App::view)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: Size::new(880.0, 620.0),
            icon: iced::window::icon::from_rgba(app_icon_rgba(), 32, 32).ok(),
            ..Default::default()
        })
        .run_with(move || {
            let mut app = App::new(transport::choose(force_mock));
            app.refresh();
            (app, Task::none())
        })
}

/// Procedural window/taskbar icon: a green fan ring on a dark rounded tile.
fn app_icon_rgba() -> Vec<u8> {
    let size = 32i32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let centre = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            let corner = dx.abs().max(dy.abs());
            let (r, g, b, a) = if corner > 15.0 {
                (0, 0, 0, 0)
            } else if (9.0..=13.0).contains(&distance) || distance <= 4.0 {
                (0x44, 0xd6, 0x2c, 0xff)
            } else {
                (0x10, 0x10, 0x10, 0xff)
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    rgba
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Performance,
    Battery,
    Lighting,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Performance, Tab::Battery, Tab::Lighting];

    fn label(self) -> &'static str {
        match self {
            Tab::Performance => "PERFORMANCE",
            Tab::Battery => "BATTERY",
            Tab::Lighting => "LIGHTING",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanChoice {
    Auto,
    Manual,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    FanChoice(FanChoice),
    FanRpm(u16),
    ApplyFan,
    Bho(u8),
    ApplyBho,
    Refresh,
}

struct App {
    transport: Box<dyn Transport>,
    tab: Tab,
    fan_choice: FanChoice,
    fan_rpm: u16,
    bho: u8,
    daemon_status: String,
    last_response: String,
}

impl App {
    fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            tab: Tab::Performance,
            fan_choice: FanChoice::Auto,
            fan_rpm: u16::midpoint(
                BLADE_14_2023.fan_range.min_rpm,
                BLADE_14_2023.fan_range.max_rpm,
            ),
            bho: 80,
            daemon_status: String::new(),
            last_response: String::new(),
        }
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "razer".to_owned(),
            Palette {
                background: PAGE_BG,
                text: TEXT,
                primary: GREEN,
                success: GREEN,
                danger: Color::from_rgb(0.86, 0.25, 0.25),
            },
        )
    }

    fn send(&mut self, line: &str) {
        self.last_response = match self.transport.request(line) {
            Ok(response) => response,
            Err(error) => format!("err {error}"),
        };
    }

    fn refresh(&mut self) {
        self.daemon_status = match self.transport.request("status") {
            Ok(response) => response,
            Err(error) => format!("err {error}"),
        };
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::TabSelected(tab) => self.tab = tab,
            Message::FanChoice(choice) => self.fan_choice = choice,
            Message::FanRpm(rpm) => self.fan_rpm = rpm,
            Message::ApplyFan => {
                let line = match self.fan_choice {
                    FanChoice::Auto => "fan auto".to_owned(),
                    FanChoice::Manual => format!("fan manual {}", self.fan_rpm),
                };
                self.send(&line);
                self.refresh();
            }
            Message::Bho(limit) => self.bho = limit,
            Message::ApplyBho => {
                self.send(&format!("bho {}", self.bho));
                self.refresh();
            }
            Message::Refresh => self.refresh(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let body = match self.tab {
            Tab::Performance => self.performance_tab(),
            Tab::Battery => self.battery_tab(),
            Tab::Lighting => self.lighting_tab(),
        };

        column![
            self.header(),
            horizontal_rule(1),
            container(body)
                .padding(24)
                .width(Length::Fill)
                .height(Length::Fill),
            horizontal_rule(1),
            self.footer(),
        ]
        .into()
    }

    fn header(&self) -> Element<'_, Message> {
        let title = container(text("RAZER BLADE 14").size(14).color(TEXT))
            .center_x(Length::Fill)
            .padding([10, 0]);

        let mut nav = row![].spacing(10);
        for tab in Tab::ALL {
            let selected = tab == self.tab;
            nav = nav.push(
                button(
                    text(tab.label())
                        .size(12)
                        .color(if selected { Color::BLACK } else { DIM }),
                )
                .padding([6, 16])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(Background::Color(if selected {
                        GREEN
                    } else {
                        Color::TRANSPARENT
                    })),
                    text_color: if selected { Color::BLACK } else { DIM },
                    border: border::rounded(999),
                    ..button::Style::default()
                })
                .on_press(Message::TabSelected(tab)),
            );
        }

        column![title, container(nav).center_x(Length::Fill).padding([0, 0])]
            .spacing(2)
            .into()
    }

    fn performance_tab(&self) -> Element<'_, Message> {
        let range = BLADE_14_2023.fan_range;

        let mode_card = |label: &'static str, caption: &'static str, choice: FanChoice| {
            let selected = self.fan_choice == choice;
            button(
                column![
                    text(label)
                        .size(15)
                        .color(if selected { GREEN } else { TEXT }),
                    text(caption).size(11).color(DIM),
                ]
                .spacing(6),
            )
            .padding(18)
            .width(190)
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(Background::Color(CARD_INNER)),
                text_color: TEXT,
                border: iced::Border {
                    color: if selected {
                        GREEN
                    } else {
                        Color::from_rgb(0.2, 0.2, 0.2)
                    },
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..button::Style::default()
            })
            .on_press(Message::FanChoice(choice))
        };

        let slider_enabled = self.fan_choice == FanChoice::Manual;
        let mut fan_slider =
            slider(range.min_rpm..=range.max_rpm, self.fan_rpm, Message::FanRpm).step(100u16);
        if !slider_enabled {
            fan_slider = slider(range.min_rpm..=range.max_rpm, self.fan_rpm, |_| {
                Message::Refresh
            })
            .step(100u16);
        }

        let fan_card = card(
            "FAN",
            column![
                row![
                    mode_card("Automatic", "EC-managed cooling", FanChoice::Auto),
                    mode_card("Manual", "Fixed RPM, failsafe protected", FanChoice::Manual),
                ]
                .spacing(14),
                row![
                    fan_slider,
                    value_bubble(format!("{} RPM", self.fan_rpm), slider_enabled),
                ]
                .spacing(14),
                text(format!(
                    "Range {}–{} RPM. The daemon rejects anything outside it; manual mode \
                     reverts to automatic if the daemon stops.",
                    range.min_rpm, range.max_rpm
                ))
                .size(11)
                .color(DIM),
                apply_button("APPLY", Message::ApplyFan),
            ]
            .spacing(16),
        );

        let experimental_card = card(
            "CPU / GPU BOOST",
            column![
                text(
                    "Locked. Boost and GPU TDP stay disabled until the safe controls have \
                     on-device mileage; the daemon rejects them without an explicit opt-in.",
                )
                .size(11)
                .color(DIM),
            ]
            .spacing(10),
        );

        column![fan_card, experimental_card].spacing(18).into()
    }

    fn battery_tab(&self) -> Element<'_, Message> {
        card(
            "BATTERY HEALTH OPTIMIZER",
            column![
                text("Battery will stop charging when it has reached the limit (%).")
                    .size(12)
                    .color(TEXT),
                row![
                    slider(50..=80u8, self.bho, Message::Bho),
                    value_bubble(format!("{}%", self.bho), true),
                ]
                .spacing(14),
                row![
                    text("50").size(11).color(DIM),
                    container(text("").size(11)).width(Length::Fill),
                    text("80").size(11).color(DIM),
                ],
                apply_button("APPLY", Message::ApplyBho),
            ]
            .spacing(14),
        )
        .into()
    }

    fn lighting_tab(&self) -> Element<'_, Message> {
        card(
            "LIGHTING",
            column![
                text(
                    "Keyboard brightness and Chroma effects arrive with the HID protocol \
                      import (next milestone). No mock controls are shown for hardware the \
                      daemon cannot drive yet."
                )
                .size(12)
                .color(DIM),
            ]
            .spacing(10),
        )
        .into()
    }

    fn footer(&self) -> Element<'_, Message> {
        let device = BLADE_14_2023;
        row![
            text(format!(
                "{} ({:04x}:{:04x}) via {}",
                device.name,
                device.id.vendor_id,
                device.id.product_id,
                self.transport.label()
            ))
            .size(11)
            .color(DIM),
            container(
                text(if self.last_response.is_empty() {
                    self.daemon_status.clone()
                } else {
                    self.last_response.clone()
                })
                .size(11)
                .color(TEXT)
            )
            .align_right(Length::Fill),
            button(text("REFRESH").size(11).color(DIM))
                .padding([4, 10])
                .style(|_theme: &Theme, _status| button::Style {
                    background: None,
                    text_color: DIM,
                    border: iced::Border {
                        color: Color::from_rgb(0.25, 0.25, 0.25),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..button::Style::default()
                })
                .on_press(Message::Refresh),
        ]
        .spacing(12)
        .padding(12)
        .into()
    }
}

fn card<'a>(title: &'static str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![text(title).size(13).color(GREEN), body.into()].spacing(14))
        .padding(20)
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(CARD_BG)),
            border: border::rounded(10),
            ..container::Style::default()
        })
        .into()
}

fn value_bubble<'a>(value: String, active: bool) -> Element<'a, Message> {
    container(
        text(value)
            .size(12)
            .color(if active { Color::BLACK } else { DIM }),
    )
    .padding([4, 10])
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(if active { GREEN } else { CARD_INNER })),
        border: border::rounded(4),
        ..container::Style::default()
    })
    .into()
}

fn apply_button(label: &'static str, message: Message) -> Element<'static, Message> {
    button(text(label).size(12).color(Color::BLACK))
        .padding([8, 22])
        .style(|_theme: &Theme, status| {
            let background = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Color::from_rgb(0.35, 0.95, 0.28)
                }
                _ => GREEN,
            };
            button::Style {
                background: Some(Background::Color(background)),
                text_color: Color::BLACK,
                border: border::rounded(999),
                ..button::Style::default()
            }
        })
        .on_press(message)
        .into()
}
