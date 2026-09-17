//! Tier-2 integration tests: a real `whisper-server` process, real audio, real
//! transcripts. Nothing in `test1..test5` is mocked. Reference:
//! Tests/VoiceCoreTests/EngineIntegrationTests.swift.
//!
//! These tests skip (print `SKIPPED: <reason>` and return) rather than fail
//! when `whisper-server` or a ggml model is missing, so machines without
//! whisper.cpp installed stay green. CI greps the log for `SKIPPED` and fails,
//! because there the environment is expected to be complete. `#[ignore]` is
//! deliberately not used: an ignored test would not print anything for CI to
//! catch.
//!
//! The server is booted once per process (`OnceLock`) and shared by every
//! test — model load dominates the runtime. Cargo runs each integration test
//! file as its own process, so this file never overlaps itself;
//! `reclaim_test_port` covers a server orphaned by an earlier crashed run.
//!
//! The unit tests at the bottom (`find_binary_*`, fake-server `transcribe`)
//! need no whisper-server and always run.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use voice_core::config::{Config, ModelCatalog};
use voice_core::engine::{EngineError, WhisperEngine};
use voice_core::{clean_transcript, wav_data, Settings};

/// Deliberately not `Config::SERVER_PORT` (8178): the user's installed Voice
/// app owns that port on a dev machine and must not be disturbed.
const TEST_PORT: u16 = 18178;

/// Cold model load plus GPU shader compilation. Measured at ~0.5 s warm, but
/// seen as high as 43 s on a machine under heavy parallel build load — hence
/// the wide margin. Exceeding it skips rather than fails.
const BOOT_TIMEOUT: Duration = Duration::from_secs(60);

/// Per-request ceiling. Far above the observed ~0.4 s so a stalled request
/// surfaces as a clear failure instead of hanging the suite.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

const FOX_PHRASE: &str = "The quick brown fox jumps over the lazy dog.";
const SECOND_PHRASE: &str = "Hello world, this is a test of the voice engine.";

/// Number of `test1..test5` functions below. Once that many have finished the
/// shared server is stopped, because a `static` engine is never dropped and a
/// child process outlives its parent. Running a filtered subset leaves the
/// server for `reclaim_test_port` on the next run.
const INTEGRATION_TEST_COUNT: usize = 5;

struct Booted {
    engine: WhisperEngine,
    boot_seconds: f64,
    model_path: PathBuf,
    fox_wav: Vec<u8>,
    fox_duration: f64,
    second_wav: Vec<u8>,
}

static BOOT: OnceLock<Result<Booted, String>> = OnceLock::new();
static FINISHED: AtomicUsize = AtomicUsize::new(0);

/// The shared engine, or `None` after printing the skip reason.
fn booted() -> Option<&'static Booted> {
    match BOOT.get_or_init(prepare_once) {
        Ok(b) => Some(b),
        Err(reason) => {
            // libtest captures println! and only replays it for failing
            // tests; CI greps the log for this line, so it has to reach the
            // real stdout regardless of --nocapture.
            let _ = writeln!(std::io::stdout().lock(), "SKIPPED: {reason}");
            None
        }
    }
}

/// Decrements the outstanding-test count on every exit path (including a
/// panicking assertion) so the last test always stops the server.
struct Completion;

impl Drop for Completion {
    fn drop(&mut self) {
        if FINISHED.fetch_add(1, Ordering::SeqCst) + 1 == INTEGRATION_TEST_COUNT {
            if let Some(Ok(b)) = BOOT.get() {
                b.engine.stop();
            }
            reclaim_test_port();
        }
    }
}

/// Prerequisite detection, fixture decoding and the single server boot.
/// Records a skip reason instead of failing when the environment can't run.
fn prepare_once() -> Result<Booted, String> {
    if WhisperEngine::find_binary("whisper-server").is_none() {
        return Err("whisper-server not installed (brew install whisper-cpp)".into());
    }
    let model = resolve_model().ok_or_else(|| {
        let dirs: Vec<String> = Config::models_dirs()
            .iter()
            .map(|d| d.display().to_string())
            .collect();
        format!("no ggml model found in {}", dirs.join(", "))
    })?;

    let fox_samples = fixture_samples("fox.wav")?;
    let fox_duration = fox_samples.len() as f64 / 16_000.0;
    let fox_wav = wav_data(&fox_samples);
    let second_wav = wav_data(&fixture_samples("hello.wav")?);
    if fox_wav.len() <= 44 || second_wav.len() <= 44 {
        return Err("speech fixtures came out empty".into());
    }

    reclaim_test_port();

    let engine = WhisperEngine::new(Some(model.clone()), TEST_PORT, 120);
    let start = Instant::now();
    engine.start();
    let became_ready = spin_until(BOOT_TIMEOUT, || engine.ready());
    let boot_seconds = start.elapsed().as_secs_f64();
    if !became_ready {
        let reason = format!(
            "whisper-server did not become ready on port {TEST_PORT} within {}s (status: {}, model: {})",
            BOOT_TIMEOUT.as_secs(),
            engine.status_text(),
            model.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
        );
        engine.stop();
        // The engine discards the child's stdio, so a server that dies on
        // launch (missing shared library, bad flag) leaves no trace. Run the
        // binary once more with --help and surface what it says, which is
        // usually the whole diagnosis when this skip shows up on CI.
        let probe = WhisperEngine::find_binary("whisper-server")
            .map(|bin| {
                std::process::Command::new(bin)
                    .arg("--help")
                    .output()
                    .map(|o| {
                        let text = String::from_utf8_lossy(&o.stderr).to_string()
                            + &String::from_utf8_lossy(&o.stdout);
                        format!(
                            "probe exit {:?}: {}",
                            o.status.code(),
                            text.lines().take(3).collect::<Vec<_>>().join(" | ")
                        )
                    })
                    .unwrap_or_else(|e| format!("probe failed to run: {e}"))
            })
            .unwrap_or_else(|| "probe: binary not found".to_string());
        return Err(format!("{reason}; {probe}"));
    }

    let booted = Booted {
        engine,
        boot_seconds,
        model_path: model,
        fox_wav,
        fox_duration,
        second_wav,
    };
    // `start()` kicks off an internal warm-up transcription. Run one more and
    // wait for it so the latency test measures a settled engine rather than
    // racing the warm-up.
    let _ = booted.engine.transcribe(&silence_wav(0.5, 0.0));
    Ok(booted)
}

// MARK: - Tests

#[test]
fn test1_server_boots_and_becomes_ready() {
    let _done = Completion;
    let Some(b) = booted() else { return };
    assert!(b.engine.ready(), "engine should be ready after boot");
    assert_eq!(b.engine.status_text(), "ready");
    assert_eq!(
        b.engine.port(),
        TEST_PORT,
        "must not touch the app's port 8178"
    );
    assert_eq!(b.engine.model(), Some(b.model_path.as_path()));
    assert!(
        b.boot_seconds < BOOT_TIMEOUT.as_secs_f64(),
        "boot exceeded the {}s budget",
        BOOT_TIMEOUT.as_secs()
    );
    log(&format!(
        "booted {} on port {TEST_PORT} in {:.2}s (fox fixture: {} WAV bytes, {:.2}s)",
        b.model_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        b.boot_seconds,
        b.fox_wav.len(),
        b.fox_duration
    ));
}

#[test]
fn test2_transcribes_speech_fixture() {
    let _done = Completion;
    let Some(b) = booted() else { return };

    let raw = transcribe_sync(&b.engine, &b.fox_wav).expect("fox transcription");
    log(&format!("fox transcript: {raw:?}"));
    assert!(
        normalize(&raw).contains("quick brown fox"),
        "expected 'quick brown fox' in transcript of {FOX_PHRASE:?}, got: {raw:?}"
    );

    let second = transcribe_sync(&b.engine, &b.second_wav).expect("second transcription");
    log(&format!("second transcript: {second:?}"));
    assert!(
        normalize(&second).contains("hello world"),
        "expected 'hello world' in transcript of {SECOND_PHRASE:?}, got: {second:?}"
    );
}

#[test]
fn test3_clean_transcript_produces_usable_text() {
    let _done = Completion;
    let Some(b) = booted() else { return };
    let raw = transcribe_sync(&b.engine, &b.fox_wav).expect("fox transcription");
    let cleaned = clean_transcript(&raw);

    assert!(
        !cleaned.is_empty(),
        "clean_transcript stripped a real utterance to nothing"
    );
    assert!(
        !cleaned.contains('['),
        "bracketed artifact survived cleanup: {cleaned}"
    );
    assert!(
        !cleaned.contains(']'),
        "bracketed artifact survived cleanup: {cleaned}"
    );
    assert!(
        !cleaned.contains('\n'),
        "newline survived cleanup: {cleaned:?}"
    );
    assert!(
        !cleaned.contains("  "),
        "double space survived cleanup: {cleaned:?}"
    );
    assert_eq!(
        cleaned,
        cleaned.trim(),
        "clean_transcript left surrounding whitespace"
    );
    assert!(
        normalize(&cleaned).contains("quick brown fox"),
        "cleanup damaged the words: {cleaned:?}"
    );
    log(&format!("cleaned transcript: {cleaned:?}"));
}

#[test]
fn test4_warm_transcription_meets_latency_budget() {
    let _done = Completion;
    let Some(b) = booted() else { return };
    assert!(
        b.fox_duration > 1.5,
        "latency fixture should be a few seconds of speech"
    );

    // Budget observed ~0.3 s warm on Apple Silicon; 5 s is the local bar.
    // GitHub's shared VMs run whisper ~50x slower (measured 11-12 s for 2.5 s
    // of audio). CI uses a tighter hang/regression ceiling than the old 60 s
    // guard so large slowdowns still fail.
    let is_ci = std::env::var_os("CI").is_some();
    let budget = if is_ci { 25.0 } else { 5.0 };
    let mut elapsed = 0.0;
    for attempt in 1..=3 {
        let start = Instant::now();
        let result = transcribe_sync(&b.engine, &b.fox_wav);
        elapsed = start.elapsed().as_secs_f64();
        result.expect("transcription");
        log(&format!(
            "latency run {attempt}: {elapsed:.2}s for {:.2}s of audio ({:.2}x realtime)",
            b.fox_duration,
            b.fox_duration / elapsed.max(0.001)
        ));
    }
    assert!(
        elapsed < budget,
        "warm transcription of {:.2}s of audio took {elapsed:.2}s, over the {budget:.2}s budget",
        b.fox_duration
    );
}

/// Documents the "No speech detected" path: whisper emits an artifact token
/// for silence, and `clean_transcript` must reduce it to the empty string so
/// the app pastes nothing rather than "[BLANK_AUDIO]".
#[test]
fn test5_silence_cleans_to_empty_string() {
    let _done = Completion;
    let Some(b) = booted() else { return };

    for (label, wav) in [
        ("digital silence", silence_wav(2.0, 0.0)),
        ("near-silence", silence_wav(2.0, 0.0005)),
    ] {
        let raw = transcribe_sync(&b.engine, &wav).expect("silence transcription");
        let cleaned = clean_transcript(&raw);
        log(&format!("{label}: raw={raw:?} cleaned={cleaned:?}"));
        assert!(
            cleaned.is_empty(),
            "{label} should clean to \"\", got {cleaned:?} from raw {raw:?}"
        );
    }
}

// MARK: - Helpers

/// Runs a (blocking) transcription on a helper thread so a stalled server
/// surfaces as a timeout error instead of hanging the whole suite.
fn transcribe_sync(engine: &'static WhisperEngine, wav: &[u8]) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    let wav = wav.to_vec();
    thread::spawn(move || {
        let _ = tx.send(engine.transcribe(&wav).map_err(|e| e.to_string()));
    });
    match rx.recv_timeout(REQUEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err(format!(
            "transcription did not complete within {}s",
            REQUEST_TIMEOUT.as_secs()
        )),
    }
}

fn spin_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while !condition() {
        if Instant::now() >= deadline {
            return condition();
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Frees the test port if a previous crashed run orphaned a server on it.
/// Only ever kills a process whose executable is `whisper-server`.
#[cfg(unix)]
fn reclaim_test_port() {
    for pid in listening_pids() {
        let comm = shell("ps", &["-o", "comm=", "-p", &pid.to_string()]).unwrap_or_default();
        if comm.contains("whisper-server") {
            let _ = shell("kill", &["-TERM", &pid.to_string()]);
        }
    }
    // Give the socket a moment to be released before a rebind.
    spin_until(Duration::from_secs(2), || listening_pids().is_empty());
}

/// Windows has no lsof/fuser; an orphan there simply makes the boot fail
/// (and skip) with a clear status.
#[cfg(not(unix))]
fn reclaim_test_port() {}

#[cfg(unix)]
fn listening_pids() -> Vec<u32> {
    let port = TEST_PORT.to_string();
    let out = shell(
        "lsof",
        &["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"],
    )
    .or_else(|| shell("fuser", &["-n", "tcp", &port]))
    .unwrap_or_default();
    out.split(|c: char| c.is_whitespace() || c == '/')
        .filter_map(|tok| tok.trim().parse::<u32>().ok())
        .collect()
}

#[cfg(unix)]
fn shell(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn resolve_model() -> Option<PathBuf> {
    // Prefer base.en: it is the app's default and what the latency budget is
    // calibrated against. The fallback scan needs a Settings only for the
    // `modelFile` preference, so a throwaway one keeps the real file untouched.
    if let Some(base) = ModelCatalog::installed_path("ggml-base.en.bin") {
        return Some(base);
    }
    let dir = tempfile::tempdir().ok()?;
    let settings = Settings::in_dir(dir.path());
    Config::find_model(&settings)
}

/// Lowercase, punctuation-free form for fuzzy matching — whisper varies
/// casing and punctuation between runs, so exact matches would be flaky.
fn normalize(s: &str) -> String {
    let stripped: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    stripped
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn log(message: &str) {
    println!("[EngineIntegration] {message}");
}

// MARK: - Fixture audio

/// Checked-in 16 kHz mono Int16 WAV, decoded to f32 and re-encoded through
/// `wav_data` so the bytes handed to the engine are byte-for-byte the same
/// shape the app produces from a live microphone capture.
fn fixture_samples(name: &str) -> Result<Vec<f32>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let data =
        std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    float_samples_from_wav(&data).map_err(|why| format!("could not decode {name}: {why}"))
}

/// Walks the RIFF chunk list to find `data`: some encoders write a large
/// header region, so the payload is not always at the canonical offset 44.
fn float_samples_from_wav(data: &[u8]) -> Result<Vec<f32>, String> {
    let u32_at = |offset: usize| -> Result<u32, String> {
        data.get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("truncated at byte {offset}"))
    };
    let tag_at = |offset: usize| -> Result<&[u8], String> {
        data.get(offset..offset + 4)
            .ok_or_else(|| format!("truncated at byte {offset}"))
    };
    if data.len() <= 12 || tag_at(0)? != b"RIFF" || tag_at(8)? != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut cursor = 12;
    let mut payload: Option<&[u8]> = None;
    while cursor + 8 <= data.len() {
        let id = tag_at(cursor)?;
        let size = u32_at(cursor + 4)? as usize;
        let body = cursor + 8;
        if id == b"data" {
            let end = (body + size).min(data.len());
            if end <= body {
                return Err("empty data chunk".into());
            }
            payload = Some(&data[body..end]);
            break;
        }
        cursor = body + size + (size % 2); // chunks are word-aligned
    }
    let pcm = payload.ok_or("no data chunk")?;
    // `as_chunks` rather than `chunks_exact`: newer clippy flags the latter
    // for constant sizes, and CI runs whatever stable clippy ships.
    let (pairs, _rest) = pcm.as_chunks::<2>();
    Ok(pairs
        .iter()
        .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
        .collect())
}

/// Digital silence, or near-silence when `amplitude` is a small non-zero value.
fn silence_wav(seconds: f64, amplitude: f32) -> Vec<u8> {
    let count = (seconds * 16_000.0) as usize;
    if amplitude <= 0.0 {
        return wav_data(&vec![0.0; count]);
    }
    // xorshift: deterministic, dependency-free noise. The exact values are
    // irrelevant; only the amplitude matters to whisper's VAD.
    let mut state: u32 = 0x9E37_79B9;
    let samples: Vec<f32> = (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let unit = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            unit * amplitude
        })
        .collect();
    wav_data(&samples)
}

// MARK: - Unit tests (no whisper-server needed)

#[test]
fn status_text_defaults_to_starting() {
    let engine = WhisperEngine::new(None, TEST_PORT, 1);
    assert_eq!(engine.status_text(), "starting…");
    assert!(!engine.ready());
    assert_eq!(engine.port(), TEST_PORT);
    assert_eq!(engine.model(), None);
}

#[test]
fn start_without_model_reports_no_model_and_notifies() {
    let engine = WhisperEngine::new(None, TEST_PORT, 1);
    let fired = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fired);
    engine.set_on_status_change(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    engine.start();
    assert_eq!(engine.status_text(), "no model found");
    assert!(!engine.ready());
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// Production stays on 8178 and tests stay on 18178 (AGENTS.md invariant);
/// Swift pinned this in SmokeTests.testPackageBuildsAndLinks.
#[test]
fn production_port_is_8178() {
    assert_eq!(Config::SERVER_PORT, 8178);
    assert_ne!(TEST_PORT, Config::SERVER_PORT);
}

/// Cold path with no model: the whisper-cli fallback is impossible, so the
/// caller gets `NotReady` with the Swift original's user-facing wording.
#[test]
fn transcribe_without_server_or_model_is_not_ready() {
    let engine = WhisperEngine::new(None, TEST_PORT, 1);
    let err = engine
        .transcribe(&wav_data(&[0.0; 100]))
        .expect_err("no server and no model cannot transcribe");
    assert!(matches!(err, EngineError::NotReady), "{err:?}");
    assert_eq!(
        err.to_string(),
        "Whisper engine not ready (install whisper.cpp)"
    );
}

#[test]
fn find_binary_returns_none_for_nonsense_name() {
    assert_eq!(
        WhisperEngine::find_binary("definitely-not-a-real-binary-7f3a9c"),
        None
    );
    assert_eq!(
        WhisperEngine::find_binary_in("definitely-not-a-real-binary-7f3a9c", Some(""), &[]),
        None
    );
}

/// Writes an executable stub named `name` into `dir` (with `.exe` on Windows,
/// which is what `find_binary` looks for there).
fn place_executable(dir: &Path, name: &str) -> PathBuf {
    let file_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let path = dir.join(file_name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    path
}

#[test]
fn find_binary_in_finds_executable_in_extra_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let stub = place_executable(dir.path(), "whisper-server");
    let found =
        WhisperEngine::find_binary_in("whisper-server", Some(""), &[dir.path().to_path_buf()]);
    assert_eq!(found, Some(stub));
}

#[test]
fn find_binary_in_finds_executable_via_path_override() {
    let dir = tempfile::tempdir().expect("temp dir");
    let stub = place_executable(dir.path(), "whisper-cli");
    let empty = tempfile::tempdir().expect("temp dir");
    // A PATH with a miss before the hit, in the platform's own separator.
    let path_var = std::env::join_paths([empty.path(), dir.path()]).expect("join paths");
    let found = WhisperEngine::find_binary_in("whisper-cli", path_var.to_str(), &[]);
    assert_eq!(found, Some(stub));
}

#[test]
fn find_binary_in_prefers_extra_dirs_over_path() {
    let beside = tempfile::tempdir().expect("temp dir");
    let on_path = tempfile::tempdir().expect("temp dir");
    let expected = place_executable(beside.path(), "whisper-server");
    place_executable(on_path.path(), "whisper-server");
    let found = WhisperEngine::find_binary_in(
        "whisper-server",
        on_path.path().to_str(),
        &[beside.path().to_path_buf()],
    );
    assert_eq!(found, Some(expected));
}

#[cfg(unix)]
#[test]
fn find_binary_in_skips_non_executable_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("whisper-server"), b"not runnable").expect("write");
    let found =
        WhisperEngine::find_binary_in("whisper-server", Some(""), &[dir.path().to_path_buf()]);
    assert_eq!(found, None);
}

/// Captured request from the fake inference server.
struct Captured {
    path: String,
    content_type: String,
    body: Vec<u8>,
}

/// Minimal HTTP/1.1 server answering one request with `reply` and handing the
/// request back through the returned receiver.
fn fake_inference_server(reply: &'static str) -> (u16, mpsc::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        if let Some(captured) = read_request(&mut stream) {
            let _ = tx.send(captured);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    (port, rx)
}

fn read_request(stream: &mut TcpStream) -> Option<Captured> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let mut content_length = 0usize;
    let mut content_type = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        } else if lower.starts_with("content-type:") {
            content_type = line["content-type:".len()..].trim().to_string();
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some(Captured {
        path,
        content_type,
        body,
    })
}

#[test]
fn transcribe_posts_swift_multipart_form_and_returns_text_verbatim() {
    let (port, rx) = fake_inference_server(r#"{"text": " hello "}"#);
    let engine = WhisperEngine::for_test_server(port);
    assert!(engine.ready());
    assert_eq!(engine.status_text(), "ready");

    let wav = wav_data(&[0.25, -0.5, 1.0]);
    let text = engine.transcribe(&wav).expect("transcribe");
    // Whitespace is the caller's problem (clean_transcript), not the engine's.
    assert_eq!(text, " hello ");

    let req = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("request captured");
    assert_eq!(req.path, "/inference");
    assert_eq!(
        req.content_type,
        "multipart/form-data; boundary=VoiceBoundary7f3a9c"
    );
    let body = String::from_utf8_lossy(&req.body).into_owned();
    for field in [
        "Content-Disposition: form-data; name=\"temperature\"\r\n\r\n0.0\r\n",
        "Content-Disposition: form-data; name=\"temperature_inc\"\r\n\r\n0.0\r\n",
        "Content-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n",
        "Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n",
    ] {
        assert!(body.contains(field), "missing multipart field {field:?} in {body:?}");
    }
    assert!(body.ends_with("\r\n--VoiceBoundary7f3a9c--\r\n"));
    let wav_at = req
        .body
        .windows(wav.len())
        .position(|w| w == wav.as_slice());
    assert!(wav_at.is_some(), "WAV bytes must be embedded unmodified");
}

#[test]
fn transcribe_with_garbage_response_is_bad_response() {
    let (port, _rx) = fake_inference_server("<html>not json</html>");
    let engine = WhisperEngine::for_test_server(port);
    let err = engine
        .transcribe(&wav_data(&[0.0; 100]))
        .expect_err("garbage must fail");
    assert!(matches!(err, EngineError::BadResponse), "got {err:?}");
    assert_eq!(err.to_string(), "Bad response from whisper engine");
}

#[test]
fn transcribe_with_json_lacking_text_is_bad_response() {
    let (port, _rx) = fake_inference_server(r#"{"error": "no text"}"#);
    let engine = WhisperEngine::for_test_server(port);
    assert!(matches!(
        engine.transcribe(&wav_data(&[0.0; 100])),
        Err(EngineError::BadResponse)
    ));
}

#[test]
fn transcribe_against_closed_port_is_http_error() {
    // Bind-and-drop: nobody is listening on this port right now.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let engine = WhisperEngine::for_test_server(port);
    let err = engine
        .transcribe(&wav_data(&[0.0; 100]))
        .expect_err("closed port must fail");
    assert!(matches!(err, EngineError::Http(_)), "got {err:?}");
}

#[test]
fn engine_handle_is_shareable_across_threads() {
    let engine = Arc::new(WhisperEngine::new(None, TEST_PORT, 1));
    let handle = Arc::clone(&engine);
    let status = thread::spawn(move || handle.status_text())
        .join()
        .expect("join");
    assert_eq!(status, engine.status_text());
}
