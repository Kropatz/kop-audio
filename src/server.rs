use std::net::SocketAddr;

use crate::BUF_SIZE;
use bincode::{Decode, Encode, config};
use log::{debug, error, info, warn};
use tokio::net::UdpSocket;

#[derive(Encode, Decode, PartialEq, Debug)]
pub enum Message {
    Audio(Vec<u8>), // decoded audio packet
    Ping,
    Hello(std::net::SocketAddr), // maybe UTF-8
    NewClient(std::net::SocketAddr),
    DeleteClient(std::net::SocketAddr),
    Bye,
    Unknown(Vec<u8>),
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
                    .send_to(&encode_message(&Message::Hello(text)), addr)
                    .await
                {
                    Ok(_) => debug!("Sent hello ack to {}", addr),
                    Err(e) => error!("Error sending hello ack to {}: {:?}", addr, e),
                }
                // if client list already contains the addr, don't notify others
                if is_new_client {
                    debug!("Got new client {}", addr);
                    // Notify other clients about the new client, and the new client about existing clients
                    for client in &clients {
                        if client.addr != addr {
                            // Notify existing clients about the new client
                            let new_client_msg = encode_message(&Message::NewClient(addr));
                            match socket.send_to(&new_client_msg, client.addr).await {
                                Ok(_) => debug!("Sent new client message to {}", client.addr),
                                Err(e) => {
                                    error!(
                                        "Error sending new client msg to {}: {:?}",
                                        client.addr, e
                                    )
                                }
                            }

                            let new_client_msg = encode_message(&Message::NewClient(client.addr));
                            match socket.send_to(&new_client_msg, addr).await {
                                Ok(_) => debug!("Sent new client message to {}", addr),
                                Err(e) => {
                                    error!("Error sending new client msg to {}: {:?}", addr, e)
                                }
                            }
                        }
                    }
                }
            }
            Message::Bye => {
                info!("Received bye from {}", addr);
                remove_client(&mut clients, &addr, &socket).await;
            }
            Message::Unknown(data) => {
                warn!(
                    "Received unknown message type from {}: {} bytes",
                    addr,
                    data.len()
                );
            }
            _ => {}
        }
    }
}

fn contains_client(clients: &Vec<ClientInfo>, addr: &SocketAddr) -> bool {
    for client in clients {
        if &client.addr == addr {
            return true;
        }
    }
    return false;
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
        let bye_msg = encode_message(&Message::Bye);
        match socket.send_to(&bye_msg, addr).await {
            Ok(_) => debug!("Sent bye message to {}", addr),
            Err(e) => error!("Error sending bye message to {}: {:?}", addr, e),
        }
        for client in clients.iter() {
            let delete_msg = encode_message(&Message::DeleteClient(*addr));
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

pub fn decode_message(buf: &[u8]) -> Message {
    if buf.is_empty() {
        return Message::Unknown(Vec::new());
    }

    return bincode::decode_from_slice(buf, config::standard())
        .map(|(msg, _)| msg)
        .unwrap_or(Message::Unknown(buf.to_vec()));
}

pub fn encode_message(msg: &Message) -> Vec<u8> {
    bincode::encode_to_vec(msg, config::standard()).unwrap()
}
