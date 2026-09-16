import Foundation

// MARK: - voicectl (command-line snippet editor)
//
// `voicectl` ships inside Voice.app (Contents/MacOS/voicectl) and is symlinked
// onto PATH by scripts/install.sh. It edits the same snippets.json the app
// reads, so scripts and coding agents can manage snippets without clicking
// through the Snippets tab. The running app picks changes up on its own
// (`SnippetStore.reloadIfChanged`), so no relaunch is needed.
//
// All parsing and execution lives in `VoiceCLI.run`, which is a pure function
// of its arguments, stdin and a directory — that is what `CLITests` exercises.
// `VoiceCLI.main` is the thin process wrapper.

struct CLIResult: Equatable {
    var status: Int32
    var stdout: String = ""
    var stderr: String = ""

    static func failure(_ message: String) -> CLIResult { CLIResult(status: 1, stderr: message + "\n") }
    static func usage(_ message: String) -> CLIResult {
        CLIResult(status: 2, stderr: message + "\nRun `voicectl --help` for usage.\n")
    }
}

public enum VoiceCLI {
    static let version = "1.0"

    static let help = """
    voicectl — manage voice snippets from the command line

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

    Exit status: 0 ok, 1 not found / invalid input, 2 usage error.
    """

    /// Process entry point: runs the real CommandLine arguments against the
    /// app's data directory and returns the exit status.
    public static func main() -> Int32 {
        let result = run(Array(CommandLine.arguments.dropFirst()),
                         directory: Store.dir,
                         stdin: { readAllStdin() })
        if !result.stdout.isEmpty { FileHandle.standardOutput.write(Data(result.stdout.utf8)) }
        if !result.stderr.isEmpty { FileHandle.standardError.write(Data(result.stderr.utf8)) }
        return result.status
    }

    /// `stdin` is only invoked when a command actually needs it (`add ... -`,
    /// `import -`), so tests can pass a canned string.
    static func run(_ arguments: [String],
                    directory: URL,
                    stdin: () -> String = { "" }) -> CLIResult {
        var args = arguments
        var dir = directory

        // Global options come before the command.
        while let first = args.first, first.hasPrefix("-") {
            switch first {
            case "-h", "--help":
                return CLIResult(status: 0, stdout: help + "\n")
            case "-V", "--version":
                return CLIResult(status: 0, stdout: "voicectl \(version)\n")
            case "--dir":
                guard args.count >= 2 else { return .usage("--dir needs a path") }
                dir = URL(fileURLWithPath: NSString(string: args[1]).expandingTildeInPath)
                args.removeFirst(2)
            default:
                return .usage("Unknown option: \(first)")
            }
        }

        guard let command = args.first else {
            return CLIResult(status: 2, stderr: help + "\n")
        }
        switch command {
        case "help":
            return CLIResult(status: 0, stdout: help + "\n")
        case "version":
            return CLIResult(status: 0, stdout: "voicectl \(version)\n")
        case "snippets", "snippet":
            return snippets(Array(args.dropFirst()), directory: dir, stdin: stdin)
        default:
            return .usage("Unknown command: \(command)")
        }
    }

    // MARK: snippets subcommands

    private static func snippets(_ args: [String], directory: URL, stdin: () -> String) -> CLIResult {
        guard let sub = args.first else { return .usage("snippets needs a subcommand") }
        var rest = Array(args.dropFirst())
        let json = rest.contains("--json")
        let replace = rest.contains("--replace")
        rest.removeAll { $0 == "--json" || $0 == "--replace" }
        if let bad = rest.first(where: { $0.hasPrefix("--") }) {
            return .usage("Unknown option: \(bad)")
        }

        let store = SnippetStore(directory: directory)

        switch sub {
        case "list", "ls":
            guard rest.isEmpty else { return .usage("list takes no arguments") }
            if json { return CLIResult(status: 0, stdout: encode(store.snippets)) }
            if store.snippets.isEmpty { return CLIResult(status: 0, stdout: "No snippets.\n") }
            return CLIResult(status: 0, stdout: store.snippets.map(line).joined(separator: "\n") + "\n")

        case "get", "show":
            guard rest.count == 1 else { return .usage("get needs exactly one trigger") }
            guard let s = store.snippet(for: rest[0]) else {
                return .failure("No snippet for trigger: \(SnippetStore.normalizeTrigger(rest[0]))")
            }
            return CLIResult(status: 0, stdout: json ? encode(s) : s.text + "\n")

        case "add", "set":
            guard rest.count == 2 else { return .usage("add needs a trigger and text (use - for stdin)") }
            var text = rest[1]
            if text == "-" {
                // Strip only the trailing newline `echo`/heredocs append; inner
                // newlines are part of a multi-line snippet.
                text = stdin()
                while text.hasSuffix("\n") { text.removeLast() }
            }
            let trigger = SnippetStore.normalizeTrigger(rest[0])
            guard !trigger.isEmpty else { return .failure("Trigger must not be empty") }
            guard !text.isEmpty else { return .failure("Text must not be empty") }
            let replaced = store.snippet(for: trigger) != nil
            store.add(trigger: trigger, text: text)
            return CLIResult(status: 0, stdout: "\(replaced ? "Updated" : "Added") snippet: \(trigger)\n")

        case "remove", "rm", "delete":
            guard rest.count == 1 else { return .usage("remove needs exactly one trigger") }
            let trigger = SnippetStore.normalizeTrigger(rest[0])
            guard store.remove(trigger: trigger) else {
                return .failure("No snippet for trigger: \(trigger)")
            }
            return CLIResult(status: 0, stdout: "Removed snippet: \(trigger)\n")

        case "expand":
            guard rest.count == 1 else { return .usage("expand needs exactly one text argument") }
            return CLIResult(status: 0, stdout: store.expand(rest[0]) + "\n")

        case "export":
            guard rest.isEmpty else { return .usage("export takes no arguments") }
            return CLIResult(status: 0, stdout: encode(store.snippets))

        case "import":
            guard rest.count == 1 else { return .usage("import needs a file path (use - for stdin)") }
            let raw: Data
            if rest[0] == "-" {
                raw = Data(stdin().utf8)
            } else {
                let url = URL(fileURLWithPath: NSString(string: rest[0]).expandingTildeInPath)
                guard let data = try? Data(contentsOf: url) else {
                    return .failure("Cannot read file: \(rest[0])")
                }
                raw = data
            }
            guard let list = try? JSONDecoder().decode([Snippet].self, from: raw) else {
                return .failure("Expected a JSON array of {\"trigger\": ..., \"text\": ...} objects")
            }
            let valid = list.filter {
                !SnippetStore.normalizeTrigger($0.trigger).isEmpty && !$0.text.isEmpty
            }
            let skipped = list.count - valid.count
            if replace {
                store.replaceAll(with: valid)
            } else {
                for s in valid { store.add(trigger: s.trigger, text: s.text) }
            }
            var msg = "Imported \(valid.count) snippet\(valid.count == 1 ? "" : "s")"
            if replace { msg += " (replaced existing)" }
            if skipped > 0 { msg += ", skipped \(skipped) with an empty trigger or text" }
            return CLIResult(status: 0, stdout: msg + "\n")

        case "path":
            guard rest.isEmpty else { return .usage("path takes no arguments") }
            return CLIResult(status: 0, stdout: store.fileURL.path + "\n")

        default:
            return .usage("Unknown snippets subcommand: \(sub)")
        }
    }

    // MARK: helpers

    /// One line per snippet; newlines inside the text are shown as `\n` so a
    /// multi-line snippet cannot masquerade as several rows.
    private static func line(_ s: Snippet) -> String {
        s.trigger + "\t" + s.text.replacingOccurrences(of: "\n", with: "\\n")
    }

    private static func encode<T: Encodable>(_ value: T) -> String {
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? enc.encode(value), let text = String(data: data, encoding: .utf8) else {
            return "[]\n"
        }
        return text + "\n"
    }

    private static func readAllStdin() -> String {
        let data = FileHandle.standardInput.readDataToEndOfFile()
        return String(decoding: data, as: UTF8.self)
    }
}
