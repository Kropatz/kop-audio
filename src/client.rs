use log::{debug, error, info, warn};
use opus::Application::Voip;
use opus::{Channels, Decoder, Encoder};
use std::slice;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tokio::net::{UdpSocket, lookup_host};
use tokio::time::timeout;

use crate::channel_util::send_tui_message;
use crate::implementations::pulseaudio::{PulseAudioConsumer, PulseAudioProducer};
use crate::server::{Message, MessageType, decode_message, encode_message};
use crate::{
    AudioProducer, BUF_SIZE, CHANNELS, Consumer, ErrorKind, FRAME_SIZE, MSG_SIZE, SAMPLE_RATE,
    channel_util, client,
};

/// A network consumer that takes audio data and sends it over UDP
pub struct NetworkClient {
    pub socket: Arc<UdpSocket>,
    encoded_data: [u8; BUF_SIZE as usize],
    encoder: Encoder,
    hangover: usize,
    hangover_limit: usize,
    muted: bool,

    // communication with TUI
    tx: Option<Sender<client::TuiMessage>>,
    rx_send_audio: Option<Receiver<client::TuiMessage>>,
}
pub enum TuiMessage {
    Connect,
    Disconnect,
    ToggleMute,
    ToggleDeafen,
    TransmitAudio(bool),
    NewClient(std::net::SocketAddr),
    DeleteClient(std::net::SocketAddr),
    Exit,
}
fn receive_tui_message(rx: &Option<Receiver<client::TuiMessage>>) -> Option<client::TuiMessage> {
    if let Some(rx) = rx {
        match rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    } else {
        None
    }
}

impl NetworkClient {
    pub async fn new(
        addr: &str,
        tx: Option<Sender<client::TuiMessage>>,
        rx_send_audio: Option<Receiver<client::TuiMessage>>,
    ) -> Result<Self, ErrorKind> {
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
                encoded_data: [0u8; BUF_SIZE as usize],
                encoder: opus_encoder(),
                hangover: 0,
                hangover_limit: 10, // number of consecutive silent frames to send before stopping
                muted: false,
                tx: tx,
                rx_send_audio: rx_send_audio,
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

    //TODO: rethink architecture here
    //  Maybe all data should be sent to a queue, and receive_audio should read from it?
    //  To avoid blocking on socket.recv_from in receive_audio and handle tui messages
    pub async fn start(
        mut self,
        is_tui: bool,
        rx_receive_audio: Option<Receiver<client::TuiMessage>>,
    ) -> () {
        let socket = self.socket.clone();
        let tx = self.tx.clone();

        tokio::spawn(async move { client::send_audio(&mut self).await });
        if is_tui {
            tokio::spawn(async move { client::receive_audio(socket, rx_receive_audio, tx).await });
        } else {
            client::receive_audio(socket, rx_receive_audio, tx).await;
        }
    }
}

impl Consumer for NetworkClient {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        match receive_tui_message(&self.rx_send_audio) {
            Some(client::TuiMessage::ToggleMute) => {
                self.muted = !self.muted;
            }
            _ => {}
        }
        if self.muted {
            debug!("Client is muted, not sending audio");
            send_tui_message(client::TuiMessage::TransmitAudio(false), &self.tx);
            return Ok(0);
        }
        let pcm: &[i16] =
            unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };

        let samples_needed = FRAME_SIZE * CHANNELS;
        let pcm = &pcm[..samples_needed];
        if is_silence(pcm, 200.0) {
            if self.hangover == 0 {
                send_tui_message(client::TuiMessage::TransmitAudio(false), &self.tx);
                return Ok(0);
            }
            self.hangover -= 1;
        } else {
            self.hangover = self.hangover_limit;
        }
        debug!("Acive audio detected, sending packet");
        let n = self.encoder.encode(&pcm, &mut self.encoded_data).unwrap();

        debug!(
            "Read {} samples, data has {} samples, encoded to {} bytes,",
            pcm.len(),
            data.len() / 2,
            n,
        );
        // Note: This is a blocking call; in a real application, consider using async methods
        send_tui_message(client::TuiMessage::TransmitAudio(true), &self.tx);
        match self
            .socket
            .try_send(&encode_message(MessageType::Audio, &self.encoded_data[..n]))
        {
            Ok(bytes_sent) => {
                debug!("Sent {} bytes", bytes_sent);
                Ok(bytes_sent)
            }
            Err(e) => Err(ErrorKind::WriteError(e.to_string())),
        }
    }
}

pub async fn receive_audio(
    socket: Arc<UdpSocket>,
    rx_receive_audio: Option<Receiver<client::TuiMessage>>,
    tx: Option<Sender<client::TuiMessage>>,
) {
    socket
        .try_send(&encode_message(MessageType::Hello, &[]))
        .unwrap();

    let mut audio_consumer = PulseAudioConsumer::new().unwrap();
    let mut decoder = opus_decoder();
    let mut data = [0u8; MSG_SIZE as usize];
    let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
    let mut deafened = false;
    info!("Ready to receive audio");
    loop {
        match receive_tui_message(&rx_receive_audio) {
            Some(client::TuiMessage::ToggleDeafen) => {
                deafened = !deafened;
            }
            Some(client::TuiMessage::Exit) => {
                // TODO: doesn't work
                socket.try_send(&encode_message(MessageType::Bye, &[])).unwrap();
                send_tui_message(TuiMessage::Disconnect, &tx);
                debug!("Exiting receive_audio loop");
            }
            _ => {}
        }
        let (len, addr) = socket.recv_from(&mut data).await.unwrap();

        let msg = decode_message(&data[..len]);
        debug!("Received message of type {:?}", msg);
        match msg {
            Message::Audio(encoded_data) => {
                if deafened {
                    debug!("Client is deafened, not playing audio");
                    continue;
                }
                debug!("Received {} bytes from {}", len, addr);
                let b = decoder
                    .decode(&encoded_data[..len - 1], &mut decoded_data, false)
                    .unwrap();
                match audio_consumer.consume(unsafe {
                    slice::from_raw_parts(
                        decoded_data.as_ptr() as *const u8,
                        b * CHANNELS * std::mem::size_of::<i16>(),
                    )
                }) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error consuming data: {:?}", e);
                    }
                }
            }
            Message::NewClient(encoded_data) => {
                let addr_str = String::from_utf8_lossy(encoded_data);
                if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
                    info!("New client connected: {}", addr);
                    let _ = send_tui_message(client::TuiMessage::NewClient(addr), &tx);
                }
            }
            Message::DeleteClient(encoded_data) => {
                let addr_str = String::from_utf8_lossy(encoded_data);
                if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
                    info!("Client disconnected: {}", addr);
                    let _ = send_tui_message(client::TuiMessage::DeleteClient(addr), &tx);
                }
            }
            Message::Bye => {
                std::process::exit(0);
            }
            _ => {}
        }

        send_tui_message(TuiMessage::Connect, &tx);
    }
}

pub async fn send_audio(consumer: &mut NetworkClient) {
    //let mut audio_consumer = PulseAudioConsumer::new().unwrap();
    let mut audio_producer = PulseAudioProducer::new().unwrap();
    let consumers: &mut [&mut dyn Consumer] = &mut [consumer];
    let mut data = vec![0u8; BUF_SIZE as usize];
    loop {
        match audio_producer.produce(&mut data) {
            Ok(_) => {}
            Err(_) => {
                error!("Error reading from stream");
                break;
            }
        }

        consumers.iter_mut().for_each(|c| match c.consume(&data) {
            Ok(_) => {}
            Err(e) => {
                error!("Error consuming data: {:?}", e);
            }
        });
    }
}

fn opus_encoder() -> Encoder {
    Encoder::new(SAMPLE_RATE, Channels::Stereo, Voip).unwrap()
}
fn opus_decoder() -> Decoder {
    Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap()
}

fn is_silence(pcm: &[i16], threshold: f32) -> bool {
    if pcm.is_empty() {
        return true;
    }

    let mut sum = 0f64;
    for &s in pcm {
        sum += (s as f64) * (s as f64);
    }

    let rms = (sum / pcm.len() as f64).sqrt();
    rms < threshold as f64
}
