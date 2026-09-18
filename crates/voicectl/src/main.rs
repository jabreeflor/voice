// Thin entry: everything lives in voice_core::cli so it stays testable.
fn main() {
    std::process::exit(voice_core::VoiceCli::main());
}
