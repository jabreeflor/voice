//! voicectl — command-line snippet editor. Reference: Sources/VoiceCore/cli.swift.
//!
//! All parsing and execution lives in `VoiceCli::run`, a pure function of its
//! arguments, stdin and a directory — that is what tests/cli.rs exercises.
//! `VoiceCli::main` is the thin process wrapper.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::expand_tilde;
use crate::store::{Snippet, SnippetStore, Store};

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CliResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliResult {
    fn ok(stdout: impl Into<String>) -> CliResult {
        CliResult {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    fn failure(message: &str) -> CliResult {
        CliResult {
            status: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        }
    }

    fn usage(message: &str) -> CliResult {
        CliResult {
            status: 2,
            stdout: String::new(),
            stderr: format!("{message}\nRun `voicectl --help` for usage.\n"),
        }
    }
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
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut stdin = || {
            let mut buf = Vec::new();
            let _ = std::io::stdin().lock().read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        };
        let result = Self::run(&args, &Store::dir(), &mut stdin);
        if !result.stdout.is_empty() {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(result.stdout.as_bytes());
            let _ = out.flush();
        }
        if !result.stderr.is_empty() {
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(result.stderr.as_bytes());
            let _ = err.flush();
        }
        result.status
    }

    /// `stdin` is only invoked when a command actually needs it (`add ... -`,
    /// `import -`), so tests can pass a canned string.
    pub fn run(args: &[String], directory: &Path, stdin: &mut dyn FnMut() -> String) -> CliResult {
        let mut args = args;
        let mut dir: PathBuf = directory.to_path_buf();

        // Global options come before the command.
        while let Some(first) = args.first().filter(|a| a.starts_with('-')) {
            match first.as_str() {
                "-h" | "--help" => return CliResult::ok(format!("{}\n", Self::HELP)),
                "-V" | "--version" => {
                    return CliResult::ok(format!("voicectl {}\n", Self::VERSION))
                }
                "--dir" => {
                    let Some(path) = args.get(1) else {
                        return CliResult::usage("--dir needs a path");
                    };
                    dir = expand_tilde(path);
                    args = &args[2..];
                }
                _ => return CliResult::usage(&format!("Unknown option: {first}")),
            }
        }

        let Some(command) = args.first() else {
            return CliResult {
                status: 2,
                stdout: String::new(),
                stderr: format!("{}\n", Self::HELP),
            };
        };
        match command.as_str() {
            "help" => CliResult::ok(format!("{}\n", Self::HELP)),
            "version" => CliResult::ok(format!("voicectl {}\n", Self::VERSION)),
            "snippets" | "snippet" => snippets(&args[1..], &dir, stdin),
            _ => CliResult::usage(&format!("Unknown command: {command}")),
        }
    }
}

// MARK: - snippets subcommands

fn snippets(args: &[String], directory: &Path, stdin: &mut dyn FnMut() -> String) -> CliResult {
    let Some(sub) = args.first() else {
        return CliResult::usage("snippets needs a subcommand");
    };
    let json = args.iter().any(|a| a == "--json");
    let replace = args.iter().any(|a| a == "--replace");
    let rest: Vec<&str> = args[1..]
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json" && *a != "--replace")
        .collect();
    if let Some(bad) = rest.iter().find(|a| a.starts_with("--")) {
        return CliResult::usage(&format!("Unknown option: {bad}"));
    }

    let mut store = SnippetStore::new(directory.to_path_buf());

    match sub.as_str() {
        "list" | "ls" => {
            if !rest.is_empty() {
                return CliResult::usage("list takes no arguments");
            }
            if json {
                return CliResult::ok(encode(store.snippets()));
            }
            if store.snippets().is_empty() {
                return CliResult::ok("No snippets.\n");
            }
            let lines: Vec<String> = store.snippets().iter().map(line).collect();
            CliResult::ok(lines.join("\n") + "\n")
        }

        "get" | "show" => {
            let [trigger] = rest[..] else {
                return CliResult::usage("get needs exactly one trigger");
            };
            let Some(s) = store.snippet_for(trigger) else {
                return CliResult::failure(&format!(
                    "No snippet for trigger: {}",
                    SnippetStore::normalize_trigger(trigger)
                ));
            };
            if json {
                CliResult::ok(encode(s))
            } else {
                CliResult::ok(format!("{}\n", s.text))
            }
        }

        "add" | "set" => {
            let [raw_trigger, raw_text] = rest[..] else {
                return CliResult::usage("add needs a trigger and text (use - for stdin)");
            };
            let text = if raw_text == "-" {
                // Strip only the trailing newline `echo`/heredocs append; inner
                // newlines are part of a multi-line snippet.
                stdin().trim_end_matches('\n').to_string()
            } else {
                raw_text.to_string()
            };
            let trigger = SnippetStore::normalize_trigger(raw_trigger);
            if trigger.is_empty() {
                return CliResult::failure("Trigger must not be empty");
            }
            if text.is_empty() {
                return CliResult::failure("Text must not be empty");
            }
            let replaced = store.snippet_for(&trigger).is_some();
            store.add(&trigger, &text);
            let verb = if replaced { "Updated" } else { "Added" };
            CliResult::ok(format!("{verb} snippet: {trigger}\n"))
        }

        "remove" | "rm" | "delete" => {
            let [raw_trigger] = rest[..] else {
                return CliResult::usage("remove needs exactly one trigger");
            };
            let trigger = SnippetStore::normalize_trigger(raw_trigger);
            if !store.remove_trigger(&trigger) {
                return CliResult::failure(&format!("No snippet for trigger: {trigger}"));
            }
            CliResult::ok(format!("Removed snippet: {trigger}\n"))
        }

        "expand" => {
            let [text] = rest[..] else {
                return CliResult::usage("expand needs exactly one text argument");
            };
            CliResult::ok(format!("{}\n", store.expand(text)))
        }

        "export" => {
            if !rest.is_empty() {
                return CliResult::usage("export takes no arguments");
            }
            CliResult::ok(encode(store.snippets()))
        }

        "import" => {
            let [source] = rest[..] else {
                return CliResult::usage("import needs a file path (use - for stdin)");
            };
            let raw: Vec<u8> = if source == "-" {
                stdin().into_bytes()
            } else {
                match std::fs::read(expand_tilde(source)) {
                    Ok(data) => data,
                    Err(_) => return CliResult::failure(&format!("Cannot read file: {source}")),
                }
            };
            let Ok(list) = serde_json::from_slice::<Vec<Snippet>>(&raw) else {
                return CliResult::failure(
                    "Expected a JSON array of {\"trigger\": ..., \"text\": ...} objects",
                );
            };
            let valid: Vec<Snippet> = list
                .iter()
                .filter(|s| {
                    !SnippetStore::normalize_trigger(&s.trigger).is_empty() && !s.text.is_empty()
                })
                .cloned()
                .collect();
            let skipped = list.len() - valid.len();
            if replace {
                store.replace_all(&valid);
            } else {
                for s in &valid {
                    store.add(&s.trigger, &s.text);
                }
            }
            let plural = if valid.len() == 1 { "" } else { "s" };
            let mut msg = format!("Imported {} snippet{plural}", valid.len());
            if replace {
                msg.push_str(" (replaced existing)");
            }
            if skipped > 0 {
                msg.push_str(&format!(
                    ", skipped {skipped} with an empty trigger or text"
                ));
            }
            CliResult::ok(msg + "\n")
        }

        "path" => {
            if !rest.is_empty() {
                return CliResult::usage("path takes no arguments");
            }
            CliResult::ok(format!("{}\n", store.file_path().display()))
        }

        _ => CliResult::usage(&format!("Unknown snippets subcommand: {sub}")),
    }
}

// MARK: - helpers

/// One line per snippet; newlines inside the text are shown as `\n` so a
/// multi-line snippet cannot masquerade as several rows.
fn line(s: &Snippet) -> String {
    format!("{}\t{}", s.trigger, s.text.replace('\n', "\\n"))
}

/// Pretty JSON with sorted keys and a trailing newline (Swift used
/// `.prettyPrinted, .sortedKeys`). Going through `serde_json::Value` is what
/// sorts the keys: `Value`'s map is a `BTreeMap` unless the `preserve_order`
/// feature is on, whereas a derived `Serialize` emits fields in declaration
/// order.
fn encode<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_value(value)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .map(|text| text + "\n")
        .unwrap_or_else(|_| "[]\n".to_string())
}
