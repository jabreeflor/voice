//! Microphone capture. Reference: `Recorder` in Sources/VoiceCore/core.swift.
//!
//! Opens the default input device with cpal, converts whatever format the
//! device delivers to mono f32 at 16 kHz (the rate whisper.cpp expects) and
//! accumulates samples until `stop()`.

#![allow(dead_code)]

use std::time::Duration;

pub struct Recorder {
    _private: (),
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub const SAMPLE_RATE: u32 = 16_000;

    pub fn new() -> Recorder {
        todo!()
    }

    /// Starts capturing. Errors carry a user-facing message ("No microphone
    /// input (check mic permission)." when no device/format is available).
    pub fn start(&mut self) -> Result<(), String> {
        todo!()
    }

    /// Stops and returns the 16 kHz mono samples (empty if not recording).
    pub fn stop(&mut self) -> Vec<f32> {
        todo!()
    }

    pub fn cancel(&mut self) {
        let _ = self.stop();
    }

    pub fn is_recording(&self) -> bool {
        todo!()
    }

    /// Smoothed input level 0..1: fast attack, slow decay
    /// (`max(min(rms * 9, 1), level * 0.82)` per buffer).
    pub fn level(&self) -> f32 {
        todo!()
    }

    /// Time since `start()` (zero when idle).
    pub fn duration(&self) -> Duration {
        todo!()
    }
}
