use crate::{AudioProducer, BUF_SIZE, CHANNELS, Consumer, SAMPLE_RATE};

use crate::ErrorKind;
use crate::psimple::Simple;
use crate::pulse::def::BufferAttr;
use crate::pulse::sample::{Format, Spec};
use crate::pulse::stream::Direction;

pub struct PulseAudioProducer {
    endpoint: Simple,
}

impl PulseAudioProducer {
    pub fn new() -> Result<Self, ErrorKind> {
        let spec = Spec {
            format: Format::S16NE,
            channels: CHANNELS as u8,
            rate: SAMPLE_RATE,
        };
        let record_attr = BufferAttr {
            maxlength: u32::MAX, // maximum length of the buffer
            tlength: u32::MAX,   // playback-only: target length of the buffer
            prebuf: u32::MAX,    // playback-only: prebuffering size
            minreq: u32::MAX,    // minimum request size
            fragsize: BUF_SIZE,  // record-only: fragment size
        };

        let rec = Simple::new(
            None,                 // Use the default server
            "Rustaudio Recorder", // Our application’s name
            Direction::Record,    // We want a recording stream
            None,                 // Use the default device
            "Record",             // Description of our stream
            &spec,                // Our sample format
            None,                 // Use default channel map
            Some(&record_attr),   // Use default buffering attributes
        );
        match rec {
            Ok(endpoint) => Ok(PulseAudioProducer { endpoint }),
            Err(_) => Err(ErrorKind::InitializationError),
        }
    }
}

impl AudioProducer for PulseAudioProducer {
    fn produce(&mut self, data: &mut [u8]) -> Result<(), ErrorKind> {
        match self.endpoint.read(data) {
            Ok(_) => Ok(()),
            Err(_) => Err(ErrorKind::ReadError),
        }
    }
}

pub struct PulseAudioConsumer {
    endpoint: Simple,
}

impl PulseAudioConsumer {
    pub fn new() -> Result<Self, ErrorKind> {
        let spec = Spec {
            format: Format::S16NE,
            channels: CHANNELS as u8,
            rate: SAMPLE_RATE,
        };
        let playback_attr = BufferAttr {
            maxlength: u32::MAX,   // maximum length of the buffer
            tlength: BUF_SIZE * 3, // playback-only: target length of the buffer
            prebuf: BUF_SIZE * 2,  // playback-only: prebuffering size
            minreq: BUF_SIZE,      // minimum request size
            fragsize: u32::MAX,    // record-only: fragment size
        };

        let out = Simple::new(
            None,
            "Rustaudio Player",
            Direction::Playback,
            None,
            "Play",
            &spec,
            None,
            Some(&playback_attr),
        );
        match out {
            Ok(endpoint) => Ok(PulseAudioConsumer { endpoint }),
            Err(_) => Err(ErrorKind::InitializationError),
        }
    }
}

impl Consumer for PulseAudioConsumer {
    fn consume(&mut self, data: &[u8]) -> Result<usize, ErrorKind> {
        match self.endpoint.write(data) {
            Ok(_) => Ok(data.len()),
            Err(e) => Err(ErrorKind::WriteError(format!("{:?}", e))),
        }
    }
}
