//! macOS only: the promised-pasteboard item behind paste-landing detection
//! (src/platform/macos_pasteboard.rs), run for real against AppKit.
//!
//! `harness = false` because the provider callback is delivered on the
//! **main thread's run loop** (exactly as in the app, where Tauri runs it):
//! libtest keeps the main thread blocked, so a reader would hang. Here
//! `main` runs the CFRunLoop and the checks run on a worker. A private named
//! pasteboard is used so the developer's clipboard is never touched, and the
//! "other app" is a separate `osascript` process reading that pasteboard.
//!
//! The module is compiled in by `#[path]`: voice-app is a binary crate, so
//! an integration test cannot import it.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/platform/macos_pasteboard.rs"]
mod macos_pasteboard;

fn main() {
    #[cfg(target_os = "macos")]
    checks::run();
}

#[cfg(target_os = "macos")]
mod checks {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::macos_pasteboard::PromisedText;

    /// What the app waits for (paste::PASTE_CONFIRM_TIMEOUT).
    const CONFIRM: Duration = Duration::from_millis(1200);
    /// A cold `osascript` start can take a while on a CI runner.
    const READER_TIMEOUT: Duration = Duration::from_secs(20);

    pub fn run() {
        std::thread::spawn(|| {
            let outcome = std::panic::catch_unwind(|| {
                read_by_another_process_counts_as_requested();
                unread_promise_times_out();
                a_later_write_means_the_promise_is_gone();
            });
            let failed = outcome.is_err();
            if failed {
                println!("\ntest result: FAILED. macos_pasteboard");
            } else {
                println!("\ntest result: ok. 3 passed; 0 failed (macos_pasteboard)");
            }
            std::process::exit(i32::from(failed));
        });
        // SAFETY: plain C call; runs the main thread's run loop until the
        // worker exits the process.
        unsafe { core_foundation::runloop::CFRunLoopRun() };
    }

    fn unique_name(tag: &str) -> String {
        format!("com.local.voice.test.{tag}.{}", std::process::id())
    }

    /// Named pasteboards live in the pboard server until released.
    fn release(name: &str) {
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::NSString;
        let pasteboard = NSPasteboard::pasteboardWithName(&NSString::from_str(name));
        // SAFETY: `releaseGlobally` takes no arguments and returns void; it is
        // not in objc2-app-kit 0.3's generated bindings.
        unsafe {
            let _: () = objc2::msg_send![&*pasteboard, releaseGlobally];
        }
    }

    /// Another process reading the named pasteboard, as a target app would
    /// read the general one on ⌘V.
    fn read_with_osascript(name: &str) -> Result<String, String> {
        let script = format!(
            "ObjC.import('AppKit'); \
             var s = $.NSPasteboard.pasteboardWithName('{name}').stringForType($.NSPasteboardTypeString); \
             s.isNil() ? '<nil>' : s.js"
        );
        let mut child = Command::new("/usr/bin/osascript")
            .args(["-l", "JavaScript", "-e", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        loop {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
            if start.elapsed() > READER_TIMEOUT {
                let _ = child.kill();
                return Err("osascript reader hung".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn read_by_another_process_counts_as_requested() {
        let name = unique_name("read");
        let text = "Ask Jabree to ship the Tauri build.";
        let promise = PromisedText::place_on(Some(&name), text).expect("promise placed");
        assert!(promise.still_on_pasteboard());
        let read = read_with_osascript(&name).expect("reader ran");
        assert_eq!(read, text, "the other process must receive the text");
        assert!(
            promise.wait_requested(CONFIRM),
            "provider was not called for a read"
        );
        drop(promise);
        release(&name);
        println!("test read_by_another_process_counts_as_requested ... ok");
    }

    fn unread_promise_times_out() {
        let name = unique_name("unread");
        let promise = PromisedText::place_on(Some(&name), "nobody reads this").expect("placed");
        let start = Instant::now();
        assert!(
            !promise.wait_requested(CONFIRM),
            "provider called with no reader"
        );
        let waited = start.elapsed();
        assert!(
            waited >= CONFIRM && waited < CONFIRM + Duration::from_millis(500),
            "waited {waited:?}"
        );
        drop(promise);
        release(&name);
        println!("test unread_promise_times_out ... ok ({waited:.2?})");
    }

    fn a_later_write_means_the_promise_is_gone() {
        let name = unique_name("replaced");
        let first = PromisedText::place_on(Some(&name), "first").expect("placed");
        let _second = PromisedText::place_on(Some(&name), "second").expect("placed");
        assert!(
            !first.still_on_pasteboard(),
            "a newer write must not be overwritten"
        );
        release(&name);
        println!("test a_later_write_means_the_promise_is_gone ... ok");
    }
}
