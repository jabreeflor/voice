# AGENTS.md

voice is a tray app for local hold-to-talk dictation on macOS, Windows and
Linux. Hold a talk key, speak, release — the transcript is pasted into the
frontmost app. Audio never leaves the machine: `whisper-server` (from
whisper.cpp) runs on `127.0.0.1:8178`. There is no telemetry, no accounts,
and no cloud transcription.

This file is for coding agents. User-facing install and usage live in
`README.md`. The app was a macOS Swift app until version 2; it is now a Rust
workspace (Tauri 2 desktop app + a pure core crate).

## Layout

```
Cargo.toml                       workspace: crates/*; [workspace.dependencies] is the
                                 single source of dependency versions
crates/voice-core/               pure logic, no GUI/audio deps; builds + tests everywhere
  src/lib.rs                     module map + re-exports
  src/settings.rs                Settings: JSON key/value file replacing UserDefaults
  src/config.rs                  Hotkey, Config (port, models dirs, model search), ModelCatalog
  src/transcript.rs              clean_transcript
  src/wav.rs                     wav_data (44-byte RIFF header, 16 kHz mono i16)
  src/status.rs                  StatusInputs / compute_status decision table
  src/store.rs                   Store::dir, HistoryStore, SnippetStore
  src/engine.rs                  WhisperEngine (whisper-server child + HTTP), find_binary
  src/download.rs                ModelDownloader
  src/cli.rs                     voicectl: VoiceCli::run (pure) / VoiceCli::main
  tests/*.rs                     tiers 1–3 (one file per former Swift test class)
  tests/fixtures/*.wav           16 kHz mono speech fixtures for tier 2
crates/voicectl/src/main.rs      thin entry: exit(VoiceCli::main())
crates/voice-app/                Tauri 2 desktop app
  Cargo.toml build.rs            tauri-build; binary is `voice` (bundled as Voice / voice)
  tauri.conf.json                windows (main, onboarding, overlay), bundle targets, CSP
  Info.plist                     merged into the macOS bundle (LSUIElement, mic usage string)
  capabilities/default.json      what the webviews may call
  icons/                         app + tray icons
  src/main.rs                    builder, plugins, tray, windows, setup
  src/app.rs                     App state machine, recording flow, status, window show/hide
  src/audio.rs                   Recorder (cpal → 16 kHz mono f32)
  src/hotkey.rs                  HotkeyController (CGEvent tap / rdev)
  src/paste.rs                   paste_text (arboard + enigo, clipboard restore)
  src/tray.rs                    tray icon + menu
  src/overlay.rs                 floating pill window driver
  src/commands.rs                #[tauri::command] handlers + DTOs used by ui/
  src/platform/{mod,macos,windows,linux}.rs  permissions, settings panes, relaunch
  ui/                            static HTML/CSS/JS (index, onboarding, overlay), sounds/
assets/app-prototype.html        HTML mirror of the design tokens — keep in sync with ui/
assets/screenshots/              README images
build.sh                         macOS wrapper: cargo tauri build → ./Voice.app (+ voicectl) + codesign
scripts/install.sh               curl | bash installer for macOS and Linux
scripts/e2e-smoke.sh             tier-4 GUI smoke (macOS, manual)
.github/workflows/ci.yml         ubuntu / macos / windows: fmt, clippy, build, test, bundle
```

Keep related types in the existing modules behind `// MARK:` sections. Do
not split into many small files unless a new concern is genuinely
independent. Module doc comments still cite `Sources/VoiceCore/*.swift` and
`Tests/VoiceCoreTests/*.swift`: those are the pre-migration Swift sources,
removed from the tree and kept only in git history
(`git log --diff-filter=D --name-only -- Sources Tests`). Consult them for
archaeology, never re-add them.

## Commands

Rust stable (edition 2021). Linux additionally needs the WebKitGTK / GTK /
ALSA / xdo dev packages listed in `.github/workflows/ci.yml`; the desktop
app needs `cargo install tauri-cli --version "^2" --locked`.

- `cargo build --workspace` — compile everything.
- `cargo test --workspace` — tiers 1–3. Integration tests need
  `whisper-server` and a ggml model under `~/voice/models` (or
  `~/.voice/models`); otherwise they print `SKIPPED:` and pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` — both gate CI.
- `cargo test -p voice-core` — the fast pure-core suite.
- `cd crates/voice-app && cargo tauri dev` — run the app; `cargo tauri build`
  writes installers to `target/release/bundle/`.
- `./build.sh` — macOS only: bundle, copy `Voice.app` to the repo root with
  `voicectl` inside, sign.
- `scripts/e2e-smoke.sh` — manual GUI end-to-end dictation on a logged-in
  Mac with Accessibility + Microphone granted. Never runs in CI.
- `VOICE_ENGINE_LOG=1` — let `whisper-server`'s stdio through (CI sets it).

Ad-hoc signing (`codesign --sign -`) drops Accessibility after every
rebuild on macOS; a keychain identity keeps grants. Microphone survives
either way.

## Platform-specific code

The three CI jobs are the only places where the `#[cfg(target_os = ...)]`
branches for the other OSes get compiled. On this Linux VM
`cargo check -p voice-app` covers Linux only; add
`cargo check -p voice-app --target x86_64-pc-windows-gnu` (and clippy with
the same `--target`) for Windows. The macOS branches cannot be checked here
(`coreaudio-sys` needs the SDK), so change them carefully and verify every
API against the crate sources under `~/.cargo/registry/src/*/` — never from
memory — and let the macOS CI job be the compiler. Do not add stubs or
feature flags to make macOS code compile elsewhere.

## Dictation loop

1. `hotkey::HotkeyController` watches the talk key (default
   `Hotkey::RightOption`: Right ⌥ on macOS, Right Alt elsewhere). macOS uses
   a CGEvent tap (`core-graphics`), Windows `rdev::grab`, Linux/X11
   `rdev::listen` (XRecord, so Esc cannot be swallowed there; Wayland is
   refused up front by `platform::linux`). Esc while recording cancels and
   is swallowed; any other key cancels and is passed through so
   Option/Alt-based shortcuts still work. Key repeats are ignored.
   `tap_running()` reports whether the OS accepted the hook; on macOS it
   fails without Accessibility and `App` retries every second.
2. `audio::Recorder` captures the default input with `cpal`, converts to
   mono f32 at 16 kHz (`rubato`), and keeps a running level for the overlay.
   Releases shorter than 0.35 s or with 4000 samples or fewer are ignored
   (`app::recording_long_enough`), then `voice_core::wav_data` wraps PCM in a
   44-byte RIFF header.
3. `voice_core::engine::WhisperEngine` POSTs the WAV to
   `http://127.0.0.1:<port>/inference`. Production port is
   `Config::SERVER_PORT` (8178). Tests use **18178** so a running app is not
   disturbed. `find_binary` searches next to the executable, `PATH`, then
   the fixed install prefixes (Homebrew, `/usr/bin`, `~/.local/bin`,
   `%LOCALAPPDATA%\voice\bin`).
4. `voice_core::clean_transcript` strips known non-speech markers, then
   `SnippetStore::expand` rewrites triggers, then `paste::paste_text` copies
   to the clipboard, synthesizes ⌘V / Ctrl+V with `enigo`, and restores the
   previous clipboard after 0.6 s. History is appended, the overlay flashes
   "Typed: …" (24-char prefix), and a trailing space is added when
   `trailingSpace` is on.

`voice_core::status::compute_status` is a pure function of `StatusInputs`
(including `Platform`). Status copy and precedence belong there (and in
`tests/status.rs`), never inlined in the UI or `app.rs`.

## voicectl (bundled CLI)

`voicectl` is the supported way for scripts and agents to edit snippets:
`voicectl snippets list|get|add|remove|expand|export|import|path`, `--json`
for machine output, `-` for stdin, `--dir` to target another data
directory. Exit codes: 0 ok, 1 not found/invalid, 2 usage. It writes the
same `snippets.json` the app reads; `SnippetStore::reload_if_changed`
(called from `expand`, from mutations, and from the Snippets tab's refresh)
is what makes the running app see those edits. `build.sh` copies it into
`Voice.app/Contents/MacOS/`; the Linux/Windows bundles do not include it
(`cargo build --release -p voicectl`). Keep `VoiceCli::run` pure and cover
new commands in `tests/cli.rs`.

## Testing

| Tier | What | Where |
| --- | --- | --- |
| 1 | Pure logic: `tests/{transcript,wav,config,settings,history,snippets,cli,download}.rs` plus the fake-server unit tests at the bottom of `tests/engine_integration.rs`; `voice-app` unit tests (`cargo test -p voice-app`: hotkey rules, recording guards, Wayland detection) | CI `cargo test --workspace` |
| 2 | Real `whisper-server` + fixture audio: `tests/engine_integration.rs` `test1..test5` | CI `cargo test --workspace` |
| 3 | `compute_status` decision table: `tests/status.rs` | CI `cargo test --workspace` |
| 4 | Full GUI dictation | `scripts/e2e-smoke.sh` only (macOS) |

Details for the e2e script are in `scripts/README.md`.

Rules:

- Integration tests print `SKIPPED: <reason>` to the real stdout and
  return when `whisper-server` or a model is missing, so a bare machine
  stays green. Never `#[ignore]` them (an ignored test prints nothing for
  CI to catch). CI must **not** skip: the workflow installs whisper.cpp,
  caches `ggml-base.en.bin`, and fails the job if the log contains
  `SKIPPED`.
- History/snippet/settings tests use a throwaway `tempfile` directory and a
  throwaway `Settings::in_dir`. Never `Store::dir()` or the real
  `settings.json`.
- `tests/config.rs` may *read* the real models dirs; files a test creates go
  in temp dirs that are dropped at the end.
- Prefer pinning behaviour with named tests and comments that say *why*,
  matching the existing suite.

## Conventions

- Rust edition 2021, stable toolchain. No `unsafe` outside
  `crates/voice-app/src/platform/` and FFI shims (the macOS CGEvent tap in
  `hotkey.rs` and `overlay.rs` window tweaks). No `unwrap()` on
  user-controlled data paths.
- `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all --check` run in CI; cross-check Windows with
  `cargo clippy -p voice-app --all-targets --target x86_64-pc-windows-gnu -- -D warnings`.
- Confirm any crate API against its source under `~/.cargo/registry/src/*/`
  before using it.
- Product name is **voice**; the bundle is `Voice` (`Voice.app`, the
  `Voice_x.y.z_*` installers), the executable inside every bundle is the
  cargo bin name `voice` (`Contents/MacOS/voice`, `voice.exe`, Linux
  `voice`), the crates stay `voice-core`,
  `voicectl`, `voice-app`. Bundle id `com.local.voice`. Models live in
  `~/voice/models` (fallback `~/.voice/models`), override with `VOICE_MODEL`.
- Default model is `ggml-base.en.bin` (fast enough for hold-to-talk).
  `Config::PREFERRED_MODELS` is ordered for latency, not size; tiny is last.
- Extract testable pure functions (`compute_status`, `clean_transcript`,
  `wav_data`, `recording_long_enough`) rather than mocking Tauri or cpal.
- UI: static HTML/CSS/JS in `crates/voice-app/ui` with `withGlobalTauri`
  and the `#[tauri::command]`/event contract documented in `commands.rs`
  and `ui/app.js`. Apple-native chat look from `assets/app-prototype.html`:
  white window, flat light-gray bubble cards (no outlines, no dashed rules),
  one black pill for the primary action, blue accent for
  links/selection/emphasis, orange/green mascot colours for the brand blob
  and hero art. System sans only: no serif, no italics, no uppercase
  microcaps (section headers are sentence case). Keep the prototype and
  `ui/app.css` tokens in sync.
- Comments explain non-obvious constraints (ports, skip-vs-fail, TCC,
  clipboard restore timing, Wayland). Do not narrate obvious code.

## Invariants — do not break these

- Privacy: `whisper-server` must bind `127.0.0.1` only. No analytics,
  accounts, or sending audio off-device. Hugging Face is used only for
  model downloads.
- Production inference stays on port 8178; tests stay on 18178.
- `clean_transcript` only strips the listed artifacts. Unknown `[brackets]`
  are left alone.
- `crates/voice-app/Info.plist` keeps `LSUIElement` true and the microphone
  usage string; tauri-bundler merges it into the generated bundle plist. On
  macOS the activation policy is Accessory until a window is shown.
- `Settings` keys keep the Swift `UserDefaults` names (`hotkey`, `sounds`,
  `trailingSpace`, `modelFile`, `onboarded`, `lastAXRelaunch`, `wordsTotal`)
  and `Hotkey::raw_value` keeps the Swift raw strings.
- `history.json` and `snippets.json` stay byte-compatible with the files the
  Swift app wrote: `DictationEntry.date` is stored as seconds since
  2001-01-01 (Apple reference date), and `Store::dir()` is
  `<platform data dir>/Voice` (`~/Library/Application Support/Voice` on
  macOS).
- `scripts/install.sh` must stay ASCII-safe for macOS bash 3.2.
- Do not add tier-4 e2e to CI (no mic, no GUI session, no TCC grants).

## PR notes

CI runs on every push to `main` and every pull request on
`ubuntu-24.04`, `macos-15` and `windows-2022`; pushes to `main` also run
the `Bundle app` jobs. A Linux agent can pre-run the Linux job locally
(fmt, clippy, build, `cargo test --workspace`) but not the macOS or Windows
compile of the cfg'd code; rely on GitHub Actions after push.
