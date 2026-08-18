# AGENTS.md

## Cursor Cloud specific instructions

### This is a macOS-only project — it cannot be built, run, or tested on the Linux Cloud Agent VM

TheVoice is a native macOS menu-bar app for local voice dictation. Every file in
`Sources/VoiceCore` imports Apple-only frameworks (`AppKit`, `AVFoundation`,
`ApplicationServices`, `ServiceManagement`) and the app uses macOS-specific APIs
(CoreGraphics global event taps, `NSPasteboard`, `AVAudioEngine`, `NSPanel`
overlays). These frameworks do **not** exist in the open-source Swift toolchain
that runs on Linux, so `swift build` and `swift test` fail on this VM with
`error: no such module 'AppKit'` — even with a working Swift 6 toolchain
installed. There is no cross-platform subset that compiles on Linux.

Consequences for a Cloud Agent running on Linux:

- Do not attempt to make `swift build` / `swift test` pass here; the failure is a
  platform limitation, not a fixable environment issue.
- `.github/workflows/ci.yml` runs on `runs-on: macos-15`. Build/lint/test must
  happen on a macOS runner (CI or a local Mac), never on this VM.
- The update/install script is intentionally a no-op: installing a Swift Linux
  toolchain is pointless because the macOS frameworks are still missing.

### How the project is actually built/run/tested (on macOS only)

These commands require macOS 13+, the Xcode Command Line Tools, and
`brew install whisper-cpp`. They are documented in `README.md` (Development
section) and `build.sh`; do not duplicate them here beyond this pointer:

- `swift build` — compile.
- `swift test` — unit/integration tests (see `Tests/VoiceCoreTests`). Integration
  tests need `whisper-server` + a `ggml` model under `~/thevoice/models`.
- `./build.sh` — assemble and code-sign `Voice.app`.
- `scripts/e2e-smoke.sh` — manual GUI end-to-end test; needs a logged-in Mac GUI
  session with Accessibility + Microphone permissions granted. Never runs in CI.

### What a Linux Cloud Agent can still do

Code review, static reading, editing Swift/shell/markdown, and reasoning about
logic are all fine. Just remember you cannot compile or execute the result here —
verification must be delegated to macOS CI or a human on a Mac.
