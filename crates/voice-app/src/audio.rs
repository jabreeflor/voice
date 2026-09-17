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
                scratch: Vec::new(),
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
        let mono = mix_to_mono(interleaved, pipeline.channels);
        let mut out = std::mem::take(&mut pipeline.scratch);
        out.clear();
        match pipeline.converter.as_mut() {
            Some(conv) => conv.push(&mono, &mut out),
            None => out.extend_from_slice(&mono),
        }
        if !out.is_empty() {
            self.set_level(next_level(rms(&out), self.level()));
            if let Ok(mut samples) = self.samples.lock() {
                samples.extend_from_slice(&out);
            }
        }
        pipeline.scratch = out;
    }
}

struct Pipeline {
    channels: usize,
    converter: Option<RateConverter>,
    scratch: Vec<f32>,
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

        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, &shared),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, &shared),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, &shared),
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
        self.stream = None;
        self.started = None;
        let Some(shared) = self.shared.take() else {
            return Vec::new();
        };
        let mut result = shared
            .samples
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        if let Ok(mut pipeline) = shared.pipeline.lock() {
            if let Some(conv) = pipeline.converter.as_mut() {
                conv.flush(&mut result);
            }
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
    /// (`max(min(rms * 9, 1), level * 0.82)` per buffer).
    pub fn level(&self) -> f32 {
        self.shared.as_ref().map(|s| s.level()).unwrap_or(0.0)
    }

    /// Time since `start()` (zero when idle).
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

/// Averages every frame's channels into one f32 sample.
fn mix_to_mono<T>(interleaved: &[T], channels: usize) -> Vec<f32>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / channels as f32)
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Fast attack, slow decay — identical to the Swift `_level` update.
fn next_level(rms: f32, previous: f32) -> f32 {
    (rms * 9.0).min(1.0).max(previous * 0.82)
}

/// Streaming device-rate → 16 kHz conversion. `SincFixedIn` needs exactly
/// `RESAMPLE_CHUNK` input frames per call, so partial buffers wait in
/// `pending` until enough arrive; `flush` zero-pads the tail at the end.
struct RateConverter {
    resampler: SincFixedIn<f32>,
    /// Output frames per input frame (16 000 / device rate).
    ratio: f64,
    pending: Vec<f32>,
    out: Vec<Vec<f32>>,
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
                Ok((_, written)) => into.extend_from_slice(&self.out[0][..written]),
                Err(e) => log::warn!("resample failed: {e}"),
            }
            offset += chunk;
        }
        self.pending.drain(..offset);
    }

    /// Pushes out the buffered remainder (zero-padded) and the filter delay.
    fn flush(&mut self, into: &mut Vec<f32>) {
        if self.pending.is_empty() {
            return;
        }
        let tail = std::mem::take(&mut self.pending);
        let input = [tail.as_slice()];
        match self
            .resampler
            .process_partial_into_buffer(Some(&input), &mut self.out, None)
        {
            Ok((_, written)) => {
                // Only the part that corresponds to real input; the rest is
                // the zero padding rendered through the filter.
                let keep = ((tail.len() as f64 * self.ratio).round() as usize).min(written);
                into.extend_from_slice(&self.out[0][..keep]);
            }
            Err(e) => log::warn!("resample flush failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_mix_averages_channels() {
        let stereo = [0.5f32, -0.5, 1.0, 0.0, 0.25, 0.75];
        assert_eq!(mix_to_mono(&stereo, 2), vec![0.0, 0.5, 0.5]);
        // Mono passes through unchanged; a trailing partial frame is dropped.
        assert_eq!(mix_to_mono(&[0.1f32, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
        assert_eq!(mix_to_mono(&[1.0f32, 1.0, 1.0], 2), vec![1.0]);
    }

    #[test]
    fn mono_mix_converts_integer_formats() {
        // cpal integer samples scale to [-1, 1] before mixing.
        let i16s = [i16::MAX, 0, i16::MIN, i16::MIN];
        let mixed = mix_to_mono(&i16s, 2);
        assert!((mixed[0] - 0.5).abs() < 1e-4, "{mixed:?}");
        assert!((mixed[1] + 1.0).abs() < 1e-4, "{mixed:?}");
        let u16s = [u16::MAX, 32768u16];
        let mixed = mix_to_mono(&u16s, 1);
        assert!(
            (mixed[0] - 1.0).abs() < 1e-3 && mixed[1].abs() < 1e-4,
            "{mixed:?}"
        );
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
    fn rms_of_known_signal() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
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

        let expected = Recorder::SAMPLE_RATE as usize;
        let diff = out.len().abs_diff(expected);
        assert!(
            diff <= 64,
            "got {} samples, expected ~{expected}",
            out.len()
        );

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
        assert!(out.len().abs_diff(160) <= 4, "got {}", out.len());
        // Flushing again with nothing pending is a no-op.
        conv.flush(&mut out);
        assert!(out.len().abs_diff(160) <= 4);
    }
}
