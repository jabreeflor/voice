//! Microphone capture. Reference: `Recorder` in Sources/VoiceCore/core.swift.
//!
//! Opens the default input device with cpal, converts whatever format the
//! device delivers to mono f32 at 16 kHz (the rate whisper.cpp expects) and
//! accumulates samples until `stop()`.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

/// Same wording as the Swift app; `app.rs` prefixes it with "Mic error: ".
const NO_INPUT: &str = "No microphone input (check mic permission).";

/// Input frames handed to the resampler per call. Device buffers are not a
/// multiple of this, so `RateConverter` buffers the remainder between calls.
const RESAMPLE_CHUNK: usize = 1024;

/// 16 kHz output frames per level update. Swift taps the input at 4096
/// frames per buffer (~85 ms at 48 kHz) and applies the 0.82 decay once per
/// buffer; cpal's default buffer is 480-1024 frames and varies by backend, so
/// the decay is tied to this fixed amount of audio instead of the device.
const LEVEL_BLOCK: usize = 4096 * Recorder::SAMPLE_RATE as usize / 48_000;

/// State shared between the cpal callback thread and the app thread.
struct Shared {
    samples: Mutex<Vec<f32>>,
    /// Owned by the callback while the stream runs; `stop()` takes it back to
    /// flush the resampler's tail once the stream has been dropped.
    pipeline: Mutex<Pipeline>,
    /// f32 bits; an atomic so `level()` never blocks the audio thread.
    level: AtomicU32,
}

impl Shared {
    fn new(channels: usize, converter: Option<RateConverter>) -> Shared {
        Shared {
            samples: Mutex::new(Vec::new()),
            pipeline: Mutex::new(Pipeline {
                channels,
                converter,
                mono: Vec::new(),
                scratch: Vec::new(),
                level_sum: 0.0,
                level_count: 0,
            }),
            level: AtomicU32::new(0.0f32.to_bits()),
        }
    }

    fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    fn set_level(&self, v: f32) {
        self.level.store(v.to_bits(), Ordering::Relaxed);
    }

    /// One device buffer: interleaved frames → mono → 16 kHz → level + append.
    fn ingest<T>(&self, interleaved: &[T])
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let Ok(mut pipeline) = self.pipeline.lock() else {
            return;
        };
        // Both buffers are reused across callbacks: no allocation on the
        // audio thread once they have grown to the device buffer size.
        let Pipeline {
            channels,
            converter,
            mono,
            scratch: out,
            level_sum,
            level_count,
        } = &mut *pipeline;
        mix_to_mono(interleaved, *channels, mono);
        out.clear();
        match converter.as_mut() {
            Some(conv) => conv.push(mono, out),
            None => out.extend_from_slice(mono),
        }
        if out.is_empty() {
            return;
        }
        for &s in out.iter() {
            *level_sum += s * s;
            *level_count += 1;
            if *level_count == LEVEL_BLOCK {
                let rms = (*level_sum / LEVEL_BLOCK as f32).sqrt();
                self.set_level(next_level(rms, self.level()));
                *level_sum = 0.0;
                *level_count = 0;
            }
        }
        if let Ok(mut samples) = self.samples.lock() {
            samples.extend_from_slice(out);
        }
    }
}

struct Pipeline {
    channels: usize,
    converter: Option<RateConverter>,
    /// Reusable mono mix of the current device buffer.
    mono: Vec<f32>,
    /// Reusable 16 kHz output of the current device buffer.
    scratch: Vec<f32>,
    /// Sum of squares and count of the partial `LEVEL_BLOCK` in progress.
    level_sum: f32,
    level_count: usize,
}

pub struct Recorder {
    stream: Option<cpal::Stream>,
    shared: Option<Arc<Shared>>,
    started: Option<Instant>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub const SAMPLE_RATE: u32 = 16_000;

    pub fn new() -> Recorder {
        Recorder {
            stream: None,
            shared: None,
            started: None,
        }
    }

    /// Starts capturing. Errors carry a user-facing message ("No microphone
    /// input (check mic permission)." when no device/format is available).
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_recording() {
            return Ok(());
        }
        // cpal cannot see a TCC denial on macOS: the device opens and the
        // callback just receives silence, where AVAudioEngine's degenerate
        // input format made Swift throw. Ask the OS explicitly so the user
        // gets the permission hint instead of an empty transcript. Windows
        // reports denial from WASAPI itself; Linux returns NotApplicable.
        if crate::platform::mic_status() == crate::platform::MicStatus::Denied {
            return Err(NO_INPUT.to_string());
        }
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(NO_INPUT)?;
        let supported = device.default_input_config().map_err(|e| {
            log::warn!("default input config unavailable: {e}");
            NO_INPUT.to_string()
        })?;
        let channels = supported.channels() as usize;
        let rate = supported.sample_rate();
        if channels == 0 || rate == 0 {
            return Err(NO_INPUT.to_string());
        }
        let converter = if rate == Self::SAMPLE_RATE {
            None
        } else {
            Some(RateConverter::new(rate).map_err(|e| format!("Resampler setup failed: {e}"))?)
        };
        let shared = Arc::new(Shared::new(channels, converter));
        let config = supported.config();

        // Every PCM format cpal can deliver converts to f32 through
        // `FromSample`; ALSA `hw:` devices and WASAPI mix formats commonly
        // land on I24/I32, which AVAudioConverter accepted transparently.
        // Only the DSD formats (not PCM) are left out.
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, &shared),
            SampleFormat::F64 => build_stream::<f64>(&device, &config, &shared),
            SampleFormat::I8 => build_stream::<i8>(&device, &config, &shared),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, &shared),
            SampleFormat::I24 => build_stream::<cpal::I24>(&device, &config, &shared),
            SampleFormat::I32 => build_stream::<i32>(&device, &config, &shared),
            SampleFormat::I64 => build_stream::<i64>(&device, &config, &shared),
            SampleFormat::U8 => build_stream::<u8>(&device, &config, &shared),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, &shared),
            SampleFormat::U24 => build_stream::<cpal::U24>(&device, &config, &shared),
            SampleFormat::U32 => build_stream::<u32>(&device, &config, &shared),
            SampleFormat::U64 => build_stream::<u64>(&device, &config, &shared),
            other => Err(format!("Unsupported microphone sample format {other:?}.")),
        }?;
        stream.play().map_err(|e| e.to_string())?;

        self.stream = Some(stream);
        self.shared = Some(shared);
        self.started = Some(Instant::now());
        Ok(())
    }

    /// Stops and returns the 16 kHz mono samples (empty if not recording).
    pub fn stop(&mut self) -> Vec<f32> {
        // Dropping the stream stops callbacks before we take the pipeline, so
        // the flush below sees every buffer the device delivered.
        // `started` is deliberately left set (as Swift leaves `startTime`)
        // so `duration()` is still valid after `stop()`; the caller reads the
        // samples and the duration in either order. `start()` overwrites it.
        self.stream = None;
        let Some(shared) = self.shared.take() else {
            return Vec::new();
        };
        // A poisoned lock (a callback panicked mid-buffer) still holds every
        // sample captured so far; recover it rather than drop the recording,
        // as `paste.rs` does with its clipboard handle.
        let mut result = {
            let mut samples = shared.samples.lock().unwrap_or_else(|p| p.into_inner());
            std::mem::take(&mut *samples)
        };
        let mut pipeline = shared.pipeline.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(conv) = pipeline.converter.as_mut() {
            conv.flush(&mut result);
        }
        result
    }

    pub fn cancel(&mut self) {
        let _ = self.stop();
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// Smoothed input level 0..1: fast attack, slow decay
    /// (`max(min(rms * 9, 1), level * 0.82)` per `LEVEL_BLOCK` of audio).
    pub fn level(&self) -> f32 {
        self.shared.as_ref().map(|s| s.level()).unwrap_or(0.0)
    }

    /// Time since the most recent `start()`. Stays valid after `stop()` so
    /// the 0.35 s short-tap guard can run on it; zero before the first start.
    pub fn duration(&self) -> Duration {
        self.started.map(|t| t.elapsed()).unwrap_or_default()
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let sink = Arc::clone(shared);
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data, _| sink.ingest(data),
            |e| log::warn!("input stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

// MARK: pure pieces (unit-tested; no device involved)

/// Averages every frame's channels into one f32 sample, replacing `out`.
fn mix_to_mono<T>(interleaved: &[T], channels: usize, out: &mut Vec<f32>)
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    out.clear();
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / channels as f32),
    );
}

/// Fast attack, slow decay — identical to the Swift `_level` update.
fn next_level(rms: f32, previous: f32) -> f32 {
    (rms * 9.0).min(1.0).max(previous * 0.82)
}

/// Streaming device-rate → 16 kHz conversion. `SincFixedIn` needs exactly
/// `RESAMPLE_CHUNK` input frames per call, so partial buffers wait in
/// `pending` until enough arrive; `flush` zero-pads the tail at the end and
/// drains the filter's group delay so the last word is not clipped.
struct RateConverter {
    resampler: SincFixedIn<f32>,
    /// Output frames per input frame (16 000 / device rate).
    ratio: f64,
    pending: Vec<f32>,
    out: Vec<Vec<f32>>,
    /// Frames consumed from `pending` and frames produced so far. The sinc
    /// filter is centred, so the output lags the input by ~sinc_len/2
    /// input frames (rubato drops that lead-in from the first chunk); the
    /// difference between `in_frames * ratio` and `out_frames` is what is
    /// still inside the filter at the end.
    in_frames: usize,
    out_frames: usize,
    /// `flush` is once per recording; a second call must not emit more.
    flushed: bool,
}

impl RateConverter {
    fn new(from_rate: u32) -> Result<RateConverter, rubato::ResamplerConstructionError> {
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };
        let ratio = f64::from(Recorder::SAMPLE_RATE) / f64::from(from_rate);
        let resampler = SincFixedIn::<f32>::new(ratio, 1.0, params, RESAMPLE_CHUNK, 1)?;
        let out = resampler.output_buffer_allocate(true);
        Ok(RateConverter {
            resampler,
            ratio,
            pending: Vec::with_capacity(RESAMPLE_CHUNK * 2),
            out,
            in_frames: 0,
            out_frames: 0,
            flushed: false,
        })
    }

    fn push(&mut self, mono: &[f32], into: &mut Vec<f32>) {
        self.pending.extend_from_slice(mono);
        let chunk = self.resampler.input_frames_next();
        let mut offset = 0;
        while self.pending.len() - offset >= chunk {
            let input = [&self.pending[offset..offset + chunk]];
            match self
                .resampler
                .process_into_buffer(&input, &mut self.out, None)
            {
                Ok((_, written)) => {
                    into.extend_from_slice(&self.out[0][..written]);
                    self.out_frames += written;
                }
                Err(e) => log::warn!("resample failed: {e}"),
            }
            self.in_frames += chunk;
            offset += chunk;
        }
        self.pending.drain(..offset);
    }

    /// Pushes out the buffered remainder (zero-padded) and the frames still
    /// held back by the filter delay (~22 at 48 kHz, ~132 at 8 kHz). Runs
    /// even when nothing is pending: the delayed frames only come out once
    /// zeros are pushed through, and skipping them clips the last word.
    fn flush(&mut self, into: &mut Vec<f32>) {
        if self.flushed {
            return;
        }
        self.flushed = true;
        let tail = std::mem::take(&mut self.pending);
        let total = ((self.in_frames + tail.len()) as f64 * self.ratio).round() as usize;
        let mut needed = total.saturating_sub(self.out_frames);
        // The first call carries the real tail; later ones feed zeros only.
        // Each partial call is padded to a full chunk, so one extra call is
        // enough when tail + delay exceed the chunk. An empty tail must go in
        // as `None`, not `Some(&[&[]])`: rubato (0.16) marks every channel
        // active regardless of length, clears its padded input for a
        // zero-length channel, and then fails validation with
        // `InsufficientInputBufferSize { size: 0, .. }`. `None` leaves the
        // padded buffer full of zeros, which is exactly the flush we want.
        let mut tail = (!tail.is_empty()).then_some(tail);
        while needed > 0 {
            let result = match tail.take() {
                Some(tail) => {
                    let input = [tail.as_slice()];
                    self.resampler
                        .process_partial_into_buffer(Some(&input), &mut self.out, None)
                }
                None => self.resampler.process_partial_into_buffer(
                    None::<&[&[f32]]>,
                    &mut self.out,
                    None,
                ),
            };
            match result {
                Ok((_, 0)) => break,
                Ok((_, written)) => {
                    // Only the part that corresponds to real input; the rest
                    // is zero padding rendered through the filter.
                    let keep = written.min(needed);
                    into.extend_from_slice(&self.out[0][..keep]);
                    self.out_frames += keep;
                    needed -= keep;
                }
                Err(e) => {
                    log::warn!("resample flush failed: {e}");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn mono<T>(interleaved: &[T], channels: usize) -> Vec<f32>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        // Pre-filled so the test also pins that `out` is replaced, not appended.
        let mut out = vec![9.0; 3];
        mix_to_mono(interleaved, channels, &mut out);
        out
    }

    #[test]
    fn mono_mix_averages_channels() {
        let stereo = [0.5f32, -0.5, 1.0, 0.0, 0.25, 0.75];
        assert_eq!(mono(&stereo, 2), vec![0.0, 0.5, 0.5]);
        // Mono passes through unchanged; a trailing partial frame is dropped.
        assert_eq!(mono(&[0.1f32, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
        assert_eq!(mono(&[1.0f32, 1.0, 1.0], 2), vec![1.0]);
    }

    #[test]
    fn mono_mix_converts_integer_formats() {
        // cpal integer samples scale to [-1, 1] before mixing.
        let i16s = [i16::MAX, 0, i16::MIN, i16::MIN];
        let mixed = mono(&i16s, 2);
        assert!((mixed[0] - 0.5).abs() < 1e-4, "{mixed:?}");
        assert!((mixed[1] + 1.0).abs() < 1e-4, "{mixed:?}");
        let u16s = [u16::MAX, 32768u16];
        let mixed = mono(&u16s, 1);
        assert!(
            (mixed[0] - 1.0).abs() < 1e-3 && mixed[1].abs() < 1e-4,
            "{mixed:?}"
        );
        // 24-bit devices (ALSA hw:, WASAPI mix formats) are dispatched too.
        let i24s = [
            cpal::I24::new(0).unwrap(),
            cpal::I24::new((1 << 23) - 1).unwrap(),
        ];
        let mixed = mono(&i24s, 1);
        assert!(
            mixed[0].abs() < 1e-6 && (mixed[1] - 1.0).abs() < 1e-3,
            "{mixed:?}"
        );
    }

    #[test]
    fn level_block_is_swift_buffer_at_16k() {
        // 4096 frames at 48 kHz (Swift's tap buffer, ~85 ms) resampled to 16 kHz.
        assert_eq!(LEVEL_BLOCK, 1365);
    }

    #[test]
    fn level_smoothing_matches_swift_formula() {
        // Attack: a loud buffer jumps straight to the clamped value.
        assert_eq!(next_level(0.5, 0.0), 1.0);
        assert!((next_level(0.05, 0.0) - 0.45).abs() < 1e-6);
        // Decay: silence lets the level fall by 18 % per buffer, never below rms*9.
        assert!((next_level(0.0, 1.0) - 0.82).abs() < 1e-6);
        assert!((next_level(0.02, 0.5) - 0.41).abs() < 1e-6);
        assert!((next_level(0.1, 0.5) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn level_updates_once_per_block_across_device_buffers() {
        // The decay must be tied to LEVEL_BLOCK frames of audio, not to the
        // device buffer size, so a 128-frame backend (small ALSA/CoreAudio
        // periods) decays at the same rate as Swift's 4096-frame tap.
        let shared = Shared::new(1, None);
        let square: Vec<f32> = (0..128)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        for _ in 0..10 {
            shared.ingest(&square);
        }
        // 1280 frames: still inside the first block, nothing published yet.
        assert_eq!(shared.level(), 0.0);
        shared.ingest(&square);
        // 1408 frames: the block completed at 1365 with rms 0.5 → clamped to 1.0.
        assert_eq!(shared.level(), 1.0);

        // Silence: exactly one 0.82 decay per completed block, regardless of
        // how many 128-frame buffers it took to fill it.
        let silence = vec![0.0f32; 128];
        let mut steps = Vec::new();
        let mut last = shared.level();
        while steps.len() < 3 {
            shared.ingest(&silence);
            let now = shared.level();
            if now != last {
                steps.push(now);
                last = now;
            }
        }
        for (got, want) in steps.iter().zip([0.82f32, 0.6724, 0.551368]) {
            assert!((got - want).abs() < 1e-5, "steps {steps:?}");
        }
    }

    #[test]
    fn resample_48k_to_16k_keeps_duration_and_amplitude() {
        // One second of a 440 Hz tone at 48 kHz delivered in device-sized
        // buffers (not multiples of the resampler chunk) must come out as
        // ~16 000 samples with the same amplitude.
        let from = 48_000u32;
        let input: Vec<f32> = (0..from)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / from as f32).sin() * 0.5)
            .collect();
        let mut conv = RateConverter::new(from).expect("resampler");
        let mut out = Vec::new();
        for buffer in input.chunks(480) {
            conv.push(buffer, &mut out);
        }
        conv.flush(&mut out);

        // The flush drains the filter delay, so the count is exact to
        // within rounding of the partial last chunk.
        let expected = Recorder::SAMPLE_RATE as usize;
        let diff = out.len().abs_diff(expected);
        assert!(diff <= 2, "got {} samples, expected ~{expected}", out.len());

        // Skip the filter transient at both ends before measuring.
        let mid = &out[1000..out.len() - 1000];
        let amp = rms(mid) * std::f32::consts::SQRT_2;
        assert!((amp - 0.5).abs() < 0.02, "amplitude {amp}");
    }

    #[test]
    fn resample_44k_flush_without_full_chunk() {
        // Fewer frames than one chunk must still produce output on flush.
        let mut conv = RateConverter::new(44_100).expect("resampler");
        let mut out = Vec::new();
        conv.push(&vec![0.25f32; 441], &mut out);
        assert!(out.is_empty());
        conv.flush(&mut out);
        assert!(out.len().abs_diff(160) <= 2, "got {}", out.len());
        // Flushing again is a no-op.
        conv.flush(&mut out);
        assert!(out.len().abs_diff(160) <= 2);
    }

    #[test]
    fn flush_drains_filter_delay_at_low_rates() {
        // At 8 kHz the sinc group delay is ~128 output frames; a recording
        // whose length is an exact multiple of the chunk (nothing pending)
        // used to lose all of them. Half a second of DC must come back whole
        // and at full amplitude right up to the end.
        let from = 8_000u32;
        let mut conv = RateConverter::new(from).expect("resampler");
        let mut out = Vec::new();
        conv.push(&vec![0.5f32; RESAMPLE_CHUNK * 4], &mut out);
        let before_flush = out.len();
        conv.flush(&mut out);
        let expected = RESAMPLE_CHUNK * 4 * 2;
        assert!(out.len() > before_flush, "flush emitted nothing");
        assert!(
            out.len().abs_diff(expected) <= 2,
            "got {} samples, expected ~{expected}",
            out.len()
        );
        let last = out[out.len() - 1];
        assert!((last - 0.5).abs() < 0.05, "tail sample {last}");
    }

    #[test]
    fn flush_also_covers_tail_longer_than_chunk_minus_delay() {
        // tail + delay exceeds one padded chunk, so flush needs a second
        // zero-only call to drain everything.
        let from = 48_000u32;
        let mut conv = RateConverter::new(from).expect("resampler");
        let mut out = Vec::new();
        let frames = RESAMPLE_CHUNK * 2 + RESAMPLE_CHUNK - 8;
        conv.push(&vec![0.5f32; frames], &mut out);
        conv.flush(&mut out);
        let expected = frames / 3;
        assert!(
            out.len().abs_diff(expected) <= 2,
            "got {} samples, expected ~{expected}",
            out.len()
        );
    }
}
