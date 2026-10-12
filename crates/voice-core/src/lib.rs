//! voice-core: everything in voice that is not a window, a microphone or a
//! keyboard hook. This crate has no GUI or audio dependencies, so it builds
//! and tests on every platform (including Linux CI without a display).
//!
//! Module map (mirrors the original Swift `VoiceCore` MARK sections):
//! - `settings`   JSON-backed key/value store replacing `UserDefaults`
//! - `config`     hotkey choices, model discovery, model catalog
//! - `transcript` `clean_transcript`
//! - `wav`        `wav_data` RIFF encoder
//! - `status`     `compute_status` decision table
//! - `store`      dictation history, snippets, vocabulary
//! - `engine`     whisper-server child process + HTTP client
//! - `download`   model downloader
//! - `cli`        voicectl argument parsing and commands

pub mod cli;
pub mod config;
pub mod download;
pub mod engine;
pub mod settings;
pub mod status;
pub mod store;
pub mod transcript;
pub mod wav;

pub use cli::{CliResult, VoiceCli};
pub use config::{expand_tilde, Appearance, Config, Hotkey, ModelCatalog, ModelSpec};
pub use download::ModelDownloader;
pub use engine::{whisper_prompt, EngineError, WhisperEngine, PROMPT_CHAR_BUDGET};
pub use settings::Settings;
pub use status::{compute_status, Platform, StatusColor, StatusInfo, StatusInputs};
pub use store::{DictationEntry, HistoryStore, Snippet, SnippetStore, Store, VocabularyStore};
pub use transcript::clean_transcript;
pub use wav::wav_data;
