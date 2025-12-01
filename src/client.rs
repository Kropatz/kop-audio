use log::{debug, error, info, warn};
use opus::Application::Voip;
use opus::{Channels, Decoder, Encoder};
use std::slice;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, mpsc};
use tokio::net::{UdpSocket, lookup_host};

use crate::implementations::pulseaudio::{PulseAudioConsumer, PulseAudioProducer};
use crate::server::{Message, MessageType, decode_message, encode_message};
use crate::{
    AudioProducer, BUF_SIZE, CHANNELS, Consumer, ErrorKind, FRAME_SIZE, SAMPLE_RATE, client,
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
        let _ = consumer
            .socket
            .try_send(&encode_message(MessageType::Hello, &[]));
        debug!("Socket connected to {}", addr);

        Ok(consumer)
    }

    pub async fn start(mut self, is_tui: bool, rx_receive_audio: Option<Receiver<client::TuiMessage>>) -> () {
        let socket = self.socket.clone();

        tokio::spawn(async move { client::send_audio(&mut self).await });
        if is_tui {
            tokio::spawn(async move { client::receive_audio(socket, rx_receive_audio).await });
        } else {
            client::receive_audio(socket, rx_receive_audio).await;
        }
    }

    fn send_tui_message(&self, message: client::TuiMessage) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(message);
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
            self.send_tui_message(client::TuiMessage::TransmitAudio(false));
            return Ok(0);
        }
        let pcm: &[i16] =
            unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };

        let samples_needed = FRAME_SIZE * CHANNELS;
        let pcm = &pcm[..samples_needed];
        if is_silence(pcm, 200.0) {
            if self.hangover == 0 {
                self.send_tui_message(client::TuiMessage::TransmitAudio(false));
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
        self.send_tui_message(client::TuiMessage::TransmitAudio(true));
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
    listener: Arc<UdpSocket>,
    rx_receive_audio: Option<Receiver<client::TuiMessage>>,
) {
    let mut audio_consumer = PulseAudioConsumer::new().unwrap();
    let mut decoder = opus_decoder();
    let mut data = [0u8; BUF_SIZE as usize + 1]; // MSG type byte
    let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
    let mut deafened = false;
    info!("Ready to receive audio");
    loop {
        match receive_tui_message(&rx_receive_audio) {
            Some(client::TuiMessage::ToggleDeafen) => {
                deafened = !deafened;
            }
            _ => {}
        }
        let (len, addr) = listener.recv_from(&mut data).await.unwrap();
        if deafened {
            debug!("Client is deafened, not playing audio");
            continue;
        }

        let msg = decode_message(&data[..len]);
        match msg {
            Message::Audio(encoded_data) => {
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
            _ => {}
        }
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
