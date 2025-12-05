use opus::{Application, Encoder as OpusEncoder};
use rubato::{FftFixedInOut, Resampler};
use symphonia::{core::{audio::{AudioBufferRef, SampleBuffer, Signal}, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint}, default::{get_codecs, get_probe}};
use std::fs::File;

use crate::{CHANNELS, FRAME_SIZE, SAMPLE_RATE};

pub fn decode_mp3(path: &str) -> Vec<f32> {
    let file = File::open(path).unwrap();
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .unwrap();

    let mut format = probed.format;

    // Prepare decoder
    let track = format.default_track().expect("No default track");
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap();

    let mut output = Vec::new();
    let track_id = track.id;


    let mut sample_count = 0;
    let mut sample_buf = None;
    loop {
        // Get the next packet from the format reader.
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break, // Finished
        };

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples, ignoring any decode errors.
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                // The decoded audio samples may now be accessed via the audio buffer if per-channel
                // slices of samples in their native decoded format is desired. Use-cases where
                // the samples need to be accessed in an interleaved order or converted into
                // another sample format, or a byte buffer is required, are covered by copying the
                // audio buffer into a sample buffer or raw sample buffer, respectively. In the
                // example below, we will copy the audio buffer into a sample buffer in an
                // interleaved order while also converting to a f32 sample format.

                // If this is the *first* decoded packet, create a sample buffer matching the
                // decoded audio buffer format.
                if sample_buf.is_none() {
                    // Get the audio buffer specification.
                    let spec = *audio_buf.spec();

                    // Get the capacity of the decoded buffer. Note: This is capacity, not length!
                    let duration = audio_buf.capacity() as u64;

                    // Create the f32 sample buffer.
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                // Copy the decoded audio buffer into the sample buffer in an interleaved format.
                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_buf);

                    // The samples may now be access via the `samples()` function.
                    sample_count += buf.samples().len();
                    print!("\rDecoded {} samples", sample_count);
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => (),
            Err(_) => break,
        }
    }

    // Decode all packets
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break, // Finished
        };

        let decoded = decoder.decode(&packet).unwrap();

        if let AudioBufferRef::F32(buf) = decoded {
            let chans = buf.spec().channels.count();
            assert_eq!(chans, CHANNELS);

            // Convert planar → interleaved f32
            for i in 0..buf.frames() {
                for ch in 0..chans {
                    output.push(buf.chan(ch)[i]);
                }
            }
        } else {
            panic!("Expected f32 audio");
        }
    }

    output
}

pub fn resample_to_48k(input: &[f32], input_rate: usize) -> Vec<f32> {
    // Split interleaved → planar
    let mut left = Vec::new();
    let mut right = Vec::new();

    for chunk in input.chunks_exact(2) {
        left.push(chunk[0]);
        right.push(chunk[1]);
    }

    let mut resampler = FftFixedInOut::<f32>::new(
        input_rate,
        SAMPLE_RATE as usize,
        FRAME_SIZE,
        2,
    )
    .unwrap();

    let out = resampler.process(&[left, right], None).unwrap();

    // planar → interleaved
    let mut interleaved = Vec::new();
    for i in 0..out[0].len() {
        interleaved.push(out[0][i]);
        interleaved.push(out[1][i]);
    }

    interleaved
}
