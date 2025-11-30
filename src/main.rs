use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;
use log::{debug, error, info};
use opus::Application::Voip;
use opus::{Channels, Decoder, Encoder};
use std::fs::OpenOptions;
use std::io::Write;
use std::slice;
use std::sync::Arc;
use tokio::net::{UdpSocket, lookup_host};
use tokio::sync::Mutex;

use crate::implementations::pulseaudio::{PulseAudioConsumer, PulseAudioProducer};
use rand::prelude::*;

mod implementations;
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: usize = 2;
const BUF_SIZE: u32 = 3840; // 20ms of stereo 48kHz 16-bit audio = 48000 samples/sec * 0.02 sec * 2 channels * 2 bytes/sample = 3840 bytes
const FRAME_SIZE: usize = 960; // for opus - 20ms at 48kHz. Per channel, so total samples = FRAME_SIZE * CHANNELS = 1920

#[derive(Debug)]
enum ErrorKind {
    InitializationError,
    InitializationError2(String),
    WriteError(String),
    ReadError,
}
trait AudioProducer {
    fn produce(&mut self, data: &mut [u8]) -> Result<(), ErrorKind>;
}

trait Consumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind>;
}

struct FileConsumer {
    file: std::fs::File,
}

impl FileConsumer {
    fn new(file: &str) -> Result<Self, ErrorKind> {
        match OpenOptions::new().create(true).append(true).open(file) {
            Ok(f) => Ok(FileConsumer { file: f }),
            Err(e) => Err(ErrorKind::WriteError(e.to_string())),
        }
    }
}

impl Consumer for FileConsumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        match self.file.write(data) {
            Ok(bytes_written) => Ok(bytes_written),
            Err(e) => Err(ErrorKind::WriteError(e.to_string())),
        }
    }
}

/// A network consumer that takes audio data and sends it over UDP
#[derive(Clone)]
struct NetworkClient {
    socket: Arc<UdpSocket>,
}

impl NetworkClient {
    async fn new(addr: &str) -> Result<Self, ErrorKind> {
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
            })
            .map_err(|e| ErrorKind::InitializationError2(e.to_string()))?;
        debug!(
            "Socket bound to {}",
            consumer.socket.local_addr().unwrap()
        );
        consumer
            .socket
            .connect(addr)
            .await
            .map_err(|e| ErrorKind::InitializationError2(e.to_string()))?;
        debug!("Socket connected to {}", addr);
        Ok(consumer)
    }
}

impl Consumer for NetworkClient {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        // Note: This is a blocking call; in a real application, consider using async methods
        match self.socket.try_send(data) {
            Ok(bytes_sent) => Ok(bytes_sent),
            Err(e) => Err(ErrorKind::WriteError(e.to_string())),
        }
    }
}

//mod external;
fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut server = false;
        let mut client = false;
        let mut ip = "kopatz.dev:1234".to_string();
        let mut args = std::env::args().skip(1).peekable();

        env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"))
            .init();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--server" => server = true,
                "--client" => client = true,
                "--ip" => {
                    if let Some(val) = args.next() {
                        ip = val;
                    } else {
                        eprintln!("--ip requires an address argument");
                        std::process::exit(1);
                    }
                }
                "--help" => help(),
                "--h" => help(),
                other => {
                    eprintln!("Unknown argument: {}", other);
                    std::process::exit(1);
                }
            }
        }
        if server && client {
            eprintln!("Cannot be both client and server");
            return;
        } else if !server && !client {
            client = true;
        }
        if client {
            let mut network_client = NetworkClient::new(&ip).await.unwrap();
            let client_clone = NetworkClient {
                socket: network_client.socket.clone(),
            };
            tokio::spawn(async move { send_audio(&mut network_client).await });
            receive_audio(client_clone.socket).await;
        } else if server {
            let listener = UdpSocket::bind("0.0.0.0:1234").await.unwrap();
            info!("Listening on 0.0.0.0:1234");
            receive_audio(Arc::new(listener)).await;
        } else {
            eprintln!("Must specify either --client or --server");
        }
    })
}

fn help() {
    println!(
        "Usage: {} [--server|--client] [--ip <address:port>]",
        std::env::args().next().unwrap()
    );
    println!("If neither --server nor --client is specified, defaults to --client.");
    println!("--ip specifies the IP address and port to connect to.");
    std::process::exit(0);
}

async fn receive_audio(listener: Arc<UdpSocket>) {
    let mut audio_consumer = PulseAudioConsumer::new().unwrap();
    info!("Ready to receive audio");
    loop {
        let mut buf = [0u8; BUF_SIZE as usize];
        let (len, addr) = listener.recv_from(&mut buf).await.unwrap();
        debug!("Received {} bytes from {}", len, addr);
        match audio_consumer.consume(&buf[..len]) {
            Ok(_) => {}
            Err(e) => {
                error!("Error consuming data: {:?}", e);
            }
        }
    }
}

async fn send_audio(consumer: &mut NetworkClient) {
    // Can be opened with audacity as raw file, signed 16 bit PCM, 44100 Hz, stereo
    //let mut file_consumer = FileConsumer::new("output.pcm").unwrap();
    //let mut audio_consumer = PulseAudioConsumer::new().unwrap();
    let mut audio_producer = PulseAudioProducer::new().unwrap();
    let consumers: &mut [&mut dyn Consumer] = &mut [consumer];
    let mut data = vec![0u8; BUF_SIZE as usize];
    let mut encoded_data = [0u8; BUF_SIZE as usize];
    let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
    let mut encoder = opus_encoder();
    let mut decoder = opus_decoder();
    // Get an RNG:
    let mut rng = rand::rng();
    loop {
        match audio_producer.produce(&mut data) {
            Ok(_) => {}
            Err(_) => {
                error!("Error reading from stream");
                break;
            }
        }

        let pcm: &[i16] =
            unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };

        let samples_needed = FRAME_SIZE * CHANNELS;
        let pcm = &pcm[..samples_needed];
        let n = encoder.encode(&pcm, &mut encoded_data).unwrap();
        if (rng.random_range(0..100)) < 0 {
            error!("Simulating packet loss");
            continue;
        }
        let b = decoder
            .decode(&encoded_data[..n], &mut decoded_data, false)
            .unwrap();

        debug!(
            "Read {} samples, data has {} samples, encoded to {} bytes, decoded to {} samples",
            pcm.len(),
            data.len() / 2,
            n,
            b
        );
        consumers.iter_mut().for_each(|c| {
            match c.consume(unsafe {
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
        });
    }
}

fn opus_encoder() -> Encoder {
    Encoder::new(SAMPLE_RATE, Channels::Stereo, Voip).unwrap()
}
fn opus_decoder() -> Decoder {
    Decoder::new(SAMPLE_RATE, Channels::Stereo).unwrap()
}
