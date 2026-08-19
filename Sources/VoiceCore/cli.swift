import Darwin
import Foundation

// MARK: - Snippet CLI

/// Agent-facing CLI for delivering snippets to Voice. Bundled as
/// `Voice.app/Contents/MacOS/voice` and installed onto PATH as `voice`.
public enum SnippetCLI {
    enum Command: Equatable {
        case add(trigger: String, text: String?)
        case list
        case remove(trigger: String)
        case help
    }

    static let usage = """
    voice — add snippets to Voice dictation

    Usage:
      voice add <trigger> [text]
      voice list
      voice remove <trigger>

    If text is omitted, it is read from stdin (verbatim). Adding the same
    trigger replaces the previous snippet. Voice picks up changes immediately.

    Examples:
      voice add brb "be right back"
      voice add "my email" you@example.com
      voice add sig < signature.txt
      voice list
      voice remove brb
    """

    /// Entry used by the `voice-cli` executable. Returns a process exit code.
    public static func main(arguments: [String] = CommandLine.arguments) -> Int32 {
        execute(arguments: arguments, store: SnippetStore())
    }

    static func execute(
        arguments: [String],
        store: SnippetStore,
        stdinTTY: Bool = isatty(STDIN_FILENO) == 1,
        readStdin: () -> String? = {
            guard let data = try? FileHandle.standardInput.readToEnd() else { return nil }
            return String(data: data, encoding: .utf8)
        },
        stdout: (String) -> Void = { fputs($0, Darwin.stdout) },
        stderr: (String) -> Void = { fputs($0, Darwin.stderr) }
    ) -> Int32 {
        switch parse(Array(arguments.dropFirst())) {
        case .failure(let message):
            stderr("error: \(message)\n\n\(usage)\n")
            return 1
        case .success(.help):
            stdout("\(usage)\n")
            return 0
        case .success(let command):
            return run(command, store: store, stdinTTY: stdinTTY,
                       readStdin: readStdin, stdout: stdout, stderr: stderr)
        }
    }

    static func parse(_ args: [String]) -> Result<Command, String> {
        var args = args
        if let first = args.first, first == "snippet" || first == "snippets" {
            args.removeFirst()
        }
        guard let cmd = args.first else { return .failure("missing command") }
        switch cmd {
        case "help", "-h", "--help":
            return .success(.help)
        case "list", "ls":
            return .success(.list)
        case "add":
            guard args.count >= 2 else { return .failure("add needs a trigger") }
            let trigger = args[1]
            if args.count == 2 { return .success(.add(trigger: trigger, text: nil)) }
            return .success(.add(trigger: trigger, text: args.dropFirst(2).joined(separator: " ")))
        case "remove", "rm", "delete":
            guard args.count >= 2 else { return .failure("remove needs a trigger") }
            return .success(.remove(trigger: args.dropFirst().joined(separator: " ")))
        default:
            return .failure("unknown command '\(cmd)'")
        }
    }

    private static func run(
        _ command: Command,
        store: SnippetStore,
        stdinTTY: Bool,
        readStdin: () -> String?,
        stdout: (String) -> Void,
        stderr: (String) -> Void
    ) -> Int32 {
        switch command {
        case .help:
            stdout("\(usage)\n")
            return 0
        case .list:
            return list(store, stdout: stdout, stderr: stderr)
        case .remove(let trigger):
            if store.remove(trigger: trigger) {
                stdout("removed: \(SnippetStore.normalizeTrigger(trigger))\n")
                return 0
            }
            stderr("error: no snippet named '\(SnippetStore.normalizeTrigger(trigger))'\n")
            return 1
        case .add(let trigger, let provided):
            let text: String
            if let provided {
                text = provided
            } else if stdinTTY {
                stderr("error: missing snippet text (pass it as an argument or pipe it on stdin)\n")
                return 1
            } else if let piped = readStdin() {
                text = piped
            } else {
                stderr("error: could not read snippet text from stdin\n")
                return 1
            }
            if store.add(trigger: trigger, text: text) {
                stdout("saved: \(SnippetStore.normalizeTrigger(trigger))\n")
                return 0
            }
            stderr("error: trigger and text must both be non-empty\n")
            return 1
        }
    }

    private static func list(
        _ store: SnippetStore,
        stdout: (String) -> Void,
        stderr: (String) -> Void
    ) -> Int32 {
        store.reload()
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        guard let data = try? enc.encode(store.snippets),
              let json = String(data: data, encoding: .utf8) else {
            stderr("error: could not encode snippets\n")
            return 1
        }
        stdout(json.hasSuffix("\n") ? json : json + "\n")
        return 0
    }
}
