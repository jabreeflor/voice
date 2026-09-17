//! Port of Tests/VoiceCoreTests/CLITests.swift.
//!
//! `voicectl` is how scripts and coding agents edit snippets, so its output and
//! exit codes are a contract. `VoiceCli::run` is pure over (args, stdin, dir);
//! every test here uses a throwaway directory, never the real `Store::dir()`.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use voice_core::{CliResult, Snippet, SnippetStore, VoiceCli};

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            dir: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> CliResult {
        self.run_stdin(args, "")
    }

    fn run_stdin(&self, args: &[&str], stdin: &str) -> CliResult {
        run_in(self.path(), args, stdin)
    }

    fn store(&self) -> SnippetStore {
        SnippetStore::new(self.path().to_path_buf())
    }
}

fn run_in(dir: &Path, args: &[&str], stdin: &str) -> CliResult {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut stdin = || stdin.to_string();
    VoiceCli::run(&args, dir, &mut stdin)
}

fn snippet(trigger: &str, text: &str) -> Snippet {
    Snippet {
        trigger: trigger.to_string(),
        text: text.to_string(),
    }
}

fn path_string(p: PathBuf) -> String {
    p.display().to_string()
}

// MARK: - Top level

#[test]
fn help_and_version_exit_zero() {
    let f = Fixture::new();
    for flags in [&["--help"][..], &["-h"], &["help"]] {
        let r = f.run(flags);
        assert_eq!(r.status, 0, "{flags:?}");
        assert!(r.stdout.contains("voicectl snippets add"), "{flags:?}");
        assert_eq!(r.stderr, "");
    }
    assert_eq!(
        f.run(&["--version"]).stdout,
        format!("voicectl {}\n", VoiceCli::VERSION)
    );
}

/// No command is a usage error (2) so a script can tell it from "not found".
#[test]
fn no_command_is_usage_error() {
    let f = Fixture::new();
    let r = f.run(&[]);
    assert_eq!(r.status, 2);
    assert!(r.stderr.contains("Usage:"));
}

#[test]
fn unknown_command_and_option_are_usage_errors() {
    let f = Fixture::new();
    assert_eq!(f.run(&["bogus"]).status, 2);
    assert_eq!(f.run(&["--bogus"]).status, 2);
    assert_eq!(f.run(&["snippets"]).status, 2);
    assert_eq!(f.run(&["snippets", "bogus"]).status, 2);
    assert_eq!(f.run(&["snippets", "list", "--bogus"]).status, 2);
    assert_eq!(f.run(&["snippets", "list", "extra"]).status, 2);
}

// MARK: - add / list / get / remove

#[test]
fn add_then_list_and_get() {
    let f = Fixture::new();
    let add = f.run(&["snippets", "add", "brb", "be right back"]);
    assert_eq!(add.status, 0);
    assert_eq!(add.stdout, "Added snippet: brb\n");

    assert_eq!(f.run(&["snippets", "list"]).stdout, "brb\tbe right back\n");
    assert_eq!(f.run(&["snippets", "get", "brb"]).stdout, "be right back\n");
    assert_eq!(
        f.run(&["snippets", "get", "BRB"]).stdout,
        "be right back\n",
        "lookup normalizes the trigger the way add does"
    );
}

#[test]
fn add_existing_trigger_reports_update() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "brb", "be right back"]);
    let r = f.run(&["snippets", "add", "BRB", "back in a bit"]);
    assert_eq!(r.status, 0);
    assert_eq!(r.stdout, "Updated snippet: brb\n");
    assert_eq!(f.store().snippets(), [snippet("brb", "back in a bit")]);
}

/// `-` reads the text from stdin so multi-line snippets (sign-offs,
/// addresses) can be piped in. Only the trailing newline is dropped.
#[test]
fn add_reads_text_from_stdin_and_keeps_inner_newlines() {
    let f = Fixture::new();
    let r = f.run_stdin(&["snippets", "add", "signoff", "-"], "Best,\nJabree\n");
    assert_eq!(r.status, 0);
    assert_eq!(
        f.store().snippet_for("signoff").map(|s| s.text.clone()),
        Some("Best,\nJabree".to_string())
    );
    // Multi-line text is escaped in the human listing so one snippet is one row.
    assert_eq!(
        f.run(&["snippets", "list"]).stdout,
        "signoff\tBest,\\nJabree\n"
    );
}

/// CRLF-terminated input (Windows `type file | voicectl ...`, CRLF editors)
/// must not leave a stray `\r` at the end of the stored text, and inner CRLFs
/// are escaped in the listing like bare newlines so one snippet is one row.
#[test]
fn add_from_stdin_strips_trailing_crlf_pairs() {
    let f = Fixture::new();
    let r = f.run_stdin(
        &["snippets", "add", "signoff", "-"],
        "Best,\r\nJabree\r\n\r\n",
    );
    assert_eq!(r.status, 0);
    assert_eq!(
        f.store().snippet_for("signoff").map(|s| s.text.clone()),
        Some("Best,\r\nJabree".to_string())
    );
    assert_eq!(
        f.run(&["snippets", "list"]).stdout,
        "signoff\tBest,\\nJabree\n"
    );
    // Whitespace-only input (a lone CRLF) is still rejected as empty.
    assert_eq!(
        f.run_stdin(&["snippets", "add", "empty", "-"], "\r\n")
            .status,
        1
    );
}

#[test]
fn add_rejects_empty_trigger_or_text() {
    let f = Fixture::new();
    assert_eq!(f.run(&["snippets", "add", "  ", "text"]).status, 1);
    assert_eq!(
        f.run_stdin(&["snippets", "add", "trigger", "-"], "\n")
            .status,
        1
    );
    assert_eq!(f.run(&["snippets", "add", "onlyone"]).status, 2);
    assert!(f.store().snippets().is_empty());
}

#[test]
fn list_on_empty_store() {
    let f = Fixture::new();
    assert_eq!(f.run(&["snippets", "list"]).stdout, "No snippets.\n");
    let json = f.run(&["snippets", "list", "--json"]).stdout;
    assert_eq!(
        serde_json::from_str::<Vec<Snippet>>(&json).ok(),
        Some(Vec::new())
    );
}

#[test]
fn get_missing_trigger_exits_one() {
    let f = Fixture::new();
    let r = f.run(&["snippets", "get", "nope"]);
    assert_eq!(r.status, 1);
    assert_eq!(r.stdout, "");
    assert!(r.stderr.contains("nope"));
}

#[test]
fn remove_by_trigger_and_missing_trigger() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "a", "AAA"]);
    f.run(&["snippets", "add", "b", "BBB"]);
    let r = f.run(&["snippets", "remove", "A"]);
    assert_eq!(r.status, 0);
    assert_eq!(r.stdout, "Removed snippet: a\n");
    let triggers: Vec<String> = f
        .store()
        .snippets()
        .iter()
        .map(|s| s.trigger.clone())
        .collect();
    assert_eq!(triggers, ["b"]);
    assert_eq!(f.run(&["snippets", "remove", "a"]).status, 1);
}

// MARK: - JSON output (what agents parse)

#[test]
fn json_list_matches_file_format() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "brb", "be right back"]);
    let out = f.run(&["snippets", "list", "--json"]).stdout;
    let decoded: Vec<Snippet> = serde_json::from_str(&out).expect("valid JSON array");
    assert_eq!(decoded, [snippet("brb", "be right back")]);
    assert_eq!(
        f.run(&["snippets", "export"]).stdout,
        out,
        "export and list --json are the same document"
    );
}

#[test]
fn json_get() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "brb", "be right back"]);
    let out = f.run(&["snippets", "get", "brb", "--json"]).stdout;
    let decoded: Snippet = serde_json::from_str(&out).expect("valid JSON object");
    assert_eq!(decoded, snippet("brb", "be right back"));
}

/// Swift encoded with `.prettyPrinted, .sortedKeys`; agents may diff or
/// hash the output, so the layout is part of the contract. The bytes are
/// serde_json's, though, not Foundation's — intentionally: serde_json writes
/// `"key": value` where JSONEncoder wrote `"key" : value`, and an empty
/// array as `[]` where JSONEncoder printed `[\n\n]`. What this pins is
/// sorted keys, 2-space indentation and the trailing newline. `Snippet`
/// declares `trigger` before `text`, so a derived encoder would emit them in
/// that order — the sorted order is the part that matters.
#[test]
fn json_is_pretty_printed_with_sorted_keys() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "brb", "be right back"]);
    assert_eq!(
        f.run(&["snippets", "get", "brb", "--json"]).stdout,
        "{\n  \"text\": \"be right back\",\n  \"trigger\": \"brb\"\n}\n"
    );
    assert_eq!(
        f.run(&["snippets", "export"]).stdout,
        "[\n  {\n    \"text\": \"be right back\",\n    \"trigger\": \"brb\"\n  }\n]\n"
    );
}

// MARK: - import

#[test]
fn import_merges_by_default() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "keep", "KEEP"]);
    f.run(&["snippets", "add", "brb", "old"]);
    let json = r#"[{"trigger":"BRB","text":"new"},{"trigger":"sig","text":"Jabree"}]"#;
    let r = f.run_stdin(&["snippets", "import", "-"], json);
    assert_eq!(r.status, 0);
    assert_eq!(r.stdout, "Imported 2 snippets\n");
    let store = f.store();
    assert_eq!(
        store.snippet_for("keep").map(|s| s.text.as_str()),
        Some("KEEP")
    );
    assert_eq!(
        store.snippet_for("brb").map(|s| s.text.as_str()),
        Some("new")
    );
    assert_eq!(
        store.snippet_for("sig").map(|s| s.text.as_str()),
        Some("Jabree")
    );
}

#[test]
fn import_replace_drops_existing() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "keep", "KEEP"]);
    let r = f.run_stdin(
        &["snippets", "import", "-", "--replace"],
        r#"[{"trigger":"sig","text":"Jabree"}]"#,
    );
    assert_eq!(r.status, 0);
    assert_eq!(r.stdout, "Imported 1 snippet (replaced existing)\n");
    assert_eq!(f.store().snippets(), [snippet("sig", "Jabree")]);
}

#[test]
fn import_from_file_skips_invalid_entries() {
    let f = Fixture::new();
    let file = f.path().join("in.json");
    fs::write(
        &file,
        r#"[{"trigger":"","text":"x"},{"trigger":"ok","text":""},{"trigger":"ok","text":"fine"}]"#,
    )
    .expect("write fixture");
    let r = f.run(&["snippets", "import", &path_string(file)]);
    assert_eq!(r.status, 0);
    assert_eq!(
        r.stdout,
        "Imported 1 snippet, skipped 2 with an empty trigger or text\n"
    );
    assert_eq!(f.store().snippets(), [snippet("ok", "fine")]);
}

#[test]
fn import_rejects_bad_json_and_missing_file() {
    let f = Fixture::new();
    assert_eq!(
        f.run_stdin(&["snippets", "import", "-"], "not json").status,
        1
    );
    let missing = path_string(f.path().join("missing.json"));
    assert_eq!(f.run(&["snippets", "import", &missing]).status, 1);
    assert!(f.store().snippets().is_empty());
}

/// Round trip: what export prints, import accepts unchanged.
#[test]
fn export_import_round_trip() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "c++", "C plus plus"]);
    f.run_stdin(&["snippets", "add", "signoff", "-"], "Best,\nJabree\n");
    let exported = f.run(&["snippets", "export"]).stdout;
    let other = f.path().join("other");
    let r = run_in(
        f.path(),
        &[
            "--dir",
            &path_string(other.clone()),
            "snippets",
            "import",
            "-",
        ],
        &exported,
    );
    assert_eq!(r.status, 0);
    assert_eq!(SnippetStore::new(other).snippets(), f.store().snippets());
}

// MARK: - expand / path / --dir

#[test]
fn expand_previews_dictation_rewrite() {
    let f = Fixture::new();
    f.run(&["snippets", "add", "brb", "be right back"]);
    assert_eq!(
        f.run(&["snippets", "expand", "okay brb soon"]).stdout,
        "okay be right back soon\n"
    );
}

#[test]
fn path_points_into_the_directory() {
    let f = Fixture::new();
    assert_eq!(
        f.run(&["snippets", "path"]).stdout,
        path_string(f.path().join("snippets.json")) + "\n"
    );
}

#[test]
fn dir_option_overrides_directory() {
    let f = Fixture::new();
    let other = f.path().join("other");
    let r = run_in(
        f.path(),
        &[
            "--dir",
            &path_string(other.clone()),
            "snippets",
            "add",
            "x",
            "X",
        ],
        "",
    );
    assert_eq!(r.status, 0);
    assert!(f.store().snippets().is_empty());
    assert_eq!(SnippetStore::new(other).snippets().len(), 1);
    assert_eq!(f.run(&["--dir"]).status, 2);
}

/// The whole point of the CLI: an already-loaded store (the running app)
/// sees what voicectl wrote.
#[test]
fn running_store_sees_cli_edits() {
    let f = Fixture::new();
    let mut app_store = f.store();
    assert_eq!(app_store.expand("brb"), "brb");
    f.run(&["snippets", "add", "brb", "be right back"]);
    assert_eq!(app_store.expand("brb"), "be right back");
    f.run(&["snippets", "remove", "brb"]);
    assert_eq!(app_store.expand("brb"), "brb");
}
