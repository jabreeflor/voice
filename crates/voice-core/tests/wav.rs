//! `wav_data` hand-builds the RIFF container that whisper.cpp is fed. A wrong
//! byte in the 44-byte header means silent mis-transcription rather than a
//! crash, so every field is pinned here.
//! Port of Tests/VoiceCoreTests/WavTests.swift.

use voice_core::wav_data;

// Helpers

fn ascii(d: &[u8], range: std::ops::Range<usize>) -> &str {
    std::str::from_utf8(&d[range]).expect("header identifiers are ASCII")
}

fn u32_at(d: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([d[offset], d[offset + 1], d[offset + 2], d[offset + 3]])
}

fn u16_at(d: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([d[offset], d[offset + 1]])
}

fn i16_at(d: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([d[offset], d[offset + 1]])
}

const COUNTS: [usize; 5] = [0, 1, 7, 1000, 16000];

// Chunk identifiers

#[test]
fn riff_wave_chunk_identifiers() {
    let d = wav_data(&[0.0, 0.0, 0.0, 0.0]);
    assert_eq!(ascii(&d, 0..4), "RIFF");
    assert_eq!(ascii(&d, 8..12), "WAVE");
    assert_eq!(ascii(&d, 12..16), "fmt ");
    assert_eq!(ascii(&d, 36..40), "data");
}

// fmt chunk

#[test]
fn format_chunk_describes_16khz_mono_16bit_pcm() {
    let d = wav_data(&[0.0]);
    assert_eq!(u32_at(&d, 16), 16, "fmt chunk size should be 16 (PCM)");
    assert_eq!(
        u16_at(&d, 20),
        1,
        "audio format should be 1 (uncompressed PCM)"
    );
    assert_eq!(u16_at(&d, 22), 1, "should be mono");
    assert_eq!(u32_at(&d, 24), 16000, "sample rate should be 16 kHz");
    assert_eq!(
        u32_at(&d, 28),
        32000,
        "byte rate = 16000 * 1 channel * 2 bytes"
    );
    assert_eq!(u16_at(&d, 32), 2, "block align = 1 channel * 2 bytes");
    assert_eq!(u16_at(&d, 34), 16, "should be 16-bit samples");
}

/// Header fields are fixed, not derived from the sample array, so they must
/// hold for any length.
#[test]
fn format_chunk_is_identical_regardless_of_sample_count() {
    let short = wav_data(&[0.1; 3]);
    let long = wav_data(&vec![0.1; 16000]);
    assert_eq!(short[12..36], long[12..36]);
}

// Sizes

#[test]
fn data_size_is_two_bytes_per_sample() {
    for count in COUNTS {
        let d = wav_data(&vec![0.0; count]);
        assert_eq!(
            u32_at(&d, 40),
            (count * 2) as u32,
            "data size wrong for {count} samples"
        );
    }
}

#[test]
fn riff_size_is_data_size_plus_36() {
    for count in COUNTS {
        let d = wav_data(&vec![0.0; count]);
        assert_eq!(
            u32_at(&d, 4),
            (36 + count * 2) as u32,
            "RIFF size wrong for {count} samples"
        );
    }
}

#[test]
fn total_length_is_header_plus_payload() {
    for count in COUNTS {
        let d = wav_data(&vec![0.0; count]);
        assert_eq!(
            d.len(),
            44 + count * 2,
            "total length wrong for {count} samples"
        );
    }
}

/// The declared sizes must agree with the bytes actually present, or the
/// decoder reads past the end of the buffer.
#[test]
fn declared_sizes_agree_with_actual_byte_count() {
    let d = wav_data(&vec![0.25; 512]);
    assert_eq!(u32_at(&d, 40) as usize, d.len() - 44);
    assert_eq!(u32_at(&d, 4) as usize, d.len() - 8);
}

// Empty input

#[test]
fn empty_input_produces_header_only_file() {
    let d = wav_data(&[]);
    assert_eq!(d.len(), 44);
    assert_eq!(ascii(&d, 0..4), "RIFF");
    assert_eq!(ascii(&d, 8..12), "WAVE");
    assert_eq!(u32_at(&d, 4), 36);
    assert_eq!(u32_at(&d, 40), 0);
}

// Sample conversion

#[test]
fn silence_encodes_as_zero_samples() {
    let d = wav_data(&[0.0, 0.0, 0.0]);
    for i in 0..3 {
        assert_eq!(i16_at(&d, 44 + i * 2), 0);
    }
}

#[test]
fn full_scale_samples_map_to_int16_extremes() {
    let d = wav_data(&[1.0, -1.0]);
    assert_eq!(i16_at(&d, 44), 32767);
    assert_eq!(i16_at(&d, 46), -32767);
}

/// Out-of-range floats are clamped rather than wrapping around, which would
/// turn a loud passage into audible garbage.
#[test]
fn out_of_range_samples_are_clamped_not_wrapped() {
    let d = wav_data(&[4.5, -4.5, 100.0, -100.0]);
    assert_eq!(i16_at(&d, 44), 32767);
    assert_eq!(i16_at(&d, 46), -32767);
    assert_eq!(i16_at(&d, 48), 32767);
    assert_eq!(i16_at(&d, 50), -32767);
}

/// A NaN sample clamps to NaN, and Rust's `as i16` saturating cast turns that
/// into 0 (silence); ±infinity clamp to ±1 like any other out-of-range value.
/// This is a deliberate divergence from Swift, where `Int16(NaN)` traps: a bad
/// audio buffer must never take the app down mid-dictation.
#[test]
fn non_finite_samples_do_not_panic() {
    let d = wav_data(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
    assert_eq!(i16_at(&d, 44), 0);
    assert_eq!(i16_at(&d, 46), 32767);
    assert_eq!(i16_at(&d, 48), -32767);
}

/// Swift's `Int16(x * 32767)` truncates toward zero, so ±0.5 must land on
/// ±16383, never round away to ±16384.
#[test]
fn mid_scale_sample_is_scaled_by_32767() {
    let d = wav_data(&[0.5, -0.5]);
    assert_eq!(i16_at(&d, 44), (0.5f32 * 32767.0) as i16);
    assert_eq!(i16_at(&d, 46), (-0.5f32 * 32767.0) as i16);
    assert_eq!(i16_at(&d, 44), 16383);
    assert_eq!(i16_at(&d, 46), -16383);
}

#[test]
fn samples_are_written_in_order_little_endian() {
    let d = wav_data(&[0.0, 1.0, 0.0, -1.0]);
    assert_eq!(i16_at(&d, 44), 0);
    assert_eq!(i16_at(&d, 46), 32767);
    assert_eq!(i16_at(&d, 48), 0);
    assert_eq!(i16_at(&d, 50), -32767);
    // 32767 little-endian is 0xFF 0x7F.
    assert_eq!(d[46], 0xFF);
    assert_eq!(d[47], 0x7F);
}

// Shape of a realistic buffer

/// The engine warm-up pushes half a second of silence through this path.
#[test]
fn half_second_of_silence_has_expected_size() {
    let d = wav_data(&vec![0.0; 8000]);
    assert_eq!(d.len(), 44 + 16000);
    assert_eq!(u32_at(&d, 24), 16000);
    assert_eq!(
        u32_at(&d, 40) as usize / 2,
        8000,
        "8000 frames = 0.5s at 16 kHz"
    );
}
