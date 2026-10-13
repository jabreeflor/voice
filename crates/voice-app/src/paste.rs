//! Paste into the frontmost app. Reference: `pasteText` in
//! Sources/VoiceCore/core.swift.
//!
//! Puts `text` on the clipboard, synthesizes ⌘V (macOS) / Ctrl+V (Windows,
//! Linux), then restores the previous clipboard text 600 ms later — long
//! enough for the target app to have consumed the paste.
//!
//! On macOS the text goes on the clipboard as a *promise*
//! (`platform::macos_pasteboard`), so the app learns whether anything read
//! it. If nothing did within `PASTE_CONFIRM_TIMEOUT` the paste is reported
//! as [`PasteOutcome::NotPasted`]: the transcript is left on the clipboard
//! (as a plain string) instead of the old contents being restored, and the
//! caller tells the user. Windows and Linux cannot tell, so they behave as
//! before. The decision itself is the platform-neutral [`paste_outcome`].
//!
//! On macOS the synthesized keystroke goes through CGEvent, which requires
//! the app to be trusted in System Settings → Privacy & Security →
//! Accessibility (the same grant the hotkey hook needs). Without it
//! `Enigo::new` fails and the text stays on the clipboard for a manual ⌘V.
//! Nothing here panics: every failure is logged and the function returns.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Swift restores after 0.6 s; shorter and slow apps paste the old contents.
const RESTORE_DELAY: Duration = Duration::from_millis(600);

/// macOS: how long after ⌘V an app has to read the promised text before the
/// paste counts as failed. Apps read on the keystroke itself (well under
/// 100 ms when the main thread is idle); the margin covers a busy target.
#[cfg(target_os = "macos")]
const PASTE_CONFIRM_TIMEOUT: Duration = Duration::from_millis(1200);

/// The handle that tells whether the paste was read. Only macOS has one;
/// elsewhere the type is uninhabited, so `Option<Watch>` is always `None`.
#[cfg(target_os = "macos")]
type Watch = crate::platform::macos_pasteboard::PromisedText;
#[cfg(not(target_os = "macos"))]
type Watch = std::convert::Infallible;

/// What happened to one `paste_text` call, as far as the OS can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
    /// The keystroke went out and either an app read the text (macOS) or
    /// the platform cannot tell (Windows, Linux, or the macOS promise could
    /// not be placed). The previous clipboard is restored.
    Pasted,
    /// macOS: ⌘V went out but no app read the text in time. The transcript
    /// stays on the clipboard for a manual paste.
    NotPasted,
    /// No keystroke reached the system (no Accessibility grant, no X
    /// connection). The transcript stays on the clipboard; the status line
    /// already explains the missing permission, so no extra notice.
    KeystrokeFailed,
    /// The clipboard could not be written; nothing was pasted.
    ClipboardFailed,
}

/// What the clipboard should hold once the attempt is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterPaste {
    RestorePrevious,
    KeepTranscript,
}

/// The platform-neutral decision. `data_requested` is `Some(read?)` when
/// the platform could watch the clipboard being read (the macOS promise),
/// `None` when it cannot; "cannot tell" is treated as success, which is
/// exactly the pre-detection behaviour.
pub fn paste_outcome(keystroke_sent: bool, data_requested: Option<bool>) -> PasteOutcome {
    match (keystroke_sent, data_requested) {
        (false, _) => PasteOutcome::KeystrokeFailed,
        (true, Some(false)) => PasteOutcome::NotPasted,
        (true, Some(true) | None) => PasteOutcome::Pasted,
    }
}

impl PasteOutcome {
    pub fn clipboard_after(self) -> AfterPaste {
        match self {
            PasteOutcome::Pasted => AfterPaste::RestorePrevious,
            _ => AfterPaste::KeepTranscript,
        }
    }

    /// Only a detected miss gets the "Not pasted" notice.
    pub fn notify_user(self) -> bool {
        self == PasteOutcome::NotPasted
    }
}

/// One process-lifetime clipboard handle, the way arboard is meant to be
/// used. On X11 the selection is served by a window owned by the handle:
/// dropping a per-call handle right after `set_text` tears that window down
/// (after a ≤100 ms handover that only works when a clipboard manager is
/// running), so the target app can find nothing to paste. macOS and Windows
/// copy the data into the OS on write, so a long-lived handle costs nothing
/// there. `None` until the first successful `Clipboard::new`, and again after
/// a failure, so a transient error (no display yet) is retried next time.
static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

/// Runs `f` on the shared handle, creating it on first use. `None` when the
/// clipboard cannot be opened at all (already logged).
fn with_clipboard<R>(f: impl FnOnce(&mut Clipboard) -> R) -> Option<R> {
    let mut guard = match CLIPBOARD.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        match Clipboard::new() {
            Ok(c) => *guard = Some(c),
            Err(e) => {
                log::error!("clipboard unavailable: {e}");
                return None;
            }
        }
    }
    guard.as_mut().map(f)
}

/// Blocking: call from a worker thread, never the Tauri main thread. The
/// clipboard read can wait up to 4 s for the X11 selection owner (arboard's
/// long timeout) and enigo sleeps ~80 ms on macOS while the paste events
/// drain, all while holding the process-wide `CLIPBOARD` mutex. On macOS it
/// then waits up to `PASTE_CONFIRM_TIMEOUT` for the paste to be read, and
/// that read is served on the main thread, so calling this from the main
/// thread would also make every macOS paste look failed.
pub fn paste_text(text: &str) -> PasteOutcome {
    // Err means "nothing textual on the clipboard" (empty, image, ...);
    // Swift's `string(forType:)` returns nil there and skips the restore.
    let written = with_clipboard(|clipboard| {
        let saved = clipboard.get_text().ok();
        write_for_paste(clipboard, text).map(|watch| (saved, watch))
    });
    let (saved, watch) = match written {
        Some(Ok(state)) => state,
        Some(Err(e)) => {
            log::error!("clipboard write failed: {e}");
            return PasteOutcome::ClipboardFailed;
        }
        None => return PasteOutcome::ClipboardFailed,
    };
    let sent_at = Instant::now();
    let sent = send_paste_shortcut();
    let outcome = paste_outcome(sent, wait_until_read(watch.as_ref(), sent));

    match outcome.clipboard_after() {
        AfterPaste::KeepTranscript => {
            // Restoring now would wipe the dictation before the user can
            // paste it by hand; keep it on the clipboard instead of losing it.
            if outcome == PasteOutcome::KeystrokeFailed {
                log::warn!(
                    "paste keystroke not sent; transcript left on the clipboard for a manual paste"
                );
            } else {
                log::warn!("no app read the pasted text; transcript left on the clipboard");
            }
            settle_promise(watch, text);
        }
        // Plain write and nothing to restore: the transcript simply stays.
        AfterPaste::RestorePrevious if saved.is_none() && watch.is_none() => {}
        AfterPaste::RestorePrevious => {
            // The restore runs on the shared handle, which stays alive for
            // the whole process, so the pasted text is still being served
            // while the target app reads it — whether or not there is
            // anything to restore afterwards. Never sooner than
            // RESTORE_DELAY after the keystroke, as before detection.
            let text = text.to_string();
            thread::spawn(move || {
                thread::sleep(RESTORE_DELAY.saturating_sub(sent_at.elapsed()));
                match saved {
                    Some(saved) => {
                        // The promise (if any) lives until after the restore
                        // has replaced it on the pasteboard.
                        let _promise = watch;
                        if let Some(Err(e)) = with_clipboard(|clipboard| clipboard.set_text(saved))
                        {
                            log::warn!("clipboard restore failed: {e}");
                        }
                    }
                    // Nothing to restore: the transcript stays, as before.
                    None => settle_promise(watch, &text),
                }
            });
        }
    }
    outcome
}

/// Writes `text` for the paste. macOS: as a promise whose reads can be
/// watched, falling back to a plain write if AppKit refuses the promise.
/// Elsewhere: a plain write, and nothing to watch.
fn write_for_paste(clipboard: &mut Clipboard, text: &str) -> Result<Option<Watch>, arboard::Error> {
    #[cfg(target_os = "macos")]
    {
        if let Some(promise) = Watch::place(text) {
            return Ok(Some(promise));
        }
        log::warn!("promised pasteboard item refused; paste landing cannot be checked");
    }
    clipboard.set_text(text).map(|()| None)
}

/// `Some(read?)` when there is a promise to watch and ⌘V went out.
fn wait_until_read(watch: Option<&Watch>, sent: bool) -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        watch
            .filter(|_| sent)
            .map(|promise| promise.wait_requested(PASTE_CONFIRM_TIMEOUT))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (watch, sent);
        None
    }
}

/// When the transcript is to stay on the clipboard and it is still there as
/// our promise, swaps the promise for a plain string so it survives this
/// app quitting; the provider is released only after that. Does nothing if
/// the user has copied something else since (their copy wins), or without
/// a promise.
fn settle_promise(watch: Option<Watch>, text: &str) {
    #[cfg(target_os = "macos")]
    if let Some(promise) = watch {
        if promise.still_on_pasteboard() {
            if let Some(Err(e)) = with_clipboard(|clipboard| clipboard.set_text(text)) {
                log::warn!("could not keep the transcript on the clipboard: {e}");
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (watch, text);
}

/// Plain clipboard write (menu "Copy Last Dictation", ledger row click).
/// Uses the shared handle so the text keeps being served on X11 and the
/// caller (a Tauri IPC thread) never blocks on arboard's drop-time handover.
/// Still blocking: `set_text` may wait on the X11 selection, so keep it off
/// the Tauri main thread too.
pub fn copy_text(text: &str) {
    if let Some(Err(e)) = with_clipboard(|clipboard| clipboard.set_text(text)) {
        log::error!("clipboard write failed: {e}");
    }
}

/// ⌘V on macOS, Ctrl+V elsewhere, as an explicit down/up sequence so the
/// modifier is held while V is pressed and released. `false` when no
/// keystroke reached the system (no Accessibility grant, no X connection,
/// or a failure mid-sequence), so the caller knows the paste did not happen.
fn send_paste_shortcut() -> bool {
    let settings = Settings {
        // The app drives the Accessibility prompt itself (onboarding /
        // status); enigo must not pop a second system dialog mid-paste.
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = match Enigo::new(&settings) {
        Ok(e) => e,
        Err(e) => {
            log::error!("cannot synthesize paste keystroke: {e}");
            return false;
        }
    };
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    // Layout-independent V, like Swift's `virtualKey: 9`. `Key::Unicode('v')`
    // is resolved through the active input source: with a non-Latin layout
    // (Cyrillic, Greek, Hebrew, kana, ...) enigo finds no keycode on macOS
    // and falls through to keycode 0 = kVK_ANSI_A, turning the paste into a
    // destructive ⌘A; on Windows `VkKeyScanExW` fails and enigo silently
    // types a literal "v" instead. `Key::Other` is passed straight through as
    // a raw keycode (macOS) / virtual key (Windows). Linux keeps the keysym
    // lookup, which is layout-correct there.
    let v_key = if cfg!(target_os = "macos") {
        Key::Other(9) // kVK_ANSI_V
    } else if cfg!(target_os = "windows") {
        Key::Other(0x56) // VK_V
    } else {
        Key::Unicode('v')
    };
    let sequence = [
        (modifier, Direction::Press),
        (v_key, Direction::Press),
        (v_key, Direction::Release),
        (modifier, Direction::Release),
    ];
    for (key, direction) in sequence {
        if let Err(e) = enigo.key(key, direction) {
            log::error!("paste keystroke failed at {key:?} {direction:?}: {e}");
            // `release_keys_when_dropped` (default true) lets go of any
            // modifier still held when `enigo` drops here.
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_paste_restores_the_previous_clipboard() {
        let outcome = paste_outcome(true, Some(true));
        assert_eq!(outcome, PasteOutcome::Pasted);
        assert_eq!(outcome.clipboard_after(), AfterPaste::RestorePrevious);
        assert!(!outcome.notify_user());
    }

    #[test]
    fn unread_paste_keeps_the_transcript_and_tells_the_user() {
        let outcome = paste_outcome(true, Some(false));
        assert_eq!(outcome, PasteOutcome::NotPasted);
        assert_eq!(outcome.clipboard_after(), AfterPaste::KeepTranscript);
        assert!(outcome.notify_user());
    }

    /// Windows and Linux (and a refused macOS promise) cannot watch the
    /// read: they must behave exactly as before detection existed.
    #[test]
    fn unknown_read_state_behaves_like_before() {
        let outcome = paste_outcome(true, None);
        assert_eq!(outcome, PasteOutcome::Pasted);
        assert_eq!(outcome.clipboard_after(), AfterPaste::RestorePrevious);
        assert!(!outcome.notify_user());
    }

    #[test]
    fn unsent_keystroke_keeps_the_transcript_without_a_notice() {
        for requested in [None, Some(false), Some(true)] {
            let outcome = paste_outcome(false, requested);
            assert_eq!(outcome, PasteOutcome::KeystrokeFailed, "{requested:?}");
            assert_eq!(outcome.clipboard_after(), AfterPaste::KeepTranscript);
            assert!(!outcome.notify_user());
        }
    }

    #[test]
    fn clipboard_failure_keeps_whatever_is_there() {
        assert_eq!(
            PasteOutcome::ClipboardFailed.clipboard_after(),
            AfterPaste::KeepTranscript
        );
        assert!(!PasteOutcome::ClipboardFailed.notify_user());
    }
}
