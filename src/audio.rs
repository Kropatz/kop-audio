use std::{
    slice,
    sync::mpsc::{Receiver, Sender},
    thread::sleep,
    time::{Duration, SystemTime},
};

use log::{debug, error};
use opus::{Channels, Decoder, Encoder};

use crate::{
    AudioProducer, BUF_SIZE, CHANNELS, Consumer, FRAME_SIZE, SAMPLE_RATE,
    client::ClientMessage,
    implementations::pulseaudio::{PulseAudioConsumer, PulseAudioProducer}, server::AudioData,
};
use opus::Application::Voip;

pub fn record_audio(
    tx: Sender<ClientMessage>,
    producer: &mut PulseAudioProducer,
    rx: Receiver<ClientMessage>,
) {
    let mut data = vec![0u8; BUF_SIZE as usize];
    let mut encoded_data = [0u8; BUF_SIZE as usize];
    let mut encoder = opus_encoder();
    let mut hangover = 0;
    let mut muted = false;
    let hangover_limit = 10;
    let mut sequence_number: u32 = 0;
    let mut timestamp: u64 = 0;
    loop {
        match rx.try_recv() {
            Ok(ClientMessage::ToggleMute) => {
                debug!("Got toggle mute in record_audio");
                muted = !muted;
            }
            _ => {}
        }
        match producer.produce(&mut data) {
            Ok(_) => {}
            Err(_) => {
                error!("Error reading from stream");
                break;
            }
        }
        if muted {
            sleep(Duration::from_millis(20));
            continue;
        }
        let pcm: &[i16] =
            unsafe { slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2) };

        let samples_needed = FRAME_SIZE * CHANNELS;
        let pcm = &pcm[..samples_needed];
        if is_silence(pcm, 200.0) {
            if hangover == 0 {
                let _ = tx.send(ClientMessage::TransmitAudio(false));
                continue;
            }
            hangover -= 1;
        } else {
            hangover = hangover_limit;
        }
        debug!("Acive audio detected, sending packet");
        let n = encoder.encode(&pcm, &mut encoded_data).unwrap();

        debug!(
            "Read {} samples, data has {} samples, encoded to {} bytes,",
            pcm.len(),
            data.len() / 2,
            n,
        );
        timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
        sequence_number = sequence_number.wrapping_add(1);
        let _ = tx.send(ClientMessage::TransmitAudio(true));
        let _ = tx.send(ClientMessage::Audio(AudioData {
            timestamp,
            seq_number: sequence_number,
            data: encoded_data[..n].to_vec(),
        }));
    }
}

pub fn play_audio(rx: Receiver<ClientMessage>, consumer: &mut PulseAudioConsumer) {
    let mut decoder = opus_decoder();
    let mut decoded_data = vec![0i16; FRAME_SIZE * CHANNELS];
    let mut deafened = false;
    for msg in rx.iter() {
        match msg {
            ClientMessage::RecvAudio(_, audio) => {
                if deafened {
                    sleep(Duration::from_millis(20));
                    continue;
                }
                let b = decoder.decode(&audio.data, &mut decoded_data, false).unwrap();
                match consumer.consume(unsafe {
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
            ClientMessage::ToggleDeafen => {
                deafened = !deafened;
            }
            _ => {}
        }
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
