# scripts

## `install.sh` — curl | bash installer

Builds voice from source on macOS or Linux and installs it (`/Applications`
on macOS; the `.deb` or an AppImage in `~/.local/bin` on Linux), then puts
`voicectl` on PATH. It checks for `cargo`, `whisper-server` and, on Linux,
the WebKitGTK/GTK/ALSA/xdo development packages, and prints the exact fix
for whatever is missing. Windows users are pointed at the prebuilt bundles
in the README. The script must stay ASCII-only and bash 3.2 compatible.

## `e2e-smoke.sh` — manual tier-4 end-to-end test (macOS)

The unit and integration tests exercise Voice's pieces in isolation. This
script exercises the whole loop the way a person does, on a real logged-in
Mac:

1. Checks that `Voice.app` is built, `whisper-server` is installed, and a
   ggml model is present in `~/voice/models` or `~/.voice/models`.
2. Launches `Voice.app` if it isn't already running and polls
   `http://127.0.0.1:8178/` until the whisper engine accepts connections.
3. Opens a scratch TextEdit document and focuses it.
4. Synthesizes a Right-Option hold via `CGEvent` (keycode 61, `.flagsChanged`
   with `.maskAlternate`), speaks a test phrase through `say` while the key is
   held, then releases.
5. Reads the document back with `osascript` and greps for a keyword from the
   phrase. Prints **PASS** or **FAIL** and exits 0 or 1 accordingly.
6. Closes the scratch document without saving.

Run it from anywhere:

```sh
./scripts/e2e-smoke.sh        # prompts before synthesizing anything
./scripts/e2e-smoke.sh -y     # skip the prompt
```

### Prerequisites

- A built app. The script looks for `Voice.app` in the repo root (what
  `./build.sh` produces) and falls back to
  `target/release/bundle/macos/Voice.app` from a plain `cargo tauri build`
  (honouring `CARGO_TARGET_DIR`).
- `brew install whisper-cpp`.
- A model in `~/voice/models` — launching `Voice.app` once downloads
  `ggml-base.en.bin` automatically.
- The Xcode Command Line Tools: the keystroke synthesis is a small Swift
  script run with `swift -` (the Rust toolchain needs the CLT anyway).
- **Voice.app** has Accessibility and Microphone granted in System Settings >
  Privacy & Security.
- **Your terminal** has Accessibility (to post synthetic events) and Automation >
  TextEdit. macOS prompts on first run; approving mid-run makes that attempt
  fail, so approve and rerun.

While it runs, don't type or click — synthesized keystrokes go to whatever is
focused, and any keypress during the hold cancels dictation by design.

### Audio: quiet room, or BlackHole

By default the script plays the test phrase through your speakers and relies on
the microphone hearing it. That works in a quiet room at normal volume, and not
at all through headphones. It is the least reliable part of the test.

For a deterministic run, route audio digitally with
[BlackHole](https://github.com/ExistentialAudio/BlackHole):

1. `brew install blackhole-2ch`.
2. In Audio MIDI Setup, create a Multi-Output Device (BlackHole 2ch + your
   speakers) and select it as system output.
3. Set input to BlackHole 2ch, then rerun the script — `say` now feeds the mic
   directly with no acoustics involved.

### Known limitations

- Acoustic capture makes failures ambiguous: an empty document means a
  permission problem, while wrong text means the model misheard. The script
  prints which case it hit.
- It assumes the default Right-Option talk key. If you changed it in Voice's
  settings, the synthesized keycode won't match.
- Timings are fixed sleeps (3s warm-up, 4s for transcription). A cold model or
  a heavily loaded machine can exceed them and produce a spurious FAIL.
- TextEdit must be able to open a new document; a Restore Windows prompt or a
  full-screen Space can interfere with focus.
- macOS only. The Windows and Linux builds have no scripted tier-4 check
  yet; dictate into a text editor by hand after a change to the hook, paste
  or audio code there.

## Test tiers

| Tier | What | Where it runs |
| --- | --- | --- |
| 1 | Unit tests for pure logic (`crates/voice-core/tests`, `voice-app` unit tests) | CI (`cargo test --workspace`) |
| 2 | `whisper-server` integration tests (`tests/engine_integration.rs`) | CI (`cargo test --workspace`) |
| 3 | Status decision-table tests (`tests/status.rs`) | CI (`cargo test --workspace`) |
| 4 | Full end-to-end dictation | This script, manually |

`.github/workflows/ci.yml` covers tiers 1–3 on every push to `main` and every
pull request, on Ubuntu, macOS and Windows. It installs whisper.cpp (Homebrew
on macOS, built from source and cached elsewhere) and restores a cached
`ggml-base.en.bin` so the tier-2 tests actually run; a `SKIPPED:` line in the
test log fails the job. Tier 4 can't run there — a CI runner has no
microphone, no logged-in GUI session, and no way to grant TCC permissions.
