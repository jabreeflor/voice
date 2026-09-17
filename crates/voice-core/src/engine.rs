//! whisper.cpp server wrapper: keeps the model loaded for fast responses.
//! Reference: `WhisperEngine` in Sources/VoiceCore/core.swift.
//!
//! Privacy invariant: the server is always started with `--host 127.0.0.1`.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no model found")]
    NoModel,
    #[error("Whisper engine not ready (install whisper.cpp: whisper-server)")]
    NotInstalled,
    #[error("Bad response from whisper engine")]
    BadResponse,
    #[error("whisper engine request failed: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Whisper engine not ready")]
    NotReady,
}

/// Shareable handle (`Send + Sync`); clone-free sharing via `Arc`.
pub struct WhisperEngine {
    _private: (),
}

impl WhisperEngine {
    /// The app uses `(model, Config::SERVER_PORT, 120)`; tests pass port 18178.
    pub fn new(model: Option<PathBuf>, port: u16, max_readiness_polls: usize) -> WhisperEngine {
        let _ = (model, port, max_readiness_polls);
        todo!()
    }

    /// Locates `whisper-server` / `whisper-cli` next to the executable, on PATH
    /// (with `.exe` on Windows) and in the usual install prefixes.
    pub fn find_binary(name: &str) -> Option<PathBuf> {
        let _ = name;
        todo!()
    }

    pub fn start(&self) {
        todo!()
    }

    pub fn stop(&self) {
        todo!()
    }

    pub fn ready(&self) -> bool {
        todo!()
    }

    pub fn status_text(&self) -> String {
        todo!()
    }

    pub fn port(&self) -> u16 {
        todo!()
    }

    pub fn model(&self) -> Option<&Path> {
        todo!()
    }

    pub fn set_on_status_change(&self, f: Box<dyn Fn() + Send + Sync>) {
        let _ = f;
        todo!()
    }

    /// Blocking. Server when ready, `whisper-cli` fallback otherwise.
    pub fn transcribe(&self, wav: &[u8]) -> Result<String, EngineError> {
        let _ = wav;
        todo!()
    }
}
