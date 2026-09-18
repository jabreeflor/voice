//! RIFF/WAV container for the 16 kHz mono float capture. Reference:
//! `Recorder.wavData` in Sources/VoiceCore/core.swift. A wrong header byte means
//! silent mis-transcription rather than a crash, so every field is pinned by tests.

pub const SAMPLE_RATE: u32 = 16_000;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const HEADER_LEN: usize = 44;

/// 44-byte header + 16-bit little-endian PCM. Samples are clamped to [-1, 1]
/// and scaled by 32767 with truncation toward zero (matches the Swift cast).
pub fn wav_data(samples: &[f32]) -> Vec<u8> {
    let data_size = (samples.len() * 2) as u32;
    let mut d = Vec::with_capacity(HEADER_LEN + data_size as usize);
    let block_align = CHANNELS * BITS_PER_SAMPLE / 8;
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + data_size).to_le_bytes());
    d.extend_from_slice(b"WAVE");
    d.extend_from_slice(b"fmt ");
    d.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    d.extend_from_slice(&1u16.to_le_bytes()); // PCM
    d.extend_from_slice(&CHANNELS.to_le_bytes());
    d.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    d.extend_from_slice(&(SAMPLE_RATE * block_align as u32).to_le_bytes()); // byte rate
    d.extend_from_slice(&block_align.to_le_bytes());
    d.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        d.extend_from_slice(&v.to_le_bytes());
    }
    d
}
