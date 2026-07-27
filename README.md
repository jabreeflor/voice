# TheVoice 🎙️

A free, 100% local Wispr Flow alternative for macOS. Hold a key, speak,
release — your words are typed into whatever app you're in. Transcription
runs on your Mac's GPU via [whisper.cpp](https://github.com/ggml-org/whisper.cpp);
nothing ever leaves your machine and there are no API costs.

## How to use

1. `./build.sh` (already done)
2. `open TheVoice.app`
3. Grant the two permissions when prompted:
   - **Microphone** — so it can hear you
   - **Accessibility** (System Settings → Privacy & Security → Accessibility) —
     so it can detect the hotkey and paste text
4. Click into any text field, **hold Right ⌥ (Option)**, speak, release.
   The text appears at your cursor.

- Press **Esc** while recording to cancel.
- Taps shorter than ~0.35 s are ignored, and pressing another key while
  holding the hotkey cancels recording (so Option-based shortcuts still work).
- A menu-bar waveform icon shows status; the icon turns red while recording.

## Menu bar options

- **Hotkey** — Right ⌥ (default), Right ⌘, or fn/🌐.
  (For fn, set *System Settings → Keyboard → "Press 🌐 key to" → Do Nothing*
  so macOS doesn't also open emoji/dictation.)
- **Sounds** — toggle the start/paste blips
- **Start at Login**
- **Copy Last Transcript** — if a paste didn't land where you wanted

## Models

Use the **Model** submenu in the menu bar to switch models or download new
ones — downloads show live progress and the app switches over automatically
when they finish. Installed: `ggml-large-v3-turbo.bin` (best, active) and
`ggml-small.en.bin`.

Models live in `~/thevoice/models/` (or `~/.thevoice/models/`). With no
explicit selection, the app prefers the most accurate model present.
You can also force one: `THEVOICE_MODEL=~/path/to/model.bin open TheVoice.app`

## How it works

- On launch the app spawns `whisper-server` (installed via
  `brew install whisper-cpp`) on `127.0.0.1:8178` so the model stays loaded
  in memory — transcription of a typical utterance takes well under a second.
- A global event tap watches for the hold-to-talk key; audio is captured at
  16 kHz mono with AVAudioEngine.
- On release, the WAV is POSTed to the local server, the transcript is
  cleaned up, placed on the clipboard, and ⌘V is synthesized into the
  frontmost app (your previous clipboard is restored afterwards).

## Rebuilding

After editing `Sources/main.swift`, run `./build.sh` again. Rebuilding
re-signs the binary, so macOS will make you re-grant **Accessibility**
(toggle TheVoice off/on in the list). Microphone permission survives.
