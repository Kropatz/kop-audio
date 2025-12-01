use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, mpsc};

use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;
use log::info;
use tokio::net::{UdpSocket, lookup_host};

use crate::client::{NetworkClient, receive_audio, send_audio};
use crate::server::server_loop;

mod client;
mod implementations;
mod server;
mod tui;
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

#[derive(Debug, Default)]
pub struct ClientState {
    sending_audio: bool,
    connected: bool,
    mute: bool,
    deafen: bool,
    exit: bool,
}

trait AudioProducer {
    fn produce(&mut self, data: &mut [u8]) -> Result<(), ErrorKind>;
}

trait Consumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind>;
}

//mod external;
fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut server = false;
        let mut client = false;
        let mut tui = true;
        let mut ip = "kopatz.dev:1234".to_string();
        let mut args = std::env::args().skip(1).peekable();
        let (tx_tui, rx_cl): (Sender<client::TuiMessage>, Receiver<client::TuiMessage>) = mpsc::channel();
        let (tx_cl, rx_tui): (Sender<client::TuiMessage>, Receiver<client::TuiMessage>) = mpsc::channel();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--server" => server = true,
                "--client" => client = true,
                "--no-tui" => tui = false,
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
        if (!tui) {
            env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"))
                .init();
        } else {
            env_logger::Builder::new()
                .filter_level(log::LevelFilter::Off)
                .init();
        }
        if server && client {
            eprintln!("Cannot be both client and server");
            return;
        } else if !server && !client {
            client = true;
        }
        if client {
            //todo: some way to mute and deafen
            let state = Arc::new(ClientState::default());

            let mut network_client;
            if (tui) {
                network_client = NetworkClient::new(&ip, Some(tx_cl), Some(rx_cl)).await.unwrap();
            } else {
                network_client = NetworkClient::new(&ip, None, None).await.unwrap();
            }
            let socket = network_client.socket.clone();
            tokio::spawn(async move { client::send_audio(&mut network_client).await });
            if tui {
                tokio::spawn(async { client::receive_audio(socket).await });
                tui::App::new(tx_tui, rx_tui);
            } else {
                client::receive_audio(socket).await;
            }
        } else if server {
            let listener = UdpSocket::bind("0.0.0.0:1234").await.unwrap();
            info!("Listening on 0.0.0.0:1234");
            //receive_audio(Arc::new(listener)).await;
            server::server_loop(listener).await;
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
