use crate::server::AudioData;

struct JitterBuffer {
    buffer: Vec<AudioData>,
    max_size: usize,
}
