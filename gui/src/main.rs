//! Thin control GUI: every action is one line of the daemon IPC protocol,
//! and every safety decision stays in the daemon.  The GUI holds no policy
//! and no privileged code.

mod transport;

use iced::widget::{button, column, container, horizontal_rule, radio, row, slider, text};
use iced::{Element, Task};
use razer_control_secureblue::BLADE_14_2023;

use transport::Transport;

fn main() -> iced::Result {
    let force_mock = std::env::args().any(|argument| argument == "--mock");
    iced::application("Razer Control Secureblue", App::update, App::view)
        .window_size((460.0, 560.0))
        .run_with(move || {
            let mut app = App::new(transport::choose(force_mock));
            app.refresh();
            (app, Task::none())
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanChoice {
    Auto,
    Manual,
}

#[derive(Debug, Clone)]
enum Message {
    FanChoice(FanChoice),
    FanRpm(u16),
    ApplyFan,
    Bho(u8),
    ApplyBho,
    Refresh,
}

struct App {
    transport: Box<dyn Transport>,
    fan_choice: FanChoice,
    fan_rpm: u16,
    bho: u8,
    daemon_status: String,
    last_response: String,
}

impl App {
    fn new(transport: Box<dyn Transport>) -> Self {
        let mid_rpm = u16::midpoint(
            BLADE_14_2023.fan_range.min_rpm,
            BLADE_14_2023.fan_range.max_rpm,
        );
        Self {
            transport,
            fan_choice: FanChoice::Auto,
            fan_rpm: mid_rpm,
            bho: 80,
            daemon_status: String::new(),
            last_response: String::new(),
        }
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
        let device = BLADE_14_2023;
        let range = device.fan_range;

        let header = column![
            text(device.name).size(22),
            text(format!(
                "{:04x}:{:04x} — via {}",
                device.id.vendor_id,
                device.id.product_id,
                self.transport.label()
            ))
            .size(13),
            text(&self.daemon_status).size(13),
        ]
        .spacing(4);

        let fan = column![
            text("Fan").size(17),
            row![
                radio(
                    "Automatic",
                    FanChoice::Auto,
                    Some(self.fan_choice),
                    Message::FanChoice
                ),
                radio(
                    "Manual",
                    FanChoice::Manual,
                    Some(self.fan_choice),
                    Message::FanChoice
                ),
            ]
            .spacing(20),
            row![
                slider(range.min_rpm..=range.max_rpm, self.fan_rpm, Message::FanRpm).step(100u16),
                text(format!("{} RPM", self.fan_rpm)).size(14),
            ]
            .spacing(12),
            text(format!(
                "Verified range: {}–{} RPM. Out-of-range values are rejected by the daemon.",
                range.min_rpm, range.max_rpm
            ))
            .size(12),
            button("Apply fan profile").on_press(Message::ApplyFan),
        ]
        .spacing(8);

        let bho = column![
            text("Battery Health Optimizer").size(17),
            row![
                slider(50..=80u8, self.bho, Message::Bho),
                text(format!("{}%", self.bho)).size(14),
            ]
            .spacing(12),
            text("Caps charging between 50% and 80% to slow battery wear.").size(12),
            button("Apply charge limit").on_press(Message::ApplyBho),
        ]
        .spacing(8);

        let experimental = column![
            text("Experimental").size(17),
            text(
                "CPU/GPU boost and GPU TDP controls are disabled. They stay locked \
                 until the safe controls have on-device mileage; the daemon rejects \
                 them without an explicit opt-in."
            )
            .size(12),
            button("Boost / GPU TDP (locked)"),
        ]
        .spacing(8);

        let footer = column![
            row![button("Refresh status").on_press(Message::Refresh)].spacing(12),
            text(if self.last_response.is_empty() {
                "No commands sent yet.".to_owned()
            } else {
                format!("Last response: {}", self.last_response)
            })
            .size(13),
        ]
        .spacing(8);

        container(
            column![
                header,
                horizontal_rule(1),
                fan,
                horizontal_rule(1),
                bho,
                horizontal_rule(1),
                experimental,
                horizontal_rule(1),
                footer,
            ]
            .spacing(14),
        )
        .padding(18)
        .into()
    }
}
