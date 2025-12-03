use log::{debug, error, info, warn};
use opus::Encoder;
use std::mem;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use tokio::net::{UdpSocket, lookup_host};

use crate::server::{Message, decode_message, encode_message};
use crate::{BUF_SIZE, ErrorKind, MSG_SIZE, client};

/// A network consumer that takes audio data and sends it over UDP
pub struct NetworkClient {
    pub socket: Arc<UdpSocket>,
    hangover: usize,
    hangover_limit: usize,
    muted: bool,

    // communication with TUI
    tx: Sender<ClientMessage>,
}
pub enum ClientMessage {
    Connect,
    Disconnect,
    ToggleMute,
    ToggleDeafen,
    Audio(Vec<u8>),
    RecvAudio(Vec<u8>),
    TransmitAudio(bool),
    NewClient(std::net::SocketAddr),
    DeleteClient(std::net::SocketAddr),
    Exit,
}

impl NetworkClient {
    pub async fn new(addr: &str, tx: Sender<ClientMessage>) -> Result<Self, ErrorKind> {
        info!("Connecting to {}", addr);
        let result = lookup_host(addr)
            .await
            .map_err(|e| ErrorKind::InitializationError2(e.to_string()))?;
        let addr = result
            .into_iter()
            .next()
            .ok_or(ErrorKind::InitializationError)?;
        debug!("Connecting to {}", addr);
        let consumer = UdpSocket::bind("0.0.0.0:0")
            .await
            .map(|s| NetworkClient {
                socket: Arc::new(s),
                hangover: 0,
                hangover_limit: 10, // number of consecutive silent frames to send before stopping
                muted: false,
                tx: tx,
            })
            .map_err(|e| ErrorKind::InitializationError2(e.to_string()))?;
        debug!("Socket bound to {}", consumer.socket.local_addr().unwrap());
        consumer
            .socket
            .connect(addr)
            .await
            .map_err(|e| ErrorKind::InitializationError2(e.to_string()))?;

        Ok(consumer)
    }

    pub async fn start(
        mut self,
        rx_receive_audio: Receiver<Message>,
        rx_net_out: Receiver<Message>,
    ) -> () {
        let socket1 = self.socket.clone();
        let socket2 = self.socket.clone();
        let tx1 = self.tx.clone();
        let tx2 = self.tx.clone();

        tokio::spawn(async move { client::send_udp(socket1, tx1, rx_net_out).await });
        tokio::spawn(async move { client::receive_udp(socket2, rx_receive_audio, tx2).await });
    }
}

pub async fn send_udp(
    socket: Arc<UdpSocket>,
    tx: Sender<client::ClientMessage>,
    rx: Receiver<Message>,
) {
    for msg in rx.iter() {
        match socket.try_send(&encode_message(&msg)) {
            Ok(bytes_sent) => {
                debug!(
                    "Sent {} bytes, msg type {:?}",
                    bytes_sent,
                    mem::discriminant(&msg)
                );
            }
            Err(e) => error!("{:?}", ErrorKind::WriteError(e.to_string())),
        }
    }
}

pub async fn receive_udp(
    socket: Arc<UdpSocket>,
    rx_receive_audio: Receiver<Message>,
    tx: Sender<client::ClientMessage>,
) {
    let mut data = [0u8; MSG_SIZE as usize];
    loop {
        let (len, addr) = socket.recv_from(&mut data).await.unwrap();
        let msg = decode_message(&data[..len]);
        debug!("Received message of type {:?}", msg);
        match msg {
            Message::Audio(encoded_data) => {
                let _ = tx.send(ClientMessage::RecvAudio(encoded_data));
            }
            Message::NewClient(addr) => {
                let _ = tx.send(ClientMessage::NewClient(addr));
            }
            Message::DeleteClient(addr) => {
                let _ = tx.send(ClientMessage::DeleteClient(addr));
            }
            _ => {}
        }
    }
}

//pub async fn receive_udp(
//    socket: Arc<UdpSocket>,
//    rx_receive_audio: Receiver<Message>,
//    tx: Sender<client::ClientMessage>,
//) {

//let mut audio_consumer = PulseAudioConsumer::new().unwrap();
//let mut decoder = opus_decoder();
//let mut data = [0u8; MSG_SIZE as usize];
//let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
//let mut deafened = false;
//info!("Ready to receive audio");
//loop {
//    match receive_client_message(&rx_receive_audio) {
//        Some(client::ClientMessage::ToggleDeafen) => {
//            deafened = !deafened;
//        }
//        Some(client::ClientMessage::Exit) => {
//            // TODO: doesn't work
//            socket.try_send(&encode_message(Message::Bye)).unwrap();
//            send_client_message(ClientMessage::Disconnect, &tx);
//            debug!("Exiting receive_audio loop");
//        }
//        _ => {}
//    }
//    let (len, addr) = socket.recv_from(&mut data).await.unwrap();

//    let msg = decode_message(&data[..len]);
//    debug!("Received message of type {:?}", msg);
//    match msg {
//        Message::Audio(encoded_data) => {
//            if deafened {
//                debug!("Client is deafened, not playing audio");
//                continue;
//            }
//            debug!("Received {} bytes from {}", len, addr);
//            let b = decoder
//                .decode(&encoded_data, &mut decoded_data, false)
//                .unwrap();
//            match audio_consumer.consume(unsafe {
//                slice::from_raw_parts(
//                    decoded_data.as_ptr() as *const u8,
//                    b * CHANNELS * std::mem::size_of::<i16>(),
//                )
//            }) {
//                Ok(_) => {}
//                Err(e) => {
//                    error!("Error consuming data: {:?}", e);
//                }
//            }
//        }
//        Message::NewClient(addr) => {
//            let _ = send_client_message(client::ClientMessage::NewClient(addr), &tx);
//        }
//        Message::DeleteClient(addr) => {
//            let _ = send_client_message(client::ClientMessage::DeleteClient(addr), &tx);
//        }
//        Message::Bye => {
//            std::process::exit(0);
//        }
//        _ => {}
//    }

//    send_client_message(ClientMessage::Connect, &tx);
//}
//}

//impl Consumer for NetworkClient {
//    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
//        //match receive_client_message(&self.rx_send_audio) {
//        //    Some(client::ClientMessage::ToggleMute) => {
//        //        self.muted = !self.muted;
//        //    }
//        //    _ => {}
//        //}
//        //if self.muted {
//        //    debug!("Client is muted, not sending audio");
//        //    send_client_message(client::ClientMessage::TransmitAudio(false), &self.tx);
//        //    return Ok(0);
//        //}
//        //let pcm: &[i16] =
//        //    unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };
//
//        //let samples_needed = FRAME_SIZE * CHANNELS;
//        //let pcm = &pcm[..samples_needed];
//        //if is_silence(pcm, 200.0) {
//        //    if self.hangover == 0 {
//        //        send_client_message(client::ClientMessage::TransmitAudio(false), &self.tx);
//        //        return Ok(0);
//        //    }
//        //    self.hangover -= 1;
//        //} else {
//        //    self.hangover = self.hangover_limit;
//        //}
//        //debug!("Acive audio detected, sending packet");
//        //let n = self.encoder.encode(&pcm, &mut self.encoded_data).unwrap();
//
//        //debug!(
//        //    "Read {} samples, data has {} samples, encoded to {} bytes,",
//        //    pcm.len(),
//        //    data.len() / 2,
//        //    n,
//        //);
//        //send_client_message(client::ClientMessage::TransmitAudio(true), &self.tx);
//        //match self.socket.try_send(&encode_message(Message::Audio(
//        //    self.encoded_data[..n].to_vec(),
//        //))) {
//        //    Ok(bytes_sent) => {
//        //        debug!("Sent {} bytes", bytes_sent);
//        //        Ok(bytes_sent)
//        //    }
//        //    Err(e) => Err(ErrorKind::WriteError(e.to_string())),
//        //}
//    }
//}
