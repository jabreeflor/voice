//! In-app model downloader (Hugging Face is the only remote host the app ever
//! talks to). Reference: `ModelDownloader` in Sources/VoiceCore/core.swift.
//!
//! The Swift original ran its delegate callbacks on the main queue. Here
//! `on_progress`/`on_finished` fire on the download thread; a GUI host
//! marshals to its UI thread itself.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::{Config, ModelCatalog, ModelSpec};

pub type ProgressFn = Box<dyn Fn(&str, f64) + Send + Sync>;
pub type FinishedFn = Box<dyn Fn(&str, Option<PathBuf>) + Send + Sync>;

type SharedProgressFn = Arc<dyn Fn(&str, f64) + Send + Sync>;
type SharedFinishedFn = Arc<dyn Fn(&str, Option<PathBuf>) + Send + Sync>;

const COPY_BUFFER: usize = 64 * 1024;

/// Ceiling on receiving the whole body. ureq's `recv_body` budget is total,
/// not per-read (there is no inactivity timeout like URLSession's 60 s), so it
/// has to be generous enough for a 1.6 GB model on a slow link — but bounded,
/// or a half-open connection would pin `progress[file]` forever and make every
/// retry a silent no-op.
const BODY_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);

pub struct ModelDownloader {
    directory: PathBuf,
    state: Arc<State>,
}

struct State {
    /// model file -> 0..1 while in flight; absent when not downloading.
    progress: Mutex<HashMap<String, f64>>,
    on_progress: Mutex<Option<SharedProgressFn>>,
    on_finished: Mutex<Option<SharedFinishedFn>>,
    /// Default config on purpose: it honours HTTP(S)_PROXY / NO_PROXY from the
    /// environment (a proxied machine still needs to reach Hugging Face) and
    /// follows the `resolve/` redirect to the CDN. Status codes are inspected
    /// by hand so a 404 body is never streamed into a `.part` file.
    agent: ureq::Agent,
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelDownloader {
    /// Downloads land in `Config::models_dirs()[0]`.
    pub fn new() -> ModelDownloader {
        let dir = Config::models_dirs()
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("models"));
        ModelDownloader::with_directory(dir)
    }

    /// Same downloader targeting an explicit directory (tests use a temp dir).
    pub fn with_directory(directory: PathBuf) -> ModelDownloader {
        Self::with_directory_and_body_timeout(directory, BODY_TIMEOUT)
    }

    /// `with_directory` with an explicit body-receive ceiling, so a test can
    /// prove a stalled transfer fails without waiting the production hours.
    pub fn with_directory_and_body_timeout(
        directory: PathBuf,
        body_timeout: Duration,
    ) -> ModelDownloader {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // No global timeout: a 1.6 GB model on a slow link takes a while.
            // Connect/header timeouts still catch a dead host; the body has
            // its own (large) ceiling so a wedged transfer still ends.
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(body_timeout))
            .build()
            .into();
        ModelDownloader {
            directory,
            state: Arc::new(State {
                progress: Mutex::new(HashMap::new()),
                on_progress: Mutex::new(None),
                on_finished: Mutex::new(None),
                agent,
            }),
        }
    }

    pub fn directory(&self) -> &PathBuf {
        &self.directory
    }

    pub fn is_downloading(&self, file: &str) -> bool {
        lock(&self.state.progress).contains_key(file)
    }

    /// 0..1 while a download is in flight.
    pub fn progress(&self, file: &str) -> Option<f64> {
        lock(&self.state.progress).get(file).copied()
    }

    pub fn set_on_progress(&self, f: ProgressFn) {
        *lock(&self.state.on_progress) = Some(Arc::from(f));
    }

    /// `None` path = failed.
    pub fn set_on_finished(&self, f: FinishedFn) {
        *lock(&self.state.on_finished) = Some(Arc::from(f));
    }

    /// Starts a background download into `Config::models_dirs()[0]`. No-op if
    /// that file is already downloading.
    pub fn download(&self, spec: &ModelSpec) {
        self.download_from(spec, &ModelCatalog::download_url(spec));
    }

    /// `download` with an explicit source URL (tests point it at a local
    /// server). Writes `<dir>/<file>.part` and renames on success; any
    /// existing destination is replaced.
    pub fn download_from(&self, spec: &ModelSpec, url: &str) {
        let file = spec.file.to_string();
        {
            let mut progress = lock(&self.state.progress);
            if progress.contains_key(&file) {
                return;
            }
            progress.insert(file.clone(), 0.0);
        }
        let state = Arc::clone(&self.state);
        let directory = self.directory.clone();
        let url = url.to_string();
        let spawned = thread::Builder::new()
            .name(format!("model-download-{file}"))
            .spawn(move || {
                let result = fetch_to_directory(&state, &file, &url, &directory);
                lock(&state.progress).remove(&file);
                let callback = lock(&state.on_finished).clone();
                if let Some(cb) = callback {
                    cb(&file, result);
                }
            });
        if spawned.is_err() {
            // Could not even start a thread: report the failure the same way
            // a failed transfer would, so the UI never waits forever.
            lock(&self.state.progress).remove(spec.file);
            let callback = lock(&self.state.on_finished).clone();
            if let Some(cb) = callback {
                cb(spec.file, None);
            }
        }
    }
}

/// Streams `url` to `<directory>/<file>.part`, then renames it into place.
/// `Some(dest)` only for HTTP 200 with the complete body; every failure
/// leaves no `.part` behind.
fn fetch_to_directory(
    state: &State,
    file: &str,
    url: &str,
    directory: &PathBuf,
) -> Option<PathBuf> {
    let response = state.agent.get(url).call().ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    if std::fs::create_dir_all(directory).is_err() {
        return None;
    }
    let dest = directory.join(file);
    let part = directory.join(format!("{file}.part"));

    let body = response.into_body();
    let total = body.content_length();
    let mut reader = body.into_reader();
    let copied = (|| -> std::io::Result<()> {
        let mut out = std::fs::File::create(&part)?;
        let mut buf = vec![0u8; COPY_BUFFER];
        let mut written: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            written += n as u64;
            // Like URLSession's didWriteData: no progress without a known
            // total, so an unknown-length body stays at 0 until it finishes.
            if let Some(total) = total.filter(|t| *t > 0) {
                let p = (written as f64 / total as f64).min(1.0);
                lock(&state.progress).insert(file.to_string(), p);
                let callback = lock(&state.on_progress).clone();
                if let Some(cb) = callback {
                    cb(file, p);
                }
            }
        }
        out.flush()?;
        if let Some(total) = total {
            if written != total {
                return Err(std::io::Error::other("short body"));
            }
        }
        Ok(())
    })();

    if copied.is_err() {
        let _ = std::fs::remove_file(&part);
        return None;
    }
    let _ = std::fs::remove_file(&dest);
    match std::fs::rename(&part, &dest) {
        Ok(()) => Some(dest),
        Err(_) => {
            let _ = std::fs::remove_file(&part);
            None
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
