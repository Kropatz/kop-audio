use std::fs::File;
use std::io::Write;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, mpsc};

use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;
use log::{LevelFilter, info};
use tokio::net::UdpSocket;
use tokio::signal;

use crate::audio::{play_audio, record_audio};
use crate::client::NetworkClient;
use crate::coordinator::run_coordinator;
use crate::implementations::pulseaudio::{PulseAudioConsumer, PulseAudioProducer};

mod audio;
mod client;
mod coordinator;
mod implementations;
mod server;
mod tui;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: usize = 2;
const BUF_SIZE: u32 = 3840; // 20ms of stereo 48kHz 16-bit audio = 48000 samples/sec * 0.02 sec * 2 channels * 2 bytes/sample = 3840 bytes
const MSG_SIZE: u32 = BUF_SIZE + 1;
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
        let mut tui = false;
        let mut tui_set = false;
        let mut debug = false;
        let mut ip = "kopatz.dev:1234".to_string();
        let mut args = std::env::args().skip(1).peekable();
        let (tx_msg, rx_msg): (
            Sender<client::ClientMessage>,
            Receiver<client::ClientMessage>,
        ) = mpsc::channel();
        let (tx_tui, rx_tui): (
            Sender<client::ClientMessage>,
            Receiver<client::ClientMessage>,
        ) = mpsc::channel();
        let (tx_record, rx_record): (
            Sender<client::ClientMessage>,
            Receiver<client::ClientMessage>,
        ) = mpsc::channel();
        let (tx_playback, rx_playback): (
            Sender<client::ClientMessage>,
            Receiver<client::ClientMessage>,
        ) = mpsc::channel();
        let (tx_net_out, rx_net_out): (Sender<server::Message>, Receiver<server::Message>) =
            mpsc::channel();

        let (tx_net_in, rx_net_in): (Sender<server::Message>, Receiver<server::Message>) =
            mpsc::channel();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--server" => server = true,
                "--client" => {
                    client = true;
                    tui = true;
                }
                "--no-tui" => {
                    tui = false;
                    tui_set = true;
                }
                "--ip" => {
                    if let Some(val) = args.next() {
                        ip = val;
                    } else {
                        eprintln!("--ip requires an address argument");
                        std::process::exit(1);
                    }
                }
                "--debug" => debug = true,
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
            if !tui_set {
                tui = true;
            }
        }
        if !tui {
            env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"))
                .init();
        } else {
            if debug {
                let target = Box::new(File::create("/tmp/log.txt").expect("Can't create file"));
                env_logger::Builder::new()
                    .filter(None, LevelFilter::Debug)
                    .target(env_logger::Target::Pipe(target))
                    .format(|buf, record| {
                        writeln!(
                            buf,
                            "[{} {} {}:{}] {}",
                            "now",
                            record.level(),
                            record.file().unwrap_or("unknown"),
                            record.line().unwrap_or(0),
                            record.args()
                        )
                    })
                    .init();
            } else {
                env_logger::Builder::new()
                    .filter_level(log::LevelFilter::Off)
                    .init();
            }
        }
        if client {
            //todo: some way to mute and deafen
            let mut audio_consumer = PulseAudioConsumer::new().unwrap();
            let mut audio_producer = PulseAudioProducer::new().unwrap();
            let tx_msg_clone = tx_msg.clone();
            tokio::spawn(async move { record_audio(tx_msg_clone, &mut audio_producer, rx_record) });
            tokio::spawn(async move { play_audio(rx_playback, &mut audio_consumer) });
            let network_client = NetworkClient::new(&ip, tx_msg.clone()).await.unwrap();
            network_client.start(rx_net_in, rx_net_out).await;
            if tui {
                tokio::spawn(async move { tui::App::new(rx_tui, tx_msg) });
            }
            run_coordinator(
                rx_msg,
                tx_playback.clone(),
                tx_record.clone(),
                tx_tui.clone(),
                tx_net_out.clone(),
                tx_net_in.clone(),
            )
            // TODO: wait for ctrl-c in non-tui mode, send Bye to server
            // TODO: probably need a mpmc channel for that
            //match signal::ctrl_c().await {
            //    Ok(()) => {
            //        std::process::exit(0);
            //    }
            //    Err(err) => {
            //        eprintln!("Unable to listen for shutdown signal: {}", err);
            //        // we also shut down in case of error
            //    }
            //}
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
