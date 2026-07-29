# scripts

## `e2e-smoke.sh` — manual tier-4 end-to-end test

The unit and integration tests exercise Voice's pieces in isolation. This script
exercises the whole loop the way a person does, on a real logged-in Mac:

1. Checks that `Voice.app` is built, `whisper-server` is installed, and a ggml
   model is present in `~/thevoice/models` or `~/.thevoice/models`.
2. Pins the talk key to Right Option (`defaults write com.local.voice hotkey
   "61:0"`), since step 5 can only synthesize that one.
3. Launches `Voice.app` if it isn't already running and polls
   `http://127.0.0.1:8178/` until the whisper engine accepts connections. A
   running instance is restarted first if it was using a different binding.
4. Opens a scratch TextEdit document and focuses it.
5. Synthesizes a Right-Option hold via `CGEvent` (keycode 61, `.flagsChanged`
   with `.maskAlternate`), speaks a test phrase through `say` while the key is
   held, then releases.
6. Reads the document back with `osascript` and greps for a keyword from the
   phrase. Prints **PASS** or **FAIL** and exits 0 or 1 accordingly.
7. Closes the scratch document without saving and restores your talk key.

Run it from anywhere:

```sh
./scripts/e2e-smoke.sh        # prompts before synthesizing anything
./scripts/e2e-smoke.sh -y     # skip the prompt
```

### Prerequisites

- `./build.sh` has been run, so `Voice.app` exists.
- `brew install whisper-cpp`.
- A model in `~/thevoice/models` — launching `Voice.app` once downloads
  `ggml-base.en.bin` automatically.
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
- It pins the talk key to Right Option for the duration of the run and puts
  your binding back afterwards, so a custom binding is no longer a reason for
  the script to fail. If it is killed hard enough to skip its cleanup trap,
  reset the key yourself in Voice's settings.
- Timings are fixed sleeps (3s warm-up, 4s for transcription). A cold model or
  a heavily loaded machine can exceed them and produce a spurious FAIL.
- TextEdit must be able to open a new document; a Restore Windows prompt or a
  full-screen Space can interfere with focus.

## Test tiers

| Tier | What | Where it runs |
| --- | --- | --- |
| 1 | Unit tests for pure logic | CI (`swift test`) |
| 2 | `whisper-server` integration tests | CI (`swift test`) |
| 3 | Status decision-table tests | CI (`swift test`) |
| 4 | Full end-to-end dictation | This script, manually |

`.github/workflows/ci.yml` covers tiers 1–3 on every push to `main` and every
pull request. It installs `whisper-cpp` and restores a cached
`ggml-base.en.bin` so the tier-2 tests actually run instead of skipping. Tier 4
can't run there — a CI runner has no microphone, no logged-in GUI session, and
no way to grant TCC permissions.
