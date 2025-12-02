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
    NewClient = 5,
    DeleteClient = 6,
}

#[derive(Debug)]
pub enum Message<'a> {
    Audio(&'a [u8]), // decoded audio packet
    Ping,
    Hello(&'a str), // maybe UTF-8
    NewClient(&'a [u8]),
    DeleteClient(&'a [u8]),
    Bye,
    Unknown(u8, &'a [u8]),
}

struct ClientInfo {
    addr: std::net::SocketAddr,
    last_active: std::time::Instant,
}

pub async fn server_loop(socket: UdpSocket) {
    let mut buf = [0u8; BUF_SIZE as usize];
    let mut clients: Vec<ClientInfo> = Vec::new();
    let mut check_counter = 0;
    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
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
        check_counter += 1;
        if check_counter >= 100 {
            let now = std::time::Instant::now();
            let to_remove: Vec<std::net::SocketAddr> = clients
                .iter()
                .filter(|client| now.duration_since(client.last_active).as_secs() >= 500)
                .map(|client| client.addr)
                .collect();
            for addr in &to_remove {
                remove_client(&mut clients, addr, &socket).await;
            }
            debug!(
                "Cleaned up inactive clients. Before: {}, After: {}",
                to_remove.len(),
                clients.len()
            );
            check_counter = 0;
        }
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
                        match socket.send_to(&buf[..len], client.addr).await {
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
                // send all clients the new client's hello message
                match socket
                    .send_to(&encode_message(MessageType::Hello, text.as_bytes()), addr)
                    .await
                {
                    Ok(_) => debug!("Sent hello ack to {}", addr),
                    Err(e) => error!("Error sending hello ack to {}: {:?}", addr, e),
                }
                // Notify other clients about the new client, and the new client about existing clients
                for client in &clients {
                    if client.addr != addr {
                        // Notify existing clients about the new client
                        let new_client_msg =
                            encode_message(MessageType::NewClient, addr.to_string().as_bytes());
                        match socket.send_to(&new_client_msg, client.addr).await {
                            Ok(_) => debug!("Sent new client message to {}", client.addr),
                            Err(e) => {
                                error!("Error sending new client msg to {}: {:?}", client.addr, e)
                            }
                        }

                        let new_client_msg = encode_message(
                            MessageType::NewClient,
                            client.addr.to_string().as_bytes(),
                        );
                        match socket.send_to(&new_client_msg, addr).await {
                            Ok(_) => debug!("Sent new client message to {}", addr),
                            Err(e) => error!("Error sending new client msg to {}: {:?}", addr, e),
                        }
                    }
                }
            }
            Message::Bye => {
                info!("Received bye from {}", addr);
                remove_client(&mut clients, &addr, &socket).await;
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

async fn remove_client(
    clients: &mut Vec<ClientInfo>,
    addr: &std::net::SocketAddr,
    socket: &UdpSocket,
) {
    let size_before = clients.len();
    clients.retain(|client| {
        if &client.addr == addr {
            debug!("Removing client {}", addr);
            false
        } else {
            true
        }
    });
    if clients.len() < size_before {
        let bye_msg = encode_message(MessageType::Bye, &[]);
        match socket.send_to(&bye_msg, addr).await {
            Ok(_) => debug!("Sent bye message to {}", addr),
            Err(e) => error!("Error sending bye message to {}: {:?}", addr, e),
        }
        for client in clients.iter() {
            let delete_msg = encode_message(MessageType::DeleteClient, addr.to_string().as_bytes());
            match socket.send_to(&delete_msg, client.addr).await {
                Ok(_) => debug!("Sent delete client message to {}", client.addr),
                Err(e) => error!(
                    "Error sending delete client msg to {}: {:?}",
                    client.addr, e
                ),
            }
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
        x if x == MessageType::NewClient as u8 => Message::NewClient(payload),
        x if x == MessageType::DeleteClient as u8 => Message::DeleteClient(payload),
        other => Message::Unknown(other, payload),
    }
}

pub fn encode_message(msg_type: MessageType, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + payload.len());
    out.push(msg_type as u8); // 1-byte message kind marker
    out.extend_from_slice(payload);
    out
}
