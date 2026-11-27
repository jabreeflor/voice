//! One running Voice per user (issue #22).
//!
//! Two copies would both hook the talk key (every dictation pasted twice) and
//! race for whisper-server's port, so `main` takes this guard before Tauri,
//! the hotkey hook or the engine start.
//!
//! - Identity is a lock file in the per-user data directory (`Store::dir()`,
//!   the same `~/Library/Application Support/Voice` every version uses), not
//!   the bundle path or version, so two installed copies exclude each other.
//!   The lock is an OS advisory lock (`File::try_lock`: `flock` /
//!   `LockFileEx`); the OS drops it when the process dies, so a crash never
//!   leaves a stale guard.
//! - The holder listens on a loopback port and writes `port token` to
//!   `voice.instance` (a separate file: Windows locks make the locked file
//!   itself unreadable). A second launch sends `<token> show`; the holder
//!   shows its window (`App::reopen`) and answers `ok`, and the newcomer
//!   exits. The token keeps other local users from poking the window.
//! - Relaunches (Accessibility `relaunch_self`, updater `request_restart`)
//!   start the new copy while the old one is still exiting. The old copy
//!   answers `closing` from `ExitRequested` on, and releases the lock at the
//!   end of `RunEvent::Exit` (after whisper-server is stopped), so the new
//!   copy waits for the lock instead of quitting. The wait is bounded.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const LOCK_FILE: &str = "voice.lock";
const INFO_FILE: &str = "voice.instance";
/// How long a launch waits for an exiting copy to let go. Covers
/// whisper-server shutdown plus the 0.7 s relaunch delay with room to spare.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(10);
const RETRY_EVERY: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// What a launch should do next.
pub enum Launch {
    /// This process is the one Voice: keep the guard alive for its lifetime.
    Primary(InstanceGuard),
    /// Another copy is running and was asked to show itself: exit quietly.
    Signalled,
    /// Another copy holds the lock but never answered or never let go.
    TimedOut,
}

type ShowFn = Box<dyn Fn() + Send + Sync>;

struct Shared {
    closing: AtomicBool,
    /// A `show` that arrived before `on_show` was set (the app was still in
    /// setup); delivered as soon as the callback is installed.
    pending_show: AtomicBool,
    on_show: Mutex<Option<ShowFn>>,
}

impl Shared {
    fn request_show(&self) {
        self.pending_show.store(true, Ordering::SeqCst);
        self.deliver_pending();
    }

    fn deliver_pending(&self) {
        let slot = self.on_show.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(f) = slot.as_ref() {
            if self.pending_show.swap(false, Ordering::SeqCst) {
                f();
            }
        }
    }
}

/// Held by the running Voice. Dropping it (or `release`) frees the lock.
pub struct InstanceGuard {
    dir: PathBuf,
    lock: Mutex<Option<File>>,
    shared: Arc<Shared>,
}

impl InstanceGuard {
    /// Called (on the listener thread) for every valid `show` request.
    pub fn on_show(&self, f: impl Fn() + Send + Sync + 'static) {
        *self
            .shared
            .on_show
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Box::new(f));
        self.shared.deliver_pending();
    }

    /// From here on a new launch waits for the lock instead of exiting: this
    /// copy is on its way out (quit, relaunch or restart to update).
    pub fn mark_closing(&self) {
        self.shared.closing.store(true, Ordering::SeqCst);
    }

    /// Frees the lock before the process actually ends, so a relaunched copy
    /// can take over as soon as this one has stopped its engine.
    pub fn release(&self) {
        self.mark_closing();
        let file = self
            .lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(file) = file {
            // Remove our address first: once unlocked, the successor writes
            // its own and must not have it deleted from under it.
            let _ = fs::remove_file(self.dir.join(INFO_FILE));
            drop(file);
        }
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Takes the per-user guard in `dir`, or asks the running copy to show
/// itself. Retries for up to `wait` while the holder is exiting or not yet
/// listening. `Err` only for I/O on `dir` itself (the caller decides
/// whether to run unguarded).
pub fn acquire_in(dir: &Path, wait: Duration) -> io::Result<Launch> {
    fs::create_dir_all(dir)?;
    let deadline = Instant::now() + wait;
    loop {
        // Never truncate: the file carries no data, and truncating a file
        // another process has locked fails on Windows.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(LOCK_FILE))?;
        match file.try_lock() {
            Ok(()) => return InstanceGuard::serve(dir, file).map(Launch::Primary),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => return Err(e),
        }
        drop(file);
        if signal_show(dir) == Reply::Shown {
            return Ok(Launch::Signalled);
        }
        if Instant::now() >= deadline {
            return Ok(Launch::TimedOut);
        }
        std::thread::sleep(RETRY_EVERY);
    }
}

impl InstanceGuard {
    fn serve(dir: &Path, lock: File) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = new_token();
        write_info(dir, port, &token)?;
        let shared = Arc::new(Shared {
            closing: AtomicBool::new(false),
            pending_show: AtomicBool::new(false),
            on_show: Mutex::new(None),
        });
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("voice-single-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    // One short line per launch; a bad peer only costs its
                    // own read timeout.
                    let _ = answer(stream, &token, &worker);
                }
            })?;
        Ok(Self {
            dir: dir.to_path_buf(),
            lock: Mutex::new(Some(lock)),
            shared,
        })
    }
}

fn answer(stream: TcpStream, token: &str, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = String::new();
    BufReader::new(&stream).take_line(&mut line)?;
    if line.trim_end() != format!("{token} show") {
        return Ok(());
    }
    let reply = if shared.closing.load(Ordering::SeqCst) {
        "closing\n"
    } else {
        shared.request_show();
        "ok\n"
    };
    (&stream).write_all(reply.as_bytes())
}

#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Shown,
    /// Holder is exiting, not listening yet, or gone: try the lock again.
    NotYet,
}

fn signal_show(dir: &Path) -> Reply {
    let Some((port, token)) = read_info(dir) else {
        return Reply::NotYet;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let exchange = || -> io::Result<String> {
        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.write_all(format!("{token} show\n").as_bytes())?;
        let mut line = String::new();
        BufReader::new(&stream).take_line(&mut line)?;
        Ok(line)
    };
    match exchange() {
        Ok(line) if line.trim_end() == "ok" => Reply::Shown,
        _ => Reply::NotYet,
    }
}

/// `read_line` capped at 256 bytes, so a peer cannot make us buffer forever.
trait TakeLine {
    fn take_line(self, buf: &mut String) -> io::Result<usize>;
}

impl<R: BufRead> TakeLine for R {
    fn take_line(self, buf: &mut String) -> io::Result<usize> {
        io::Read::take(self, 256).read_line(buf)
    }
}

fn write_info(dir: &Path, port: u16, token: &str) -> io::Result<()> {
    // Write-then-rename so a launch never reads half a file.
    let tmp = dir.join(format!("{INFO_FILE}.{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // The token is the only thing keeping other local users out.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut f = options.open(&tmp)?;
    writeln!(f, "{port} {token}")?;
    drop(f);
    fs::rename(&tmp, dir.join(INFO_FILE))
}

fn read_info(dir: &Path) -> Option<(u16, String)> {
    let text = fs::read_to_string(dir.join(INFO_FILE)).ok()?;
    let (port, token) = text.trim().split_once(' ')?;
    Some((port.parse().ok()?, token.to_string()))
}

/// 128 bits from two independently seeded std hashers (seeded from the OS
/// RNG): enough for a same-machine shared secret without a new dependency.
fn new_token() -> String {
    let salt = (std::process::id(), Instant::now());
    let a = RandomState::new().hash_one(salt);
    let b = RandomState::new().hash_one(salt);
    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    const SHORT: Duration = Duration::from_millis(300);

    fn primary(dir: &Path) -> InstanceGuard {
        match acquire_in(dir, SHORT).unwrap() {
            Launch::Primary(g) => g,
            _ => panic!("expected to become the primary instance"),
        }
    }

    fn counting(guard: &InstanceGuard) -> Arc<AtomicUsize> {
        let shows = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&shows);
        guard.on_show(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        shows
    }

    #[test]
    fn second_launch_shows_the_running_copy_and_does_not_start() {
        let dir = tempfile::tempdir().unwrap();
        let first = primary(dir.path());
        let shows = counting(&first);
        assert!(matches!(
            acquire_in(dir.path(), SHORT).unwrap(),
            Launch::Signalled
        ));
        assert_eq!(shows.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn repeated_launches_each_show_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let first = primary(dir.path());
        let shows = counting(&first);
        for _ in 0..5 {
            assert!(matches!(
                acquire_in(dir.path(), SHORT).unwrap(),
                Launch::Signalled
            ));
        }
        assert_eq!(shows.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn show_before_the_app_finished_setup_is_delivered_later() {
        let dir = tempfile::tempdir().unwrap();
        let first = primary(dir.path());
        assert!(matches!(
            acquire_in(dir.path(), SHORT).unwrap(),
            Launch::Signalled
        ));
        let shows = counting(&first);
        assert_eq!(shows.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn simultaneous_launches_elect_exactly_one_primary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let launches: Vec<_> = (0..8)
            .map(|_| {
                let (path, barrier) = (path.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    acquire_in(&path, Duration::from_secs(5)).unwrap()
                })
            })
            .collect();
        let results: Vec<Launch> = launches.into_iter().map(|t| t.join().unwrap()).collect();
        let primaries = results
            .iter()
            .filter(|r| matches!(r, Launch::Primary(_)))
            .count();
        let signalled = results
            .iter()
            .filter(|r| matches!(r, Launch::Signalled))
            .count();
        assert_eq!((primaries, signalled), (1, 7));
    }

    #[test]
    fn relaunch_waits_for_the_exiting_copy_instead_of_quitting() {
        let dir = tempfile::tempdir().unwrap();
        let old = primary(dir.path());
        let shows = counting(&old);
        // Restart to Update / Accessibility relaunch: the old copy is exiting
        // when the new one starts, and lets go a moment later.
        old.mark_closing();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            old.release();
        });
        let started = Instant::now();
        let new = acquire_in(dir.path(), Duration::from_secs(5)).unwrap();
        assert!(matches!(new, Launch::Primary(_)));
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert_eq!(
            shows.load(Ordering::SeqCst),
            0,
            "a closing copy must not show"
        );
        releaser.join().unwrap();
    }

    #[test]
    fn a_holder_that_never_lets_go_times_out_rather_than_doubling_up() {
        let dir = tempfile::tempdir().unwrap();
        let stuck = primary(dir.path());
        stuck.mark_closing();
        assert!(matches!(
            acquire_in(dir.path(), SHORT).unwrap(),
            Launch::TimedOut
        ));
    }

    #[test]
    fn release_lets_the_next_launch_become_primary() {
        let dir = tempfile::tempdir().unwrap();
        let first = primary(dir.path());
        first.release();
        assert!(!dir.path().join(INFO_FILE).exists());
        let _second = primary(dir.path());
        drop(first); // a second release must not touch the new holder's file
        assert!(dir.path().join(INFO_FILE).exists());
    }

    #[test]
    fn wrong_token_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let first = primary(dir.path());
        let shows = counting(&first);
        let (port, _) = read_info(dir.path()).unwrap();
        write_info(dir.path(), port, "not-the-token").unwrap();
        assert_eq!(signal_show(dir.path()), Reply::NotYet);
        assert_eq!(shows.load(Ordering::SeqCst), 0);
    }

    /// Real processes, from two different paths (as two installed versions
    /// would be): the identity is the per-user directory, not the binary.
    /// The test re-runs its own executable as the "launches".
    #[test]
    fn separate_processes_from_two_install_paths_share_one_guard() {
        const ROLE: &str = "VOICE_SINGLE_INSTANCE_ROLE";
        const DIR: &str = "VOICE_SINGLE_INSTANCE_DIR";
        let name =
            "single_instance::tests::separate_processes_from_two_install_paths_share_one_guard";
        if let (Ok(role), Ok(dir)) = (std::env::var(ROLE), std::env::var(DIR)) {
            match acquire_in(Path::new(&dir), Duration::from_secs(5)).unwrap() {
                Launch::Primary(guard) => {
                    let shows = counting(&guard);
                    println!("RESULT primary");
                    if role == "hold" {
                        std::thread::sleep(Duration::from_secs(3));
                    }
                    println!("SHOWS {}", shows.load(Ordering::SeqCst));
                }
                Launch::Signalled => println!("RESULT signalled"),
                Launch::TimedOut => println!("RESULT timedout"),
            }
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let installs = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let copies: Vec<PathBuf> = ["v1/voice", "v2/Voice Beta/voice"]
            .iter()
            .map(|rel| {
                let to = installs.path().join(rel);
                fs::create_dir_all(to.parent().unwrap()).unwrap();
                fs::copy(&exe, &to).unwrap();
                to
            })
            .collect();
        let launch = |exe: &Path, role: &str| {
            std::process::Command::new(exe)
                .args(["--exact", name, "--nocapture", "--test-threads=1"])
                .env(ROLE, role)
                .env(DIR, dir.path())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        };
        let result = |child: std::process::Child| {
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).into_owned()
        };

        let holder = launch(&copies[0], "hold");
        let started = Instant::now();
        while read_info(dir.path()).is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "holder never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // Repeated, then simultaneous, launches from the other install.
        for _ in 0..2 {
            assert!(result(launch(&copies[1], "probe")).contains("RESULT signalled"));
        }
        let burst: Vec<_> = (0..4).map(|i| launch(&copies[i % 2], "probe")).collect();
        for child in burst {
            assert!(result(child).contains("RESULT signalled"));
        }
        let held = result(holder);
        assert!(held.contains("RESULT primary"), "{held}");
        assert!(held.contains("SHOWS 6"), "{held}");
        // Holder gone (lock freed by the OS): the next launch starts normally.
        assert!(result(launch(&copies[1], "probe")).contains("RESULT primary"));
    }

    #[test]
    fn tokens_differ_between_instances() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
