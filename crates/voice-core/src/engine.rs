//! whisper.cpp server wrapper: keeps the model loaded for fast responses.
//! Reference: `WhisperEngine` in Sources/VoiceCore/core.swift.
//!
//! Privacy invariant: the server is always started with `--host 127.0.0.1`.
//!
//! The Swift original delivered status changes on the main queue. There is no
//! main queue here: `set_on_status_change` callbacks run *synchronously* on
//! whichever thread changed the status — inside `start()` on the caller's own
//! thread, or on the readiness/exit supervisor thread. Because the callback
//! can re-enter the caller of `start()`, it must not acquire a lock that
//! caller already holds and must not call back into the engine; it should only
//! post to the host's UI loop.

use std::ffi::OsStr;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::wav::wav_data;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Reserved for the app layer (no model configured at all). `transcribe`
    /// itself reports a missing model on the cold path as [`NotReady`].
    ///
    /// [`NotReady`]: EngineError::NotReady
    #[error("no model found")]
    NoModel,
    /// Reserved for the app layer (whisper.cpp not installed at all).
    /// `transcribe` reports a missing `whisper-cli` on the cold path as
    /// [`NotReady`]; a missing `whisper-server` goes through `status_text()`.
    ///
    /// [`NotReady`]: EngineError::NotReady
    #[error("Whisper engine not ready (install whisper.cpp)")]
    NotInstalled,
    #[error("Bad response from whisper engine")]
    BadResponse,
    #[error("whisper engine request failed: {0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The server is not up and the `whisper-cli` fallback is impossible too
    /// (no model, or no `whisper-cli` binary). Same wording as the Swift
    /// original's cold-path error, which is what the user sees.
    #[error("Whisper engine not ready (install whisper.cpp)")]
    NotReady,
}

/// Same fixed boundary as the Swift client, so a captured request is
/// byte-identical between the two implementations.
const MULTIPART_BOUNDARY: &str = "VoiceBoundary7f3a9c";

const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(300);
const READINESS_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(120);

/// Keeps a console-subsystem child (whisper-server.exe, whisper-cli.exe) off
/// screen: the app is a GUI-subsystem binary with no console of its own, so
/// without this flag Windows would allocate a visible console window for the
/// child. `Stdio::null()` does not prevent that; only the creation flag does.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Shareable handle (`Send + Sync`); clone-free sharing via `Arc`.
pub struct WhisperEngine {
    model: Option<PathBuf>,
    port: u16,
    max_readiness_polls: usize,
    inner: Arc<Inner>,
}

struct Inner {
    ready: AtomicBool,
    status: Mutex<String>,
    on_status_change: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    child: Mutex<Option<Child>>,
    /// Bumped by every `start`/`stop`. A supervisor thread from an earlier
    /// generation must not report "engine stopped" for a child it no longer
    /// owns, e.g. after `stop()` followed by a fresh `start()`.
    generation: AtomicU64,
    /// Local-only traffic: never routed through an HTTP(S)_PROXY from the
    /// environment, and any HTTP response (even a 4xx) counts as "the server
    /// is up" during readiness polling, as in the Swift client.
    agent: ureq::Agent,
}

impl WhisperEngine {
    /// The app uses `(model, Config::SERVER_PORT, 120)`; tests pass port 18178.
    pub fn new(model: Option<PathBuf>, port: u16, max_readiness_polls: usize) -> WhisperEngine {
        WhisperEngine {
            model,
            port,
            max_readiness_polls,
            inner: Arc::new(Inner {
                ready: AtomicBool::new(false),
                status: Mutex::new("starting…".to_string()),
                on_status_change: Mutex::new(None),
                child: Mutex::new(None),
                generation: AtomicU64::new(0),
                agent: local_agent(),
            }),
        }
    }

    /// An engine that believes a server is already listening on `port`
    /// (ready, status "ready", no child). For tests that stand in a fake
    /// HTTP server; never spawns anything.
    pub fn for_test_server(port: u16) -> WhisperEngine {
        let engine = WhisperEngine::new(None, port, 0);
        engine.inner.ready.store(true, Ordering::SeqCst);
        *lock(&engine.inner.status) = "ready".to_string();
        engine
    }

    /// Locates `whisper-server` / `whisper-cli` next to the executable, on PATH
    /// (with `.exe` on Windows) and in the usual install prefixes.
    pub fn find_binary(name: &str) -> Option<PathBuf> {
        let beside_exe: Vec<PathBuf> = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .into_iter()
            .collect();
        let path_var = std::env::var("PATH").ok();
        Self::find_binary_in(name, path_var.as_deref(), &beside_exe)
    }

    /// Testable core of [`find_binary`](Self::find_binary). Search order:
    /// `extra_dirs` (the executable's own directory in production), each
    /// entry of `path_var` (a `PATH`-formatted string), then the fixed
    /// install prefixes.
    pub fn find_binary_in(
        name: &str,
        path_var: Option<&str>,
        extra_dirs: &[PathBuf],
    ) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = extra_dirs.to_vec();
        if let Some(path_var) = path_var {
            dirs.extend(std::env::split_paths(path_var));
        }
        dirs.extend(install_prefixes());
        dirs.into_iter()
            .filter(|dir| !dir.as_os_str().is_empty())
            .flat_map(|dir| candidate_names(name).map(move |n| dir.join(n)))
            .find(|candidate| is_executable(candidate))
    }

    pub fn start(&self) {
        let Some(model) = self.model.as_ref() else {
            self.set_status("no model found");
            return;
        };
        let Some(bin) = Self::find_binary("whisper-server") else {
            self.set_status("whisper-server not installed");
            return;
        };
        let cpus = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let threads = std::cmp::max(4, cpus.saturating_sub(2));
        // A previous child would keep the port and make the new bind fail;
        // dropping a `Child` does not kill it, so reap it explicitly.
        if let Some(mut old) = lock(&self.inner.child).take() {
            let _ = old.kill();
            let _ = old.wait();
        }
        let spawned = hidden_command(bin)
            .arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1"])
            .args(["--port", &self.port.to_string()])
            .args(["-t", &threads.to_string()])
            .args(["-bs", "1"]) // greedy decoding — ~2x faster than beam search
            .arg("-nf") // no temperature fallback — kills worst-case retries
            .stdin(Stdio::null())
            .stdout(engine_log_stdio())
            .stderr(engine_log_stdio())
            .spawn();
        let child = match spawned {
            Ok(child) => child,
            Err(_) => {
                self.set_status("failed to launch engine");
                return;
            }
        };
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *lock(&self.inner.child) = Some(child);
        self.set_status("loading model…");

        let inner = Arc::clone(&self.inner);
        let port = self.port;
        let max_polls = self.max_readiness_polls;
        if thread::Builder::new()
            .name("whisper-engine-supervisor".into())
            .spawn(move || supervise(inner, generation, port, max_polls))
            .is_err()
        {
            // Nothing would ever poll readiness or reap this child, so treat
            // it like a launch failure rather than sitting on "loading model…".
            if let Some(mut child) = lock(&self.inner.child).take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.set_status("failed to launch engine");
        }
    }

    pub fn stop(&self) {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.ready.store(false, Ordering::SeqCst);
        if let Some(mut child) = lock(&self.inner.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }

    pub fn status_text(&self) -> String {
        lock(&self.inner.status).clone()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn model(&self) -> Option<&Path> {
        self.model.as_deref()
    }

    /// Registers the status-change callback. It runs synchronously on the
    /// thread that changed the status: on the caller's thread from inside
    /// `start()` (so it can fire before `start()` returns), and on the
    /// supervisor thread for "ready" / "engine stopped" / "engine did not
    /// start". It may therefore re-enter the caller of `start()`: do not
    /// acquire a lock that caller already holds, and do not call back into
    /// the engine — only post to the UI loop. (`status_text` and
    /// `set_on_status_change` are the exceptions: they are safe to call.)
    pub fn set_on_status_change(&self, f: Box<dyn Fn() + Send + Sync>) {
        *lock(&self.inner.on_status_change) = Some(Arc::from(f));
    }

    /// Blocking. Server when ready, `whisper-cli` fallback otherwise.
    pub fn transcribe(&self, wav: &[u8]) -> Result<String, EngineError> {
        if self.ready() {
            transcribe_via_server(&self.inner.agent, self.port, wav)
        } else {
            self.transcribe_via_cli(wav)
        }
    }

    fn set_status(&self, s: &str) {
        self.inner.set_status(s);
    }

    /// Cold path used before the server is up (or when it never came up):
    /// one whisper-cli process per utterance, which reloads the model every
    /// time. `-np -nt` suppress prints and timestamps so stdout is the text.
    /// `NotReady` when the fallback is impossible, as in the Swift original.
    fn transcribe_via_cli(&self, wav: &[u8]) -> Result<String, EngineError> {
        let model = self.model.as_ref().ok_or(EngineError::NotReady)?;
        let bin = Self::find_binary("whisper-cli").ok_or(EngineError::NotReady)?;
        let dir = create_cli_scratch_dir()?;
        let tmp = dir.join("audio.wav");
        let result = (|| {
            std::fs::write(&tmp, wav)?;
            let output = hidden_command(bin)
                .arg("-m")
                .arg(model)
                .arg("-f")
                .arg(&tmp)
                .args(["-np", "-nt", "-bs", "1", "-nf"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()?;
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

/// Per-process counter for `transcribe_via_cli` scratch directories.
static CLI_TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Attempts at a fresh scratch-directory name before giving up.
const CLI_TMP_ATTEMPTS: u32 = 8;

/// A private directory rather than a predictably named file in the shared
/// temp dir: `create_dir` (never `create_dir_all`) fails instead of following
/// a symlink an attacker pre-planted at that name (on Linux `/tmp` is
/// world-writable), and the per-process counter keeps concurrent calls from
/// colliding on one path. `AlreadyExists` is retried with the next counter
/// value: a SIGKILLed run leaves `voice-<pid>-0` behind, and a later process
/// handed the recycled pid must not fail its first cold-path transcription
/// over that stale directory.
fn create_cli_scratch_dir() -> std::io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let mut last_err = None;
    for _ in 0..CLI_TMP_ATTEMPTS {
        let dir = base.join(format!(
            "voice-{pid}-{}",
            CLI_TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last_err = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("scratch dir")))
}

/// whisper-server's own log is discarded so it never spams the app's stderr,
/// except when `VOICE_ENGINE_LOG` is set: CI turns that on so a server that
/// receives bad audio or dies on launch explains itself in the job log.
fn engine_log_stdio() -> Stdio {
    if std::env::var_os("VOICE_ENGINE_LOG").is_some() {
        Stdio::inherit()
    } else {
        Stdio::null()
    }
}

/// `Command::new` plus the Windows no-console flag (a no-op elsewhere).
fn hidden_command(bin: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new(bin);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    Command::new(bin)
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Inner {
    fn set_status(&self, s: &str) {
        *lock(&self.status) = s.to_string();
        // Clone out of the lock so a callback may itself call
        // `set_on_status_change` or `status_text` without deadlocking.
        let callback = lock(&self.on_status_change).clone();
        if let Some(cb) = callback {
            cb();
        }
    }

    /// `Some(true)` running, `Some(false)` exited, `None` no child (stopped).
    fn child_running(&self) -> Option<bool> {
        let mut guard = lock(&self.child);
        let child = guard.as_mut()?;
        Some(matches!(child.try_wait(), Ok(None)))
    }
}

/// Readiness polling followed by exit watching, for one `start()` generation.
fn supervise(inner: Arc<Inner>, generation: u64, port: u16, max_polls: usize) {
    let current = || inner.generation.load(Ordering::SeqCst) == generation;
    let url = format!("http://127.0.0.1:{port}/");

    let mut became_ready = false;
    for _ in 0..max_polls {
        if !current() || inner.child_running() != Some(true) {
            break;
        }
        let probe = inner
            .agent
            .get(&url)
            .config()
            .timeout_global(Some(READINESS_PROBE_TIMEOUT))
            .build()
            .call();
        if probe.is_ok() {
            became_ready = true;
            break;
        }
        thread::sleep(READINESS_POLL_INTERVAL);
    }
    if !current() {
        return;
    }

    if became_ready {
        inner.ready.store(true, Ordering::SeqCst);
        inner.set_status("ready");
        warm_up(&inner.agent, port);
    } else if inner.child_running() == Some(true) {
        // Give up: kill the child too, or a late-binding server would be
        // left running untracked, squatting on the port forever.
        let Some(mut child) = lock(&inner.child).take() else {
            return;
        };
        let _ = child.kill();
        let _ = child.wait();
        if !current() {
            // stop() won the race; an intentional shutdown must not be
            // displayed as "engine did not start".
            return;
        }
        inner.ready.store(false, Ordering::SeqCst);
        inner.set_status("engine did not start");
        return;
    }

    // Exit watch: mirrors Process.terminationHandler. `stop()` takes the
    // child out of the slot first, so an intentional stop never reports
    // "engine stopped".
    loop {
        thread::sleep(READINESS_POLL_INTERVAL);
        if !current() {
            return;
        }
        match inner.child_running() {
            Some(true) => continue,
            None => return,
            Some(false) => {
                if let Some(mut child) = lock(&inner.child).take() {
                    let _ = child.wait();
                }
                inner.ready.store(false, Ordering::SeqCst);
                inner.set_status("engine stopped");
                return;
            }
        }
    }
}

/// Push a half-second of silence through the model right after load so
/// GPU kernels are compiled before the user's first real dictation.
fn warm_up(agent: &ureq::Agent, port: u16) {
    let silence = wav_data(&[0.0; 8000]);
    let _ = transcribe_via_server(agent, port, &silence);
}

fn transcribe_via_server(
    agent: &ureq::Agent,
    port: u16,
    wav: &[u8],
) -> Result<String, EngineError> {
    let body = multipart_body(wav);
    let response = agent
        .post(format!("http://127.0.0.1:{port}/inference"))
        .config()
        .timeout_global(Some(INFERENCE_TIMEOUT))
        .build()
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        )
        .send(body)
        .map_err(|e| EngineError::Http(e.to_string()))?;
    let text = response
        .into_body()
        .read_to_string()
        .map_err(|e| EngineError::Http(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| EngineError::BadResponse)?;
    match value.get("text") {
        Some(serde_json::Value::String(s)) => Ok(s.clone()),
        _ => Err(EngineError::BadResponse),
    }
}

/// Exactly the Swift form: three text fields, then `file` as `audio/wav`.
fn multipart_body(wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 512);
    let mut field = |name: &str, value: &str| {
        let _ = write!(
            body,
            "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        );
    };
    field("temperature", "0.0");
    field("temperature_inc", "0.0");
    field("response_format", "json");
    let _ = write!(
        body,
        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    );
    body.extend_from_slice(wav);
    let _ = write!(body, "\r\n--{MULTIPART_BOUNDARY}--\r\n");
    body
}

fn local_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into()
}

fn install_prefixes() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/opt/whisper-cpp/bin"),
        PathBuf::from("/usr/bin"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("voice").join("bin"));
    }
    dirs
}

/// On Windows a bare name is tried too, so a `whisper-server` shim without an
/// extension (e.g. from a package manager) is still found.
fn candidate_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let exe = if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        Some(format!("{name}.exe"))
    } else {
        None
    };
    exe.into_iter().chain(std::iter::once(name.to_string()))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Status/child state stays consistent even if a callback panicked while a
/// lock was held; poisoning would otherwise turn one bad callback into a
/// permanently dead engine.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn engine_is_send_and_sync() {
        assert_send_sync::<WhisperEngine>();
    }

    #[test]
    fn multipart_body_matches_swift_layout() {
        let body = multipart_body(b"RIFFxxxx");
        let text = String::from_utf8_lossy(&body);
        let expected = "--VoiceBoundary7f3a9c\r\nContent-Disposition: form-data; name=\"temperature\"\r\n\r\n0.0\r\n\
--VoiceBoundary7f3a9c\r\nContent-Disposition: form-data; name=\"temperature_inc\"\r\n\r\n0.0\r\n\
--VoiceBoundary7f3a9c\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n\
--VoiceBoundary7f3a9c\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFFxxxx\r\n\
--VoiceBoundary7f3a9c--\r\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn candidate_names_add_exe_only_on_windows() {
        let names: Vec<String> = candidate_names("whisper-server").collect();
        if cfg!(windows) {
            assert_eq!(names, ["whisper-server.exe", "whisper-server"]);
        } else {
            assert_eq!(names, ["whisper-server"]);
        }
    }
}
