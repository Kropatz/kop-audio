use log::debug;
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{Block, Paragraph, Widget},
};
use std::{
    io::Result,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use crate::{ClientState, client};

#[derive(Debug)]
pub struct App {
    client_state: ClientState,

    main_widget: UserListWidget,

    rx: Receiver<client::TuiMessage>,
    tx_send_audio: Sender<client::TuiMessage>,
    tx_receive_audio: Sender<client::TuiMessage>,
}

impl App {
    pub fn new(
        rx: Receiver<client::TuiMessage>,
        tx_send_audio: Sender<client::TuiMessage>,
        tx_receive_audio: Sender<client::TuiMessage>,
    ) {
        let mut app = App {
            client_state: ClientState::default(),
            rx,
            tx_send_audio,
            tx_receive_audio,
            main_widget: UserListWidget { users: vec![] }
        };
        let terminal = ratatui::init();
        let result = app.run(terminal);
        ratatui::restore();
    }

    fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let mut should_draw = true;
        while !self.client_state.exit {
            if should_draw {
                terminal.draw(|frame| self.draw(frame))?;
            }
            should_draw = self.handle_tui_messages();
            if let Ok(true) = event::poll(Duration::from_millis(100)) {
                self.handle_event(event::read()?);
                should_draw = true;
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints(vec![Constraint::Min(5), Constraint::Percentage(100)])
            .spacing(-1)
            .split(frame.area());
        frame.render_widget(self, layout[0]);
        frame.render_widget(&self.main_widget, layout[1]);
    }

    fn handle_tui_messages(&mut self) -> bool {
        let mut updated = false;
        while let Ok(message) = self.rx.try_recv() {
            match message {
                client::TuiMessage::Connect => {
                    self.client_state.connected = true;
                }
                client::TuiMessage::Disconnect => {
                    self.client_state.connected = false;
                    self.client_state.sending_audio = false;
                }
                client::TuiMessage::TransmitAudio(sending) => {
                    self.client_state.sending_audio = sending;
                }
                client::TuiMessage::NewClient(addr)=> {
                    self.main_widget.users.push(addr.to_string());
                }
                client::TuiMessage::DeleteClient(addr)=> {
                    self.main_widget.users.retain(|user| user != &addr.to_string());
                }
                _ => {}
            }
            updated = true;
        }
        updated
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                match key_event.code {
                    event::KeyCode::Char('d') | event::KeyCode::Char('D') => {
                        self.client_state.deafen = !self.client_state.deafen;
                        let _ = self.tx_receive_audio.send(client::TuiMessage::ToggleDeafen);
                    }
                    event::KeyCode::Char('m') | event::KeyCode::Char('M') => {
                        self.client_state.mute = !self.client_state.mute;
                        let _ = self.tx_send_audio.send(client::TuiMessage::ToggleMute);
                    }
                    event::KeyCode::Char('q') | event::KeyCode::Char('Q') => {
                        self.client_state.exit = true;
                        let _ = self.tx_receive_audio.send(client::TuiMessage::Exit);
                        debug!("Exiting TUI upon user request");
                    }
                    _ => {}
                }
            }
            _ => {}
        };
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut status_line = vec![" WapplaTalk ".bold()];
        let mutOrDeafen = self.client_state.mute || self.client_state.deafen;
        status_line.push("| ".into());
        if self.client_state.connected {
            status_line.push("Connected ".green())
        } else {
            status_line.push("Disconnected ".red())
        };
        if mutOrDeafen {
            status_line.push("(".into());
        }
        if self.client_state.mute {
            status_line.push(" Muted".yellow())
        }
        if self.client_state.deafen {
            if self.client_state.mute {
                status_line.push(",".into());
            }
            status_line.push(" Deafened".yellow())
        }
        if mutOrDeafen {
            status_line.push(" ) ".into());
        }
        status_line.push("| ".into());
        if self.client_state.sending_audio {
            status_line.push("Sending Audio ".green())
        } else {
            status_line.push("Not Sending Audio ".red())
        };

        let status_line = Line::from(status_line);
        let instructions = Line::from(vec![
            " Mute ".into(),
            "<M>".blue().bold(),
            " Deafen ".into(),
            "<D>".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);

        let layout = Layout::default()
            .spacing(1)
            .direction(ratatui::layout::Direction::Vertical)
            .constraints(vec![Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(area);

        Paragraph::new(status_line.centered()).render(layout[0], buf);
        Paragraph::new(instructions.centered()).render(layout[1], buf);
    }
}

#[derive(Debug)]
struct UserListWidget {
    users: Vec<String>,
}

impl Widget for &UserListWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title("Users")
            .border_set(border::THICK);
        let inner_area = block.inner(area);
        let user_lines: Vec<Line> = self
            .users
            .iter()
            .map(|user| Line::from(user.as_str()))
            .collect();
        let paragraph = Paragraph::new(Text::from(user_lines));
        block.render(area, buf);
        paragraph.render(inner_area, buf);
    }
}
