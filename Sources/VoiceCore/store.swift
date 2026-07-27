import Foundation

// MARK: - Persistence root

enum Store {
    static var dir: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory,
                                            in: .userDomainMask)[0]
            .appendingPathComponent("Voice")
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }
}

// MARK: - Dictation history

struct DictationEntry: Codable, Equatable {
    let text: String
    let date: Date
    let duration: Double   // seconds of speech
    let latency: Double    // seconds from key-release to paste
}

final class HistoryStore {
    private(set) var entries: [DictationEntry] = []
    private let limit = 300
    private let directory: URL
    private let defaults: UserDefaults
    private var fileURL: URL { directory.appendingPathComponent("history.json") }

    /// Bumped on every mutation so views know when to rebuild.
    private(set) var stamp = 0

    init(directory: URL = Store.dir, defaults: UserDefaults = .standard) {
        self.directory = directory
        self.defaults = defaults
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        load()
        migrateLegacy()
    }

    var totalWords: Int {
        get { defaults.integer(forKey: "wordsTotal") }
        set { defaults.set(newValue, forKey: "wordsTotal") }
    }

    var averageWPM: Int {
        let timed = entries.filter { $0.duration > 0.5 }
        let words = timed.reduce(0) { $0 + $1.text.split(separator: " ").count }
        let secs = timed.reduce(0.0) { $0 + $1.duration }
        guard secs > 1 else { return 0 }
        return Int((Double(words) / secs * 60).rounded())
    }

    var averageLatency: Double {
        let timed = entries.filter { $0.latency > 0 }.prefix(50)
        guard !timed.isEmpty else { return 0 }
        return timed.reduce(0.0) { $0 + $1.latency } / Double(timed.count)
    }

    func add(_ entry: DictationEntry) {
        entries.insert(entry, at: 0)
        if entries.count > limit { entries.removeLast(entries.count - limit) }
        totalWords += entry.text.split(separator: " ").count
        stamp += 1
        save()
    }

    /// Entries grouped by day, newest first, titles like "Today" / "Yesterday".
    func grouped(max maxEntries: Int = 40) -> [(String, [DictationEntry])] {
        let cal = Calendar.current
        let fmt = DateFormatter()
        fmt.dateFormat = "EEEE, MMM d"
        var groups: [(String, [DictationEntry])] = []
        for e in entries.prefix(maxEntries) {
            let title: String
            if cal.isDateInToday(e.date) { title = "Today" }
            else if cal.isDateInYesterday(e.date) { title = "Yesterday" }
            else { title = fmt.string(from: e.date) }
            if groups.last?.0 == title {
                groups[groups.count - 1].1.append(e)
            } else {
                groups.append((title, [e]))
            }
        }
        return groups
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let list = try? JSONDecoder().decode([DictationEntry].self, from: data) else { return }
        entries = list
    }

    private func save() {
        if let data = try? JSONEncoder().encode(entries) {
            try? data.write(to: fileURL)
        }
    }

    /// Import the plain-string history from earlier builds, once.
    private func migrateLegacy() {
        guard let old = defaults.stringArray(forKey: "history"), !old.isEmpty else { return }
        let now = Date()
        for (i, text) in old.enumerated() {
            entries.append(DictationEntry(text: text,
                                          date: now.addingTimeInterval(Double(-i) * 60),
                                          duration: 0, latency: 0))
            totalWords += text.split(separator: " ").count
        }
        defaults.removeObject(forKey: "history")
        stamp += 1
        save()
    }
}

// MARK: - Snippets

struct Snippet: Codable, Equatable {
    var trigger: String
    var text: String
}

final class SnippetStore {
    private(set) var snippets: [Snippet] = []
    private let directory: URL
    private var fileURL: URL { directory.appendingPathComponent("snippets.json") }
    private(set) var stamp = 0

    init(directory: URL = Store.dir) {
        self.directory = directory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        load()
    }

    func add(trigger: String, text: String) {
        let t = trigger.trimmingCharacters(in: CharacterSet(charactersIn: "\" ")).lowercased()
        guard !t.isEmpty, !text.isEmpty else { return }
        snippets.removeAll { $0.trigger == t }
        snippets.append(Snippet(trigger: t, text: text))
        stamp += 1
        save()
    }

    func remove(at index: Int) {
        guard snippets.indices.contains(index) else { return }
        snippets.remove(at: index)
        stamp += 1
        save()
    }

    /// Replace spoken triggers with their expansions. A trigger spoken as the
    /// entire utterance (ignoring case and trailing punctuation) becomes the
    /// snippet verbatim; triggers inside a sentence are swapped in place.
    func expand(_ transcript: String) -> String {
        guard !snippets.isEmpty else { return transcript }
        var out = transcript
        for s in snippets.sorted(by: { $0.trigger.count > $1.trigger.count }) {
            let whole = out.trimmingCharacters(in: .whitespacesAndNewlines)
                .trimmingCharacters(in: CharacterSet(charactersIn: ".,!?"))
            if whole.compare(s.trigger, options: .caseInsensitive) == .orderedSame {
                return s.text
            }
            // Lookarounds instead of \b: a \b after a symbol like "+" needs a
            // word character to follow, so triggers such as "c++" would never
            // match mid-sentence. (?<!\w)…(?!\w) behaves like \b for word-edged
            // triggers and still bounds symbol-edged ones.
            let pattern = "(?<!\\w)" + NSRegularExpression.escapedPattern(for: s.trigger) + "(?!\\w)"
            out = out.replacingOccurrences(
                of: pattern,
                with: NSRegularExpression.escapedTemplate(for: s.text),
                options: [.regularExpression, .caseInsensitive])
        }
        return out
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let list = try? JSONDecoder().decode([Snippet].self, from: data) else { return }
        snippets = list
    }

    private func save() {
        if let data = try? JSONEncoder().encode(snippets) {
            try? data.write(to: fileURL)
        }
    }
}
