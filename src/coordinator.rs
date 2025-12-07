use std::{net::SocketAddr, sync::mpsc::{Receiver, Sender}};

use crate::{client::ClientMessage, server::Message};

pub async fn run_coordinator(
    rx_msg: Receiver<ClientMessage>,
    tx_playback: Sender<ClientMessage>,
    tx_record: Sender<ClientMessage>,
    tx_tui: Sender<ClientMessage>,
    tx_net_out: Sender<Message>,
    tx_net_in: Sender<Message>,
) {
    tx_net_out.send(Message::Hello("0.0.0.0:0".parse::<SocketAddr>().unwrap())).unwrap();
    tx_net_out.send(Message::Hello("0.0.0.0:0".parse::<SocketAddr>().unwrap())).unwrap();
    tx_net_out.send(Message::Hello("0.0.0.0:0".parse::<SocketAddr>().unwrap())).unwrap();

    for cmd in rx_msg.iter() {
        match cmd {
            ClientMessage::Connect => {
                tx_tui.send(ClientMessage::Connect).unwrap();
            }
            ClientMessage::Audio(audio) => {
                tx_tui.send(ClientMessage::TransmitAudio(true)).unwrap();
                tx_net_out.send(Message::Audio(audio)).unwrap();
            }
            ClientMessage::RecvAudio(addr, audio) => {
                tx_playback.send(ClientMessage::RecvAudio(addr, audio)).unwrap();
                tx_tui.send(ClientMessage::ShowActive(addr)).unwrap();
            }
            ClientMessage::ToggleMute => {
                tx_record.send(ClientMessage::ToggleMute).unwrap();
            }
            ClientMessage::ToggleDeafen => {
                tx_playback.send(ClientMessage::ToggleDeafen).unwrap();
            }
            ClientMessage::TransmitAudio(status) => {
                tx_tui.send(ClientMessage::TransmitAudio(status)).unwrap();
            }
            ClientMessage::NewClient(addr) => {
                tx_tui.send(ClientMessage::NewClient(addr)).unwrap();
            }
            ClientMessage::DeleteClient(addr) => {
                tx_tui.send(ClientMessage::DeleteClient(addr)).unwrap();
            }
            ClientMessage::Exit => {
                tx_net_out.send(Message::Bye).unwrap();
                tx_net_out.send(Message::Bye).unwrap();
                tx_net_out.send(Message::Bye).unwrap();
                let _ = tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    std::process::exit(0);
                }).await;
            }
            _ => {}
        }
    }
}

pub fn receive_client_message(rx: &Option<Receiver<ClientMessage>>) -> Option<ClientMessage> {
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    } else {
        None
    }
}
pub fn send_client_message(message: ClientMessage, tx: &Option<Sender<ClientMessage>>) {
    if let Some(tx) = tx {
        let _ = tx.send(message);
    }
}
