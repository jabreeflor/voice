//! voicectl — command-line snippet editor. Reference: Sources/VoiceCore/cli.swift.
//!
//! All parsing and execution lives in `VoiceCli::run`, a pure function of its
//! arguments, stdin and a directory — that is what tests/cli.rs exercises.
//! `VoiceCli::main` is the thin process wrapper.

use std::path::Path;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CliResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct VoiceCli;

impl VoiceCli {
    pub const VERSION: &'static str = "1.0";

    pub const HELP: &'static str = "voicectl — manage voice snippets from the command line

Usage:
  voicectl snippets list [--json]
  voicectl snippets get <trigger> [--json]
  voicectl snippets add <trigger> <text|->        (- reads the text from stdin)
  voicectl snippets remove <trigger>
  voicectl snippets expand <text>                 (preview what dictation would paste)
  voicectl snippets export                        (JSON array to stdout)
  voicectl snippets import <file|-> [--replace]   (JSON array; --replace drops existing)
  voicectl snippets path                          (where snippets.json lives)
  voicectl --version | --help

Options:
  --dir <path>   Use a different data directory (default: the app's).

A snippet's trigger is a phrase you say while dictating; the text is what
gets pasted instead. Triggers are lowercased. Adding an existing trigger
replaces it. The running Voice app picks up changes immediately.

Exit status: 0 ok, 1 not found / invalid input, 2 usage error.";

    /// Process entry point: real argv, the app's data directory, real stdin.
    pub fn main() -> i32 {
        todo!()
    }

    /// `stdin` is only invoked when a command actually needs it (`add ... -`,
    /// `import -`), so tests can pass a canned string.
    pub fn run(args: &[String], directory: &Path, stdin: &mut dyn FnMut() -> String) -> CliResult {
        let _ = (args, directory, stdin);
        todo!()
    }
}
