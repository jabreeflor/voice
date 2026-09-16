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
    var fileURL: URL { directory.appendingPathComponent("snippets.json") }
    private(set) var stamp = 0

    /// (modification date, size) of snippets.json as of the last load or save.
    /// `reloadIfChanged` compares against it so the app notices edits made by
    /// `voicectl` (or a text editor) without re-reading the file every time.
    private var fingerprint: (Date?, Int)?

    init(directory: URL = Store.dir) {
        self.directory = directory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        load()
    }

    /// The trigger as stored: surrounding quotes/spaces dropped, lowercased.
    /// `add`, `remove(trigger:)` and `snippet(for:)` all go through this so a
    /// CLI user can type `"My Email"` and still hit the `my email` snippet.
    static func normalizeTrigger(_ trigger: String) -> String {
        trigger.trimmingCharacters(in: CharacterSet(charactersIn: "\" ")).lowercased()
    }

    func snippet(for trigger: String) -> Snippet? {
        let t = SnippetStore.normalizeTrigger(trigger)
        return snippets.first { $0.trigger == t }
    }

    /// Adds or replaces the snippet for `trigger`. Returns false (and changes
    /// nothing) when the trigger or text is empty after normalization.
    @discardableResult
    func add(trigger: String, text: String) -> Bool {
        let t = SnippetStore.normalizeTrigger(trigger)
        guard !t.isEmpty, !text.isEmpty else { return false }
        reloadIfChanged()
        snippets.removeAll { $0.trigger == t }
        snippets.append(Snippet(trigger: t, text: text))
        stamp += 1
        save()
        return true
    }

    func remove(at index: Int) {
        guard snippets.indices.contains(index) else { return }
        reloadIfChanged()
        guard snippets.indices.contains(index) else { return }
        snippets.remove(at: index)
        stamp += 1
        save()
    }

    /// Removes the snippet whose trigger matches. Returns false when there is
    /// no such snippet.
    @discardableResult
    func remove(trigger: String) -> Bool {
        reloadIfChanged()
        let t = SnippetStore.normalizeTrigger(trigger)
        let before = snippets.count
        snippets.removeAll { $0.trigger == t }
        guard snippets.count != before else { return false }
        stamp += 1
        save()
        return true
    }

    /// Replaces every snippet with `list` (normalized, empties dropped, later
    /// duplicates winning). Used by `voicectl snippets import --replace`.
    func replaceAll(with list: [Snippet]) {
        var out: [Snippet] = []
        for s in list {
            let t = SnippetStore.normalizeTrigger(s.trigger)
            guard !t.isEmpty, !s.text.isEmpty else { continue }
            out.removeAll { $0.trigger == t }
            out.append(Snippet(trigger: t, text: s.text))
        }
        snippets = out
        stamp += 1
        save()
    }

    /// Re-reads snippets.json if another process has written it since the last
    /// load or save. Returns true when the in-memory list actually changed.
    /// The app calls this before expanding a transcript and while the
    /// Snippets tab is open, so `voicectl` edits take effect without a relaunch.
    @discardableResult
    func reloadIfChanged() -> Bool {
        let current = currentFingerprint()
        guard !sameFingerprint(current, fingerprint) else { return false }
        let previous = snippets
        load()
        if snippets != previous {
            stamp += 1
            return true
        }
        return false
    }

    /// Replace spoken triggers with their expansions. A trigger spoken as the
    /// entire utterance (ignoring case and trailing punctuation) becomes the
    /// snippet verbatim; triggers inside a sentence are swapped in place.
    func expand(_ transcript: String) -> String {
        reloadIfChanged()
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

    private func currentFingerprint() -> (Date?, Int)? {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: fileURL.path) else {
            return nil   // no file
        }
        return (attrs[.modificationDate] as? Date, (attrs[.size] as? NSNumber)?.intValue ?? 0)
    }

    private func sameFingerprint(_ a: (Date?, Int)?, _ b: (Date?, Int)?) -> Bool {
        switch (a, b) {
        case (nil, nil): return true
        case let (x?, y?): return x.0 == y.0 && x.1 == y.1
        default: return false
        }
    }

    private func load() {
        fingerprint = currentFingerprint()
        guard let data = try? Data(contentsOf: fileURL),
              let list = try? JSONDecoder().decode([Snippet].self, from: data) else {
            snippets = []
            return
        }
        snippets = list
    }

    private func save() {
        // Atomic so a concurrent reader (the app or voicectl) never sees a
        // half-written file.
        if let data = try? JSONEncoder().encode(snippets) {
            try? data.write(to: fileURL, options: .atomic)
        }
        fingerprint = currentFingerprint()
    }
}
