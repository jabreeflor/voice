//! macOS only: did the ⌘V land? Answered with a *promised* pasteboard item.
//!
//! Instead of writing the transcript to the general pasteboard, `place`
//! writes an `NSPasteboardItem` whose string data is supplied lazily by a
//! data provider (`NSPasteboardItemDataProvider`). AppKit calls the provider
//! only when some app actually reads the item, so "nobody asked within ~1 s
//! of ⌘V" means the keystroke reached nothing that pastes (no focused text
//! field, a secure input field, an app that ignored it).
//!
//! Constraints this relies on, observed on macOS 26 with a scratch program
//! built against the same objc2 crates:
//!
//! - The provider callback is delivered on the **main thread** through its
//!   run loop, even when the item was written from a worker. Tauri's main
//!   thread runs the AppKit event loop, so it is serviced; a blocked main
//!   thread would stall the reading app until it is free. `paste::paste_text`
//!   waits on a worker and never on the main thread.
//! - A reader in another process gets the text once the callback has set it
//!   (`setString:forType:` on the item inside the callback).
//!
//! Callers gate on `cfg(target_os = "macos")`; there is no equivalent on
//! Windows or Linux, which keep the plain clipboard write.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardItemDataProvider, NSPasteboardType,
    NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSInteger, NSObject, NSObjectProtocol, NSString};

/// Set (once) by the provider callback; waited on by the paste worker.
#[derive(Default)]
struct Requested {
    flag: Mutex<bool>,
    changed: Condvar,
}

impl Requested {
    fn mark(&self) {
        *self.flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut flag = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        while !*flag {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            flag = match self.changed.wait_timeout(flag, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        *flag
    }
}

struct ProviderIvars {
    text: String,
    requested: Arc<Requested>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the class has no
    // Drop impl. Not MainThreadOnly: it is created on the paste worker and
    // called on the main thread, and its ivars are Send + Sync.
    #[unsafe(super(NSObject))]
    #[ivars = ProviderIvars]
    struct TextProvider;

    unsafe impl NSObjectProtocol for TextProvider {}

    // SAFETY: the one required method is implemented with the signature from
    // objc2-app-kit's NSPasteboardItemDataProvider declaration.
    unsafe impl NSPasteboardItemDataProvider for TextProvider {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide_data(
            &self,
            _pasteboard: Option<&NSPasteboard>,
            item: &NSPasteboardItem,
            data_type: &NSPasteboardType,
        ) {
            let ivars = self.ivars();
            item.setString_forType(&NSString::from_str(&ivars.text), data_type);
            ivars.requested.mark();
        }
    }
);

/// A promise for some text, currently (or formerly) on the general
/// pasteboard. Keep it alive until the pasteboard holds something else:
/// `paste_text` replaces the promise (restore, or a concrete copy of the
/// text) before dropping it, so AppKit never calls a released provider.
pub struct PromisedText {
    _provider: Retained<TextProvider>,
    requested: Arc<Requested>,
    /// `None` for the general pasteboard. A name rather than the
    /// `NSPasteboard` itself, which is not `Send`.
    pasteboard_name: Option<String>,
    change_count: NSInteger,
}

impl PromisedText {
    /// Clears the general pasteboard and writes a lazily provided string
    /// item for `text`. `None` when AppKit refuses either step; the caller
    /// then writes the text normally and simply cannot tell whether the
    /// paste landed.
    pub fn place(text: &str) -> Option<PromisedText> {
        Self::place_on(None, text)
    }

    /// [`place`](Self::place) on a named pasteboard instead of the general
    /// one, so `tests/macos_pasteboard.rs` can exercise the real promise
    /// without touching the user's clipboard.
    pub fn place_on(pasteboard_name: Option<&str>, text: &str) -> Option<PromisedText> {
        let requested = Arc::new(Requested::default());
        let provider = TextProvider::alloc().set_ivars(ProviderIvars {
            text: text.to_string(),
            requested: Arc::clone(&requested),
        });
        // SAFETY: `init` on a freshly allocated NSObject subclass whose ivars
        // were just set, as in objc2's define_class! documentation.
        let provider: Retained<TextProvider> = unsafe { msg_send![super(provider), init] };

        // SAFETY: a constant NSString owned by AppKit for the process lifetime.
        let string_type = unsafe { NSPasteboardTypeString };
        let item = NSPasteboardItem::new();
        let types = NSArray::from_slice(&[string_type]);
        if !item.setDataProvider_forTypes(ProtocolObject::from_ref(&*provider), &types) {
            return None;
        }
        let pasteboard = pasteboard(pasteboard_name);
        pasteboard.clearContents();
        let objects = NSArray::from_retained_slice(&[
            ProtocolObject::<dyn NSPasteboardWriting>::from_retained(item),
        ]);
        if !pasteboard.writeObjects(&objects) {
            return None;
        }
        Some(PromisedText {
            _provider: provider,
            requested,
            pasteboard_name: pasteboard_name.map(str::to_string),
            change_count: pasteboard.changeCount(),
        })
    }

    /// Blocks up to `timeout`; true as soon as any app has read the text.
    pub fn wait_requested(&self, timeout: Duration) -> bool {
        self.requested.wait(timeout)
    }

    /// True while nothing else has been written to the pasteboard since
    /// `place` (the user copying something must not be overwritten).
    pub fn still_on_pasteboard(&self) -> bool {
        pasteboard(self.pasteboard_name.as_deref()).changeCount() == self.change_count
    }
}

fn pasteboard(name: Option<&str>) -> Retained<NSPasteboard> {
    match name {
        Some(name) => NSPasteboard::pasteboardWithName(&NSString::from_str(name)),
        None => NSPasteboard::generalPasteboard(),
    }
}
