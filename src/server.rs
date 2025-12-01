use crate::BUF_SIZE;
use log::{debug, error, info, warn};
use tokio::net::UdpSocket;

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum MessageType {
    Audio = 1,
    Ping = 2,
    Hello = 3,
    Bye = 4,
}

#[derive(Debug)]
pub enum Message<'a> {
    Audio(&'a [u8]), // decoded audio packet
    Ping,
    Hello(&'a str), // maybe UTF-8
    Bye,
    Unknown(u8, &'a [u8]),
}

struct ClientInfo {
    addr: std::net::SocketAddr,
    last_active: std::time::Instant,
}

pub async fn server_loop(listener: UdpSocket) {
    let mut buf = [0u8; BUF_SIZE as usize];
    let mut clients: Vec<ClientInfo> = Vec::new();
    loop {
        let (len, addr) = match listener.recv_from(&mut buf).await {
            Ok(res) => res,
            Err(e) => {
                error!("Error receiving data: {:?}", e);
                continue;
            }
        };
        let mut is_new_client = true;
        for client in &mut clients {
            if client.addr == addr {
                client.last_active = std::time::Instant::now();
                is_new_client = false;
            }
        }
        if is_new_client {
            info!("New client connected: {}", addr);
            clients.push(ClientInfo {
                addr,
                last_active: std::time::Instant::now(),
            });
        }
        //todo: clean up inactive clients periodically
        let msg = decode_message(&buf[..len]);
        match msg {
            Message::Audio(data) => {
                debug!(
                    "Received audio packet of {} bytes from {}",
                    data.len(),
                    addr
                );
                for client in &clients {
                    if client.addr != addr {
                        match listener.send_to(&buf[..len], client.addr).await {
                            Ok(_) => println!("Forwarded audio packet to {}", client.addr),
                            Err(e) => error!("Error forwarding audio to {}: {:?}", client.addr, e),
                        }
                    }
                }
                // Here you would handle the audio data, e.g., play it or forward it
            }
            Message::Ping => {
                debug!("Received ping from {}", addr);
                // Handle ping
            }
            Message::Hello(text) => {
                info!("Received hello from {}: {}", addr, text);
                // Handle hello
            }
            Message::Bye => {
                info!("Received bye from {}", addr);
                // Handle bye
            }
            Message::Unknown(kind, data) => {
                warn!(
                    "Received unknown message type {} from {}: {} bytes",
                    kind,
                    addr,
                    data.len()
                );
            }
            _ => {}
        }
    }
}

pub fn decode_message(buf: &[u8]) -> Message<'_> {
    if buf.is_empty() {
        return Message::Unknown(0, buf);
    }

    let kind = buf[0];
    let payload = &buf[1..];

    match kind {
        x if x == MessageType::Audio as u8 => Message::Audio(payload),
        x if x == MessageType::Ping as u8 => Message::Ping,
        x if x == MessageType::Hello as u8 => {
            let text = std::str::from_utf8(payload).unwrap_or("");
            Message::Hello(text)
        }
        x if x == MessageType::Bye as u8 => Message::Bye,
        other => Message::Unknown(other, payload),
    }
}

pub fn encode_message(msg_type: MessageType, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(msg_type as u8); // 1-byte message kind marker
    out.extend_from_slice(payload);
    out
}
