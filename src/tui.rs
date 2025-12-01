use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyEventKind},
    layout::Rect,
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
        };
        let terminal = ratatui::init();
        let result = app.run(terminal);
        ratatui::restore();
    }

    fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.client_state.exit {
            self.handle_tui_messages();
            terminal.draw(|frame| self.draw(frame))?;
            if let Ok(true) = event::poll(Duration::from_millis(100)) {
                self.handle_event(event::read()?);
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_tui_messages(&mut self) {
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
                _ => {}
            }
        }
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
                        //let _ = self.tx.send(client::TuiMessage::Exit);
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

        let block = Block::bordered()
            .title(status_line.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        Paragraph::new("").centered().block(block).render(area, buf);
    }
}
