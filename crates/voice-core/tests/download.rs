//! `ModelDownloader` against a tiny local HTTP server: nothing here touches
//! the network. The only remote host the real downloader talks to is Hugging
//! Face, and that URL is pinned in tests/config.rs.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use voice_core::config::{Config, ModelSpec};
use voice_core::ModelDownloader;

const SPEC: ModelSpec = ModelSpec {
    file: "ggml-tiny.en.bin",
    label: "Tiny — fastest (75 MB)",
};

/// Large enough to span several 64 KB copy buffers so progress is reported
/// more than once; small enough to keep the test instant.
const FAKE_MODEL_LEN: usize = 300 * 1024;

fn fake_model() -> Vec<u8> {
    (0..FAKE_MODEL_LEN).map(|i| (i % 251) as u8).collect()
}

struct Server {
    port: u16,
    connections: Arc<AtomicUsize>,
}

/// One-line HTTP/1.1 server. `respond(path)` returns the full raw response.
/// `delay` holds the response back so a test can observe the in-flight state
/// deterministically (a 300 KB localhost transfer would otherwise finish
/// before the assertion runs).
fn spawn_server(delay: Duration, respond: impl Fn(&str) -> Vec<u8> + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            counter.fetch_add(1, Ordering::SeqCst);
            let Some(path) = read_request_path(&mut stream) else {
                continue;
            };
            thread::sleep(delay);
            let _ = stream.write_all(&respond(&path));
            let _ = stream.flush();
        }
    });
    Server { port, connections }
}

fn read_request_path(stream: &mut TcpStream) -> Option<String> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some(path)
}

fn response(status: &str, body: &[u8], declared_len: Option<usize>) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        declared_len.unwrap_or(body.len())
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

struct Harness {
    dir: tempfile::TempDir,
    downloader: ModelDownloader,
    progress: Arc<Mutex<Vec<f64>>>,
    finished: mpsc::Receiver<(String, Option<PathBuf>)>,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().expect("temp dir");
    // A nested, not-yet-existing directory: the downloader must create it.
    let downloader = ModelDownloader::with_directory(dir.path().join("models"));
    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&progress);
    downloader.set_on_progress(Box::new(move |_, p| {
        sink.lock().expect("progress lock").push(p)
    }));
    let (tx, finished) = mpsc::channel();
    downloader.set_on_finished(Box::new(move |file, path| {
        let _ = tx.send((file.to_string(), path));
    }));
    Harness {
        dir,
        downloader,
        progress,
        finished,
    }
}

fn wait_finished(h: &Harness) -> (String, Option<PathBuf>) {
    h.finished
        .recv_timeout(Duration::from_secs(30))
        .expect("on_finished should fire")
}

#[test]
fn successful_download_streams_to_part_then_renames() {
    let body = fake_model();
    let expected = body.clone();
    let server = spawn_server(Duration::from_millis(300), move |path| {
        assert_eq!(path, "/ggml-tiny.en.bin");
        response("200 OK", &body, None)
    });
    let h = harness();
    let url = format!("http://127.0.0.1:{}/ggml-tiny.en.bin", server.port);

    h.downloader.download_from(&SPEC, &url);
    assert!(h.downloader.is_downloading(SPEC.file));
    assert_eq!(h.downloader.progress(SPEC.file), Some(0.0));
    // A second request for the same file while one is in flight is a no-op:
    // the server must see exactly one connection.
    h.downloader.download_from(&SPEC, &url);

    let (file, path) = wait_finished(&h);
    assert_eq!(file, SPEC.file);
    let dest = path.expect("download should succeed");
    assert_eq!(dest, h.dir.path().join("models").join(SPEC.file));
    assert_eq!(std::fs::read(&dest).expect("read model"), expected);
    assert!(
        !dest.with_extension("bin.part").exists(),
        ".part must be renamed away"
    );
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);

    assert!(!h.downloader.is_downloading(SPEC.file));
    assert_eq!(h.downloader.progress(SPEC.file), None);
    let progress = h.progress.lock().expect("progress lock").clone();
    assert!(
        !progress.is_empty(),
        "progress should be reported when Content-Length is known"
    );
    assert!(
        progress.windows(2).all(|w| w[0] <= w[1]),
        "progress must be monotonic: {progress:?}"
    );
    assert!(progress.iter().all(|p| (0.0..=1.0).contains(p)));
    assert_eq!(progress.last().copied(), Some(1.0));
}

#[test]
fn existing_destination_is_replaced() {
    let body = fake_model();
    let expected = body.clone();
    let server = spawn_server(Duration::ZERO, move |_| response("200 OK", &body, None));
    let h = harness();
    let dest = h.dir.path().join("models").join(SPEC.file);
    std::fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    std::fs::write(&dest, b"stale model").expect("write stale");

    h.downloader
        .download_from(&SPEC, &format!("http://127.0.0.1:{}/x", server.port));
    let (_, path) = wait_finished(&h);
    assert_eq!(path.as_deref(), Some(dest.as_path()));
    assert_eq!(std::fs::read(&dest).expect("read"), expected);
}

#[test]
fn http_404_fails_without_leaving_files() {
    let server = spawn_server(Duration::ZERO, |_| {
        response("404 Not Found", b"missing", None)
    });
    let h = harness();
    h.downloader
        .download_from(&SPEC, &format!("http://127.0.0.1:{}/nope", server.port));

    let (file, path) = wait_finished(&h);
    assert_eq!(file, SPEC.file);
    assert!(path.is_none(), "a non-200 status is a failure");
    let models = h.dir.path().join("models");
    assert!(!models.join(SPEC.file).exists());
    assert!(!models.join("ggml-tiny.en.bin.part").exists());
    assert!(!h.downloader.is_downloading(SPEC.file));
    assert!(h.progress.lock().expect("progress lock").is_empty());
}

#[test]
fn truncated_body_fails_and_removes_part_file() {
    // Declares more bytes than it sends, then closes: the download must not
    // be promoted to a "complete" model.
    let server = spawn_server(Duration::ZERO, |_| {
        response("200 OK", &fake_model()[..1024], Some(FAKE_MODEL_LEN))
    });
    let h = harness();
    h.downloader
        .download_from(&SPEC, &format!("http://127.0.0.1:{}/short", server.port));

    let (_, path) = wait_finished(&h);
    assert!(path.is_none());
    let models = h.dir.path().join("models");
    assert!(!models.join(SPEC.file).exists());
    assert!(!models.join("ggml-tiny.en.bin.part").exists());
}

/// A server that sends headers plus a few body bytes and then hangs without
/// closing: the body-receive ceiling must turn that into a failure, clear the
/// in-flight state and fire `on_finished(file, None)` so the user can retry.
/// Without it `is_downloading` would stay true for the life of the process.
#[test]
fn stalled_body_times_out_and_allows_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        if read_request_path(&mut stream).is_none() {
            return;
        }
        // Full headers, 1 KB of a declared 300 KB body, then silence: the
        // socket is held open until the test finishes so the client cannot
        // see EOF and must rely on its own timeout.
        let _ = stream.write_all(&response(
            "200 OK",
            &fake_model()[..1024],
            Some(FAKE_MODEL_LEN),
        ));
        let _ = stream.flush();
        let _ = release_rx.recv();
    });

    let dir = tempfile::tempdir().expect("temp dir");
    let downloader = ModelDownloader::with_directory_and_body_timeout(
        dir.path().join("models"),
        Duration::from_secs(2),
    );
    let (tx, finished) = mpsc::channel();
    downloader.set_on_finished(Box::new(move |file, path| {
        let _ = tx.send((file.to_string(), path));
    }));
    downloader.download_from(&SPEC, &format!("http://127.0.0.1:{port}/stall"));
    assert!(downloader.is_downloading(SPEC.file));

    let (file, path) = finished
        .recv_timeout(Duration::from_secs(30))
        .expect("a stalled body must still report on_finished");
    assert_eq!(file, SPEC.file);
    assert!(path.is_none(), "a timed-out transfer is a failure");
    assert!(
        !downloader.is_downloading(SPEC.file),
        "progress entry must be cleared so a retry is not a no-op"
    );
    assert_eq!(downloader.progress(SPEC.file), None);
    let models = dir.path().join("models");
    assert!(!models.join(SPEC.file).exists());
    assert!(!models.join("ggml-tiny.en.bin.part").exists());
    drop(release_tx);
}

#[test]
fn unreachable_server_fails() {
    // A port nobody listens on: bind-and-drop guarantees it was free just now.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let h = harness();
    h.downloader
        .download_from(&SPEC, &format!("http://127.0.0.1:{port}/x"));
    let (_, path) = wait_finished(&h);
    assert!(path.is_none());
    assert!(!h.downloader.is_downloading(SPEC.file));
}

#[test]
fn default_downloader_targets_first_models_dir() {
    // Construction only — never starts a download against the real directory.
    let d = ModelDownloader::new();
    assert_eq!(d.directory(), &Config::models_dirs()[0]);
    assert!(!d.is_downloading(SPEC.file));
    assert_eq!(d.progress(SPEC.file), None);
}
