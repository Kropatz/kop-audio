use libpulse_binding as pulse;
use libpulse_binding::def::BufferAttr;
use libpulse_simple_binding as psimple;
use opus::Application::Voip;
use opus::{Channels, Decoder, Encoder};
use psimple::Simple;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use std::fs::OpenOptions;
use std::io::Write;
use std::slice;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: usize = 2;
const BUF_SIZE: u32 = 3840; // 20ms of stereo 48kHz 16-bit audio = 48000 samples/sec * 0.02 sec * 2 channels * 2 bytes/sample = 3840 bytes
const FRAME_SIZE: usize = 960; // for opus - 20ms at 48kHz. Per channel, so total samples = FRAME_SIZE * CHANNELS = 1920

#[derive(Debug)]
enum ErrorKind {
    WriteError,
    ReadError,
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
            Err(_) => Err(ErrorKind::WriteError),
        }
    }
}

struct AudioConsumer {
    endpoint: psimple::Simple,
}

impl Consumer for FileConsumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        match self.file.write(data) {
            Ok(bytes_written) => Ok(bytes_written),
            Err(_) => Err(ErrorKind::WriteError),
        }
    }
}

impl Consumer for AudioConsumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        match self.endpoint.write(data) {
            Ok(_) => Ok(data.len()),
            Err(_) => Err(ErrorKind::WriteError),
        }
    }
}

//mod external;
fn main() {
    // Can be opened with audacity as raw file, signed 16 bit PCM, 44100 Hz, stereo
    let mut file_consumer = FileConsumer::new("output.pcm").unwrap();
    let spec = Spec {
        format: Format::S16NE,
        channels: CHANNELS as u8,
        rate: SAMPLE_RATE,
    };
    // https://www.freedesktop.org/software/pulseaudio/doxygen/structpa__buffer__attr.html#abef20d3a6cab53f716846125353e56a4
    let record_attr = BufferAttr {
        maxlength: u32::MAX, // maximum length of the buffer
        tlength: u32::MAX,   // playback-only: target length of the buffer
        prebuf: u32::MAX,    // playback-only: prebuffering size
        minreq: u32::MAX,    // minimum request size
        fragsize: BUF_SIZE,  // record-only: fragment size
    };
    let playback_attr = BufferAttr {
        maxlength: u32::MAX,   // maximum length of the buffer
        tlength: BUF_SIZE * 3, // playback-only: target length of the buffer
        prebuf: BUF_SIZE * 2,  // playback-only: prebuffering size
        minreq: BUF_SIZE,      // minimum request size
        fragsize: u32::MAX,    // record-only: fragment size
    };
    assert!(spec.is_valid());

    let rec = Simple::new(
        None,                 // Use the default server
        "Rustaudio Recorder", // Our application’s name
        Direction::Record,    // We want a recording stream
        None,                 // Use the default device
        "Record",             // Description of our stream
        &spec,                // Our sample format
        None,                 // Use default channel map
        Some(&record_attr),   // Use default buffering attributes
    )
    .unwrap();
    let out = Simple::new(
        None,
        "Rustaudio Player",
        Direction::Playback,
        None,
        "Play",
        &spec,
        None,
        Some(&playback_attr),
    )
    .unwrap();
    let mut audio_consumer = AudioConsumer { endpoint: out };
    let consumers: &mut [&mut dyn Consumer] = &mut [&mut audio_consumer];

    let mut data = vec![0u8; BUF_SIZE as usize];
    let mut encoded_data = [0u8; BUF_SIZE as usize];
    let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
    let mut encoder = opus_encoder();
    let mut decoder = opus_decoder();
    loop {
        match rec.get_latency() {
            Ok(latency) => {
                println!("Latency: {} ms", latency.as_millis());
            }
            Err(e) => {}
        }
        match rec.read(&mut data) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading from stream: {}", e);
                break;
            }
        }

        let pcm: &[i16] =
            unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };

        let samples_needed = FRAME_SIZE * CHANNELS;
        let pcm = &pcm[..samples_needed];
        let n = encoder.encode(&pcm, &mut encoded_data).unwrap();
        let b = decoder
            .decode(&encoded_data[..n], &mut decoded_data, false)
            .unwrap();

        println!(
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
                    eprintln!("Error consuming data: {:?}", e);
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
