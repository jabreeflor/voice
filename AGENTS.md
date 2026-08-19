# AGENTS.md

voice is a native macOS menu-bar app for local hold-to-talk dictation.
Hold a hotkey, speak, release — the transcript is pasted into the frontmost
app. Audio never leaves the Mac: `whisper-server` (from `brew install whisper-cpp`)
runs on `127.0.0.1:8178`. There is no telemetry, no accounts, and no cloud
transcription.

This file is for coding agents. User-facing install and usage live in `README.md`.

## macOS-only — do not build on Linux Cloud Agent VMs

Every file in `Sources/VoiceCore` imports Apple-only frameworks (`AppKit`,
`AVFoundation`, `ApplicationServices`, `ServiceManagement`). The app uses
CoreGraphics event taps, `NSPasteboard`, `AVAudioEngine`, and `NSPanel`
overlays. Those modules do **not** exist in the open-source Swift toolchain
on Linux, so `swift build` and `swift test` fail here with
`error: no such module 'AppKit'`. There is no cross-platform subset.

Consequences:

- Do not try to make `swift build` / `swift test` pass on Linux. The failure
  is a platform limitation, not a missing toolchain.
- Do not install a Swift Linux toolchain to "fix" the environment; it will
  still lack AppKit.
- `.github/workflows/ci.yml` is `runs-on: macos-15`. Build, lint, and test
  belong on a Mac (CI or a human), never on this VM.
- Code review, static edits to Swift/shell/markdown, and reasoning about
  logic are fine. Delegate verification to macOS CI.

## Commands (macOS 13+, Xcode CLT, `brew install whisper-cpp`)

Do not duplicate the Development section of `README.md` or `build.sh`.

- `swift build` — compile the Swift package.
- `swift test` — unit + integration tests in `Tests/VoiceCoreTests`.
  Integration tests need `whisper-server` and a `ggml` model under
  `~/voice/models` (or `~/.voice/models`).
- `./build.sh` — assemble and code-sign `Voice.app` (bundles `voice` CLI).
- `scripts/e2e-smoke.sh` — manual GUI end-to-end dictation. Needs a logged-in
  Mac session with Accessibility + Microphone granted. Never runs in CI.

Ad-hoc signing (`codesign --sign -`) drops Accessibility after every rebuild;
a real keychain identity keeps grants. Microphone survives either way.

## Layout

```
Package.swift                 SPM: VoiceCore, Voice + voice-cli executables, tests
Sources/VoiceApp/main.swift   Thin entry: VoiceMain.run()
Sources/VoiceCLI/main.swift   Thin entry: SnippetCLI.main() (PATH name: voice)
Sources/VoiceCore/core.swift  Config, models, recorder, whisper engine,
                              transcript cleanup, overlay, hotkey, paste
Sources/VoiceCore/app.swift   AppDelegate, status decision table, recording flow
Sources/VoiceCore/ui.swift    AppKit windows, onboarding, design system
Sources/VoiceCore/store.swift History + snippets (Foundation only)
Sources/VoiceCore/cli.swift   Agent CLI: add / list / remove snippets
Tests/VoiceCoreTests/         XCTest, four tiers (see below)
build.sh                      Release build → Voice.app + bundled voice CLI + codesign
Info.plist                    LSUIElement (no Dock), mic usage string
scripts/install.sh            curl | bash installer (clones main, runs build.sh)
scripts/e2e-smoke.sh          Tier-4 GUI smoke
.github/workflows/ci.yml      macOS 15: brew whisper-cpp, cache base.en, swift test
```

Keep related types in the existing files behind `// MARK:` sections. Do not
split into many small files unless a new concern is genuinely independent
(the way `store.swift` is).

## Dictation loop

1. `HotkeyController` CGEvent tap watches the hold-to-talk key (default
   Right Option, keycode 61). Esc while recording cancels and is swallowed;
   any other key while held cancels and is passed through (so Option-based
   shortcuts still work). Taps shorter than ~0.35 s or fewer than ~4000
   samples are ignored.
2. `Recorder` captures 16 kHz mono float via `AVAudioEngine`, then
   `Recorder.wavData` wraps PCM in a 44-byte RIFF header.
3. `WhisperEngine` POSTs the WAV to `http://127.0.0.1:<port>/inference`.
   Production port is `Config.serverPort` (8178). Tests use **18178** so a
   running `Voice.app` is not disturbed.
4. `cleanTranscript` strips known non-speech markers, then
   `SnippetStore.expand` rewrites triggers, then `pasteText` copies to the
   clipboard, synthesizes ⌘V, and restores the previous clipboard after 0.6 s.

`expand` reloads `snippets.json` first so snippets added via the CLI while
Voice is running are live on the next utterance.

`computeStatus` in `app.swift` is a pure function of `StatusInputs`. Status
copy and precedence belong there (and in `StatusTests`), not inlined in UI.

## Snippet CLI

Agents deliver snippets with the bundled `voice` CLI (SPM target `voice-cli`;
inside the app bundle it is `Voice.app/Contents/MacOS/voice`):

```
voice add <trigger> [text]     # stdin if text is omitted
voice list                     # JSON
voice remove <trigger>
```

`swift run voice-cli -- add brb "be right back"` from a checkout. Writes
`~/Library/Application Support/Voice/snippets.json` — the same file the
app uses. Do not hand-edit that JSON; go through `SnippetStore` / the CLI
so triggers stay normalized. History/snippet tests still use a throwaway
directory, never `Store.dir`.

## Testing

| Tier | What | Where |
| --- | --- | --- |
| 1 | Pure logic (transcript, WAV, config, stores) | CI `swift test` |
| 2 | Real `whisper-server` + fixture audio | CI `swift test` |
| 3 | `computeStatus` decision table | CI `swift test` |
| 4 | Full GUI dictation | `scripts/e2e-smoke.sh` only |

Details for the e2e script are in `scripts/README.md`.

Rules:

- Integration tests `XCTSkip` when `whisper-server` or a model is missing so
  a bare Mac stays green. CI must **not** skip: the workflow installs
  whisper-cpp, caches `ggml-base.en.bin`, and fails the job if the log
  contains `skipped`.
- History/snippet tests must use a throwaway directory (and a throwaway
  `UserDefaults` suite for history). Never `Store.dir` or `.standard`.
- `ConfigTests` may *read* the real models dirs; files they create go in
  temp and must be deleted in `tearDown`.
- Prefer pinning behavior with named tests and comments that say *why*,
  matching the existing suite.

## Conventions

- Swift 5.9, AppKit (not SwiftUI), macOS 13+. Bundle id `com.local.voice`.
- Product name is **voice**; the app bundle and Swift modules stay `Voice` /
  `VoiceCore`. Models live in `~/voice/models` (fallback `~/.voice/models`).
  Override with `VOICE_MODEL`.
- Default model is `ggml-base.en.bin` (fast enough for hold-to-talk).
  `Config.preferredModels` is ordered for latency, not size; tiny is last.
- Extract testable pure functions (`computeStatus`, `cleanTranscript`,
  `Recorder.wavData`) rather than mocking AppKit.
- UI: `Palette`, serif-capable labels, `StickerCard` (ink outline, **no**
  purple offset accent). `CapsuleButton` uses `actionable` instead of
  `isEnabled` so disabled titles do not gray into a blob.
- Comments explain non-obvious constraints (ports, skip-vs-fail, TCC,
  clipboard restore timing). Do not narrate obvious code.

## Invariants — do not break these

- Privacy: `whisper-server` must bind `127.0.0.1` only. No analytics,
  accounts, or sending audio off-device. Hugging Face is used only for
  model downloads.
- Do not "fix" the project to compile on Linux by stubbing AppKit or
  splitting a cross-platform core. The app is macOS-native by design.
- Production inference stays on port 8178; tests stay on 18178.
- `cleanTranscript` only strips the listed artifacts. Unknown `[brackets]`
  are left alone.
- `Info.plist` `LSUIElement` is true (menu-bar accessory, no Dock icon).
- `scripts/install.sh` must stay ASCII-safe for macOS bash 3.2.

## PR notes

CI runs on every push to `main` and every pull request (`macos-15`). A Linux
Cloud Agent cannot pre-run that job; rely on GitHub Actions after push.
Do not add tier-4 e2e to CI (no mic, no GUI session, no TCC grants).
