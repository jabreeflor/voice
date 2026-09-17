//! In-app model downloader (Hugging Face is the only remote host the app ever
//! talks to). Reference: `ModelDownloader` in Sources/VoiceCore/core.swift.

use std::path::PathBuf;

use crate::config::ModelSpec;

pub type ProgressFn = Box<dyn Fn(&str, f64) + Send + Sync>;
pub type FinishedFn = Box<dyn Fn(&str, Option<PathBuf>) + Send + Sync>;

pub struct ModelDownloader {
    _private: (),
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelDownloader {
    pub fn new() -> ModelDownloader {
        todo!()
    }

    pub fn is_downloading(&self, file: &str) -> bool {
        let _ = file;
        todo!()
    }

    /// 0..1 while a download is in flight.
    pub fn progress(&self, file: &str) -> Option<f64> {
        let _ = file;
        todo!()
    }

    pub fn set_on_progress(&self, f: ProgressFn) {
        let _ = f;
        todo!()
    }

    /// `None` path = failed.
    pub fn set_on_finished(&self, f: FinishedFn) {
        let _ = f;
        todo!()
    }

    /// Starts a background download into `Config::models_dirs()[0]`. No-op if
    /// that file is already downloading.
    pub fn download(&self, spec: &ModelSpec) {
        let _ = spec;
        todo!()
    }
}
