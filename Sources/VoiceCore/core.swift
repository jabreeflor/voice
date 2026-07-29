import AppKit
import AVFoundation
import ApplicationServices
import ServiceManagement

// MARK: - Config

/// A key that can be held down on its own. These arrive as `.flagsChanged`
/// rather than `.keyDown`/`.keyUp`, and `flag` is the bit that goes high
/// while the key is down.
enum ModifierKey: Int64, CaseIterable {
    case capsLock = 57
    case leftShift = 56
    case rightShift = 60
    case leftControl = 59
    case rightControl = 62
    case leftOption = 58
    case rightOption = 61
    case leftCommand = 55
    case rightCommand = 54
    case function = 63

    var flag: CGEventFlags {
        switch self {
        case .capsLock: return .maskAlphaShift
        case .leftShift, .rightShift: return .maskShift
        case .leftControl, .rightControl: return .maskControl
        case .leftOption, .rightOption: return .maskAlternate
        case .leftCommand, .rightCommand: return .maskCommand
        case .function: return .maskSecondaryFn
        }
    }

    /// `flag` can't tell the two ⌥ keys apart, which is enough to hang a
    /// recording open: hold left ⌥, tap and release right ⌥, and
    /// `.maskAlternate` is still set by the key that never moved. Hardware
    /// events carry a second, side-specific bit (the `NX_DEVICE*` masks) that
    /// does distinguish them. `nil` for the keys that have no sibling.
    var deviceFlags: (mine: CGEventFlags, sibling: CGEventFlags)? {
        func pair(_ l: UInt64, _ r: UInt64) -> (CGEventFlags, CGEventFlags) {
            (CGEventFlags(rawValue: l), CGEventFlags(rawValue: r))
        }
        switch self {
        case .leftControl:   return pair(0x00000001, 0x00002000)
        case .rightControl:  return pair(0x00002000, 0x00000001)
        case .leftShift:     return pair(0x00000002, 0x00000004)
        case .rightShift:    return pair(0x00000004, 0x00000002)
        case .leftCommand:   return pair(0x00000008, 0x00000010)
        case .rightCommand:  return pair(0x00000010, 0x00000008)
        case .leftOption:    return pair(0x00000020, 0x00000040)
        case .rightOption:   return pair(0x00000040, 0x00000020)
        case .capsLock, .function: return nil
        }
    }

    /// Whether this specific key is down, given the flags on an event.
    /// Synthesized events (the e2e smoke test, anything driving the app from
    /// a script) usually carry no device bits at all, so their absence falls
    /// back to the shared flag rather than reporting the key as released.
    func isHeld(in flags: CGEventFlags) -> Bool {
        guard flags.contains(flag) else { return false }
        guard let (mine, sibling) = deviceFlags else { return true }
        guard flags.contains(mine) || flags.contains(sibling) else { return true }
        return flags.contains(mine)
    }

    /// "Right ⌥" — enough to recognise at a glance.
    var shortName: String {
        switch self {
        case .capsLock: return "⇪ Caps Lock"
        case .leftShift: return "Left ⇧"
        case .rightShift: return "Right ⇧"
        case .leftControl: return "Left ⌃"
        case .rightControl: return "Right ⌃"
        case .leftOption: return "Left ⌥"
        case .rightOption: return "Right ⌥"
        case .leftCommand: return "Left ⌘"
        case .rightCommand: return "Right ⌘"
        case .function: return "fn"
        }
    }

    /// "Right ⌥ Option" — the spelled-out form for status text and menus.
    var longName: String {
        switch self {
        case .capsLock: return "⇪ Caps Lock"
        case .leftShift: return "Left ⇧ Shift"
        case .rightShift: return "Right ⇧ Shift"
        case .leftControl: return "Left ⌃ Control"
        case .rightControl: return "Right ⌃ Control"
        case .leftOption: return "Left ⌥ Option"
        case .rightOption: return "Right ⌥ Option"
        case .leftCommand: return "Left ⌘ Command"
        case .rightCommand: return "Right ⌘ Command"
        case .function: return "fn"
        }
    }
}

/// Why a binding is a bad idea. `.unusable` cannot work at all and is
/// refused; `.risky` works but shadows ordinary typing, so it is offered
/// with a confirmation rather than silently accepted.
enum HotkeyIssue: Equatable {
    case unusable(String)
    case risky(String)
}

/// The hold-to-talk binding: one key, plus any modifiers that must be held
/// with it. `keyCode` may itself be a modifier key (the default, Right ⌥),
/// in which case the tap watches `.flagsChanged`; otherwise it watches
/// `.keyDown`/`.keyUp`. Any key the keyboard can produce is bindable.
struct Hotkey: Equatable {
    let keyCode: Int64
    /// Modifiers that must *also* be held. Never includes the bound key's
    /// own flag — a modifier key always sets its own bit, so requiring it
    /// separately would be redundant.
    let modifiers: CGEventFlags

    /// The flags worth comparing. Two deliberate omissions: Caps Lock is a
    /// latch rather than a held modifier, so every binding would break
    /// whenever the light was on; and macOS sets the fn bit by itself on
    /// arrows, F-keys and the navigation cluster, so requiring it would make
    /// those bindings match inconsistently. fn stays bindable as a key in
    /// its own right — it just isn't usable as a *required* modifier.
    static let significantModifiers: CGEventFlags =
        [.maskShift, .maskControl, .maskAlternate, .maskCommand]

    /// AppKit reports modifiers as `NSEvent.ModifierFlags`; the event tap
    /// speaks `CGEventFlags`. This is the bridge used when recording.
    static func flags(from ns: NSEvent.ModifierFlags) -> CGEventFlags {
        var f: CGEventFlags = []
        if ns.contains(.shift) { f.insert(.maskShift) }
        if ns.contains(.control) { f.insert(.maskControl) }
        if ns.contains(.option) { f.insert(.maskAlternate) }
        if ns.contains(.command) { f.insert(.maskCommand) }
        return f
    }

    init(keyCode: Int64, modifiers: CGEventFlags = []) {
        self.keyCode = keyCode
        var m = modifiers.intersection(Hotkey.significantModifiers)
        if let own = ModifierKey(rawValue: keyCode) { m.remove(own.flag) }
        self.modifiers = m
    }

    var modifierKey: ModifierKey? { ModifierKey(rawValue: keyCode) }
    var isModifierKey: Bool { modifierKey != nil }

    // MARK: presets

    static let rightOption = Hotkey(keyCode: ModifierKey.rightOption.rawValue)
    static let rightCommand = Hotkey(keyCode: ModifierKey.rightCommand.rawValue)
    static let fn = Hotkey(keyCode: ModifierKey.function.rawValue)

    /// The quick picks offered in onboarding. Anything else is reachable by
    /// recording a key, so this list is convenience, not the whole menu.
    static let presets: [Hotkey] = [.rightOption, .rightCommand, .fn]
    static let fallback = Hotkey.rightOption

    // MARK: labels

    var label: String {
        Hotkey.join(symbols: modifiers, with: modifierKey?.longName
                        ?? Hotkey.keyName(for: keyCode))
    }

    var shortLabel: String {
        Hotkey.join(symbols: modifiers, with: modifierKey?.shortName
                        ?? Hotkey.keyName(for: keyCode))
    }

    /// "⌃⌥D" reads fine closed up, but "⌃Right ⌥" does not — a spelled-out
    /// key name gets a space in front of it.
    private static func join(symbols flags: CGEventFlags, with name: String) -> String {
        let prefix = Hotkey.symbols(for: flags)
        if prefix.isEmpty { return name }
        return name.contains(" ") ? prefix + " " + name : prefix + name
    }

    /// Apple's canonical ordering, ⌃⌥⇧⌘. fn is absent by design: it can be
    /// the bound key but never a required modifier, so it is spelled by
    /// `keyName` rather than here.
    static func symbols(for flags: CGEventFlags) -> String {
        var s = ""
        if flags.contains(.maskControl) { s += "⌃" }
        if flags.contains(.maskAlternate) { s += "⌥" }
        if flags.contains(.maskShift) { s += "⇧" }
        if flags.contains(.maskCommand) { s += "⌘" }
        return s
    }

    /// Names for the standard ANSI virtual keycodes. Unknown codes still get
    /// a usable name so a media key or a sixth mouse thumb button on some
    /// unusual keyboard can be bound and displayed rather than rejected.
    static func keyName(for code: Int64) -> String {
        if let m = ModifierKey(rawValue: code) { return m.shortName }
        if let name = keyNames[code] { return name }
        return "Key \(code)"
    }

    private static let keyNames: [Int64: String] = [
        0: "A", 1: "S", 2: "D", 3: "F", 4: "H", 5: "G", 6: "Z", 7: "X", 8: "C",
        9: "V", 10: "§", 11: "B", 12: "Q", 13: "W", 14: "E", 15: "R", 16: "Y",
        17: "T", 18: "1", 19: "2", 20: "3", 21: "4", 22: "6", 23: "5", 24: "=",
        25: "9", 26: "7", 27: "-", 28: "8", 29: "0", 30: "]", 31: "O", 32: "U",
        33: "[", 34: "I", 35: "P", 36: "Return", 37: "L", 38: "J", 39: "'",
        40: "K", 41: ";", 42: "\\", 43: ",", 44: "/", 45: "N", 46: "M", 47: ".",
        48: "Tab", 49: "Space", 50: "`", 51: "Delete", 53: "Esc",
        64: "F17", 65: "Keypad .", 67: "Keypad *", 69: "Keypad +", 71: "Clear",
        75: "Keypad /", 76: "Keypad Enter", 78: "Keypad -", 79: "F18", 80: "F19",
        81: "Keypad =", 82: "Keypad 0", 83: "Keypad 1", 84: "Keypad 2",
        85: "Keypad 3", 86: "Keypad 4", 87: "Keypad 5", 88: "Keypad 6",
        89: "Keypad 7", 90: "F20", 91: "Keypad 8", 92: "Keypad 9",
        96: "F5", 97: "F6", 98: "F7", 99: "F3", 100: "F8", 101: "F9",
        103: "F11", 105: "F13", 106: "F16", 107: "F14", 109: "F10", 110: "Menu",
        111: "F12", 113: "F15", 114: "Help", 115: "Home", 116: "Page Up",
        117: "Forward Delete", 118: "F4", 119: "End", 120: "F2",
        121: "Page Down", 122: "F1", 123: "←", 124: "→", 125: "↓", 126: "↑",
    ]

    // MARK: validation

    /// Keys that are safe to hold on their own because nothing types them:
    /// F1–F20 and Help.
    static let standaloneSafeKeyCodes: Set<Int64> = [
        122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111,   // F1–F12
        105, 107, 113, 106, 64, 79, 80, 90,                       // F13–F20
        114,                                                      // Help
    ]

    var issue: HotkeyIssue? {
        if modifierKey == .capsLock {
            return .unusable("Caps Lock latches on and off instead of reporting when it is held, so it can't drive hold-to-talk. Pick another key.")
        }
        if isModifierKey { return nil }
        if modifiers.isEmpty && !Hotkey.standaloneSafeKeyCodes.contains(keyCode) {
            return .risky("\(shortLabel) on its own will start dictation every time you press it — including in the middle of a word. Adding ⌃ or ⌥ keeps it out of the way of typing.")
        }
        return nil
    }

    // MARK: persistence

    /// "61:0" — keycode and modifier bits. Round-trips through
    /// `UserDefaults` as a plain string so the old value is upgradable.
    var storedValue: String { "\(keyCode):\(modifiers.rawValue)" }

    /// Accepts both the current encoding and the three names written by
    /// versions that only had a fixed choice of three keys.
    init?(stored: String) {
        if let legacy = Hotkey.legacyNames[stored] {
            self = legacy
            return
        }
        let parts = stored.split(separator: ":")
        guard parts.count == 2,
              let code = Int64(parts[0]),
              let bits = UInt64(parts[1]) else { return nil }
        self.init(keyCode: code, modifiers: CGEventFlags(rawValue: bits))
    }

    private static let legacyNames: [String: Hotkey] = [
        "rightOption": .rightOption,
        "rightCommand": .rightCommand,
        "fn": .fn,
    ]
}

enum Config {
    static let serverPort: UInt16 = 8178

    /// Injectable so tests can bind a talk key without writing into the
    /// defaults of whoever is running them. Mirrors `HistoryStore`.
    static var defaults: UserDefaults = .standard

    static var hotkey: Hotkey {
        get {
            if let raw = defaults.string(forKey: "hotkey"),
               let h = Hotkey(stored: raw) { return h }
            return .fallback
        }
        set { defaults.set(newValue.storedValue, forKey: "hotkey") }
    }

    static var soundsEnabled: Bool {
        defaults.object(forKey: "sounds") == nil
            ? true : defaults.bool(forKey: "sounds")
    }

    static var trailingSpace: Bool {
        defaults.object(forKey: "trailingSpace") == nil
            ? true : defaults.bool(forKey: "trailingSpace")
    }

    static var modelsDirs: [URL] {
        let home = FileManager.default.homeDirectoryForCurrentUser
        return [
            home.appendingPathComponent("thevoice/models"),
            home.appendingPathComponent(".thevoice/models"),
        ]
    }

    // Preference order tuned for dictation latency: base.en transcribes a
    // sentence in about a second on Apple Silicon even under load, which is
    // what makes hold-speak-release feel instant. Bigger models are more
    // accurate but add seconds per utterance.
    static let preferredModels = [
        "ggml-base.en.bin", "ggml-base.bin",
        "ggml-small.en.bin", "ggml-small.bin",
        "ggml-medium.en.bin", "ggml-medium.bin",
        "ggml-large-v3-turbo.bin",
        "ggml-tiny.en.bin", "ggml-tiny.bin",
    ]

    static var selectedModelFile: String? {
        get { UserDefaults.standard.string(forKey: "modelFile") }
        set { UserDefaults.standard.set(newValue, forKey: "modelFile") }
    }

    static func findModel() -> URL? {
        let fm = FileManager.default
        if let env = ProcessInfo.processInfo.environment["THEVOICE_MODEL"] {
            let u = URL(fileURLWithPath: (env as NSString).expandingTildeInPath)
            if fm.fileExists(atPath: u.path) { return u }
        }
        if let chosen = selectedModelFile, let u = ModelCatalog.installedURL(of: chosen) {
            return u
        }
        for dir in modelsDirs {
            for name in preferredModels {
                let u = dir.appendingPathComponent(name)
                if fm.fileExists(atPath: u.path) { return u }
            }
            if let items = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil),
               let any = items.first(where: {
                   $0.lastPathComponent.hasPrefix("ggml") && $0.pathExtension == "bin"
               }) {
                return any
            }
        }
        return nil
    }
}

// MARK: - Model catalog & in-app downloader

struct ModelSpec {
    let file: String
    let label: String
}

enum ModelCatalog {
    static let all: [ModelSpec] = [
        ModelSpec(file: "ggml-tiny.en.bin", label: "Tiny — fastest (75 MB)"),
        ModelSpec(file: "ggml-base.en.bin", label: "Base — fast (142 MB)"),
        ModelSpec(file: "ggml-small.en.bin", label: "Small — balanced (466 MB)"),
        ModelSpec(file: "ggml-medium.en.bin", label: "Medium — accurate (1.5 GB)"),
        ModelSpec(file: "ggml-large-v3-turbo.bin", label: "Large v3 Turbo — best (1.6 GB)"),
    ]

    // Auto-setup default: base.en — small download, ~1s per utterance, which
    // is what makes hold-speak-release feel instant. Never surfaced in UI.
    static let defaultSpec: ModelSpec = {
        guard let spec = all.first(where: { $0.file == "ggml-base.en.bin" }) else {
            fatalError("ModelCatalog missing ggml-base.en.bin")
        }
        return spec
    }()

    static func downloadURL(for spec: ModelSpec) -> URL {
        URL(string: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/\(spec.file)")!
    }

    static func installedURL(of file: String) -> URL? {
        for dir in Config.modelsDirs {
            let u = dir.appendingPathComponent(file)
            if FileManager.default.fileExists(atPath: u.path) { return u }
        }
        return nil
    }
}

final class ModelDownloader: NSObject, URLSessionDownloadDelegate {
    // All delegate callbacks and state live on the main queue.
    private lazy var session = URLSession(configuration: .default,
                                          delegate: self,
                                          delegateQueue: OperationQueue.main)
    private var files: [Int: String] = [:]          // task id -> model file
    private(set) var progress: [String: Double] = [:] // model file -> 0..1
    var onProgress: ((String, Double) -> Void)?
    var onFinished: ((String, URL?) -> Void)?       // nil URL = failed

    func isDownloading(_ file: String) -> Bool { progress[file] != nil }

    func download(_ spec: ModelSpec) {
        guard !isDownloading(spec.file) else { return }
        let task = session.downloadTask(with: ModelCatalog.downloadURL(for: spec))
        files[task.taskIdentifier] = spec.file
        progress[spec.file] = 0
        task.resume()
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didWriteData bytesWritten: Int64, totalBytesWritten: Int64,
                    totalBytesExpectedToWrite: Int64) {
        guard let file = files[downloadTask.taskIdentifier],
              totalBytesExpectedToWrite > 0 else { return }
        let p = Double(totalBytesWritten) / Double(totalBytesExpectedToWrite)
        progress[file] = p
        onProgress?(file, p)
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didFinishDownloadingTo location: URL) {
        guard let file = files[downloadTask.taskIdentifier] else { return }
        files[downloadTask.taskIdentifier] = nil
        progress[file] = nil

        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 0
        let dir = Config.modelsDirs[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent(file)
        try? FileManager.default.removeItem(at: dest)
        var moved = false
        if status == 200 {
            moved = (try? FileManager.default.moveItem(at: location, to: dest)) != nil
        }
        onFinished?(file, moved ? dest : nil)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    didCompleteWithError error: Error?) {
        guard error != nil, let file = files[task.taskIdentifier] else { return }
        files[task.taskIdentifier] = nil
        progress[file] = nil
        onFinished?(file, nil)
    }
}

// MARK: - Audio recorder (16 kHz mono float)

final class Recorder {
    private let engine = AVAudioEngine()
    private var converter: AVAudioConverter?
    private var samples: [Float] = []
    private let lock = NSLock()
    private(set) var isRecording = false
    private var startTime: Date?
    private var _level: Float = 0

    var level: Float { _level }
    var duration: TimeInterval { startTime.map { Date().timeIntervalSince($0) } ?? 0 }

    func start() throws {
        guard !isRecording else { return }
        lock.lock(); samples.removeAll(keepingCapacity: true); lock.unlock()
        _level = 0

        let input = engine.inputNode
        let inFormat = input.outputFormat(forBus: 0)
        guard inFormat.sampleRate > 0, inFormat.channelCount > 0 else {
            throw NSError(domain: "Voice", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "No microphone input (check mic permission)."])
        }
        let outFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16000,
                                      channels: 1, interleaved: false)!
        converter = AVAudioConverter(from: inFormat, to: outFormat)

        input.installTap(onBus: 0, bufferSize: 4096, format: inFormat) { [weak self] buffer, _ in
            self?.process(buffer: buffer, outFormat: outFormat)
        }
        engine.prepare()
        try engine.start()
        isRecording = true
        startTime = Date()
    }

    private func process(buffer: AVAudioPCMBuffer, outFormat: AVAudioFormat) {
        guard let converter = converter else { return }
        let ratio = outFormat.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: outFormat, frameCapacity: capacity) else { return }

        var fed = false
        var err: NSError?
        converter.convert(to: out, error: &err) { _, status in
            if fed { status.pointee = .noDataNow; return nil }
            fed = true
            status.pointee = .haveData
            return buffer
        }
        guard err == nil, let ch = out.floatChannelData, out.frameLength > 0 else { return }

        let n = Int(out.frameLength)
        let ptr = ch[0]
        var sumSq: Float = 0
        for i in 0..<n { sumSq += ptr[i] * ptr[i] }
        let rms = sqrtf(sumSq / Float(n))
        // Smooth: fast attack, slow decay
        _level = max(min(rms * 9, 1), _level * 0.82)

        lock.lock()
        samples.append(contentsOf: UnsafeBufferPointer(start: ptr, count: n))
        lock.unlock()
    }

    func stop() -> [Float] {
        guard isRecording else { return [] }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        isRecording = false
        _level = 0
        lock.lock(); let result = samples; samples.removeAll(); lock.unlock()
        return result
    }

    func cancel() { _ = stop() }

    static func wavData(samples: [Float]) -> Data {
        let dataSize = UInt32(samples.count * 2)
        var d = Data(capacity: 44 + Int(dataSize))
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        d.append(contentsOf: Array("RIFF".utf8)); u32(36 + dataSize)
        d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1)
        u32(16000); u32(16000 * 2); u16(2); u16(16)
        d.append(contentsOf: Array("data".utf8)); u32(dataSize)
        var pcm = [Int16](repeating: 0, count: samples.count)
        for i in 0..<samples.count {
            pcm[i] = Int16(max(-1, min(1, samples[i])) * 32767)
        }
        pcm.withUnsafeBytes { d.append(contentsOf: $0) }
        return d
    }
}

// MARK: - whisper.cpp server (keeps model loaded for fast responses)

final class WhisperEngine {
    private var process: Process?
    private(set) var ready = false
    private(set) var statusText = "starting…"
    var onStatusChange: (() -> Void)?
    let modelURL: URL?
    let port: UInt16
    private let maxReadinessPollAttempts: Int

    init(modelURL: URL?, port: UInt16 = Config.serverPort, maxReadinessPollAttempts: Int = 120) {
        self.modelURL = modelURL
        self.port = port
        self.maxReadinessPollAttempts = maxReadinessPollAttempts
    }

    static func findBinary(_ name: String) -> String? {
        let candidates = [
            "/opt/homebrew/bin/\(name)",
            "/usr/local/bin/\(name)",
            "/opt/homebrew/opt/whisper-cpp/bin/\(name)",
        ]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    private func setStatus(_ s: String) {
        DispatchQueue.main.async { self.statusText = s; self.onStatusChange?() }
    }

    private func setStatusOnMain(_ s: String) {
        assert(Thread.isMainThread)
        statusText = s
        onStatusChange?()
    }

    private func terminateProcessForReadinessTimeout() {
        assert(Thread.isMainThread)
        guard let p = process else { return }
        p.terminationHandler = nil
        p.terminate()
        process = nil
        ready = false
        setStatusOnMain("engine did not start")
    }

    func start() {
        guard let model = modelURL else { setStatus("no model found"); return }
        guard let bin = Self.findBinary("whisper-server") else {
            setStatus("whisper-server not installed"); return
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = [
            "-m", model.path,
            "--host", "127.0.0.1",
            "--port", String(port),
            "-t", String(max(4, ProcessInfo.processInfo.activeProcessorCount - 2)),
            "-bs", "1",   // greedy decoding — ~2x faster than beam search
            "-nf",        // no temperature fallback — kills worst-case retries
        ]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        p.terminationHandler = { [weak self] _ in
            DispatchQueue.main.async {
                self?.ready = false
                self?.setStatus("engine stopped")
            }
        }
        do { try p.run() } catch {
            setStatus("failed to launch engine"); return
        }
        process = p
        setStatus("loading model…")
        pollUntilReady()
    }

    private func pollUntilReady() {
        DispatchQueue.global(qos: .utility).async { [weak self, port, maxReadinessPollAttempts] in
            let url = URL(string: "http://127.0.0.1:\(port)/")!
            for _ in 0..<maxReadinessPollAttempts {
                guard let self = self else { return }
                if self.process?.isRunning != true { return }
                let sem = DispatchSemaphore(value: 0)
                var ok = false
                var req = URLRequest(url: url)
                req.timeoutInterval = 1
                URLSession.shared.dataTask(with: req) { _, resp, _ in
                    ok = (resp as? HTTPURLResponse) != nil
                    sem.signal()
                }.resume()
                sem.wait()
                if ok {
                    DispatchQueue.main.async {
                        self.ready = true
                        self.setStatus("ready")
                        self.warmUp()
                    }
                    return
                }
                Thread.sleep(forTimeInterval: 0.3)
            }
            // Give up: kill the child too, or a late-binding server would be
            // left running untracked, squatting on the port forever.
            DispatchQueue.main.async { [weak self] in
                self?.terminateProcessForReadinessTimeout()
            }
        }
    }

    func stop() {
        process?.terminationHandler = nil
        process?.terminate()
        process = nil
        ready = false
    }

    /// Push a half-second of silence through the model right after load so
    /// Metal kernels are compiled before the user's first real dictation.
    private func warmUp() {
        let silence = Recorder.wavData(samples: [Float](repeating: 0, count: 8000))
        transcribeViaServer(wav: silence) { _ in }
    }

    func transcribe(wav: Data, completion: @escaping (Result<String, Error>) -> Void) {
        if ready {
            transcribeViaServer(wav: wav, completion: completion)
        } else {
            transcribeViaCLI(wav: wav, completion: completion)
        }
    }

    private func transcribeViaServer(wav: Data, completion: @escaping (Result<String, Error>) -> Void) {
        let boundary = "TheVoiceBoundary7f3a9c"
        var req = URLRequest(url: URL(string: "http://127.0.0.1:\(port)/inference")!)
        req.httpMethod = "POST"
        req.timeoutInterval = 120
        req.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        var body = Data()
        func field(_ name: String, _ value: String) {
            body.append(Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n".utf8))
        }
        field("temperature", "0.0")
        field("temperature_inc", "0.0")
        field("response_format", "json")
        body.append(Data("--\(boundary)\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n".utf8))
        body.append(wav)
        body.append(Data("\r\n--\(boundary)--\r\n".utf8))
        req.httpBody = body

        URLSession.shared.dataTask(with: req) { data, _, error in
            DispatchQueue.main.async {
                if let error = error { completion(.failure(error)); return }
                guard let data = data,
                      let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                      let text = obj["text"] as? String else {
                    completion(.failure(NSError(domain: "Voice", code: 2, userInfo: [
                        NSLocalizedDescriptionKey: "Bad response from whisper engine"])))
                    return
                }
                completion(.success(text))
            }
        }.resume()
    }

    private func transcribeViaCLI(wav: Data, completion: @escaping (Result<String, Error>) -> Void) {
        guard let model = modelURL, let bin = Self.findBinary("whisper-cli") else {
            completion(.failure(NSError(domain: "Voice", code: 3, userInfo: [
                NSLocalizedDescriptionKey: "Whisper engine not ready (brew install whisper-cpp)"])))
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let tmp = FileManager.default.temporaryDirectory
                .appendingPathComponent("thevoice-\(UUID().uuidString).wav")
            do {
                try wav.write(to: tmp)
                let p = Process()
                p.executableURL = URL(fileURLWithPath: bin)
                p.arguments = ["-m", model.path, "-f", tmp.path, "-np", "-nt", "-bs", "1", "-nf"]
                let pipe = Pipe()
                p.standardOutput = pipe
                p.standardError = FileHandle.nullDevice
                try p.run()
                let out = pipe.fileHandleForReading.readDataToEndOfFile()
                p.waitUntilExit()
                try? FileManager.default.removeItem(at: tmp)
                let text = String(data: out, encoding: .utf8) ?? ""
                DispatchQueue.main.async { completion(.success(text)) }
            } catch {
                try? FileManager.default.removeItem(at: tmp)
                DispatchQueue.main.async { completion(.failure(error)) }
            }
        }
    }
}

// MARK: - Transcript cleanup

func cleanTranscript(_ raw: String) -> String {
    var t = raw
    let artifacts = [
        "[BLANK_AUDIO]", "[INAUDIBLE]", "[MUSIC]", "[SILENCE]", "[NOISE]", "[TYPING]",
        "(blank audio)", "(silence)", "(music)", "(noise)", "(typing)",
        "[MUSIC PLAYING]", "(music playing)", "[SOUND]", "♪",
    ]
    for a in artifacts {
        t = t.replacingOccurrences(of: a, with: "", options: .caseInsensitive)
    }
    t = t.replacingOccurrences(of: "\n", with: " ")
    while t.contains("  ") { t = t.replacingOccurrences(of: "  ", with: " ") }
    return t.trimmingCharacters(in: .whitespacesAndNewlines)
}

// MARK: - Floating pill overlay (Wisprflow-style)

final class BarsView: NSView {
    enum Mode { case listening, processing }
    var mode: Mode = .listening
    var levelProvider: (() -> Float)?

    private var history: [Float] = Array(repeating: 0, count: 17)
    private var phase: CGFloat = 0
    private var timer: Timer?

    func start() {
        stop()
        history = Array(repeating: 0, count: history.count)
        phase = 0
        let t = Timer(timeInterval: 1.0 / 30.0, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func stop() { timer?.invalidate(); timer = nil }

    private func tick() {
        if mode == .listening {
            history.removeFirst()
            history.append(levelProvider?() ?? 0)
        } else {
            phase += 0.28
        }
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let n = history.count
        let barW: CGFloat = 3.5, gap: CGFloat = 4.5
        let totalW = CGFloat(n) * barW + CGFloat(n - 1) * gap
        var x = (bounds.width - totalW) / 2
        let midY = bounds.midY
        NSColor.white.withAlphaComponent(0.92).setFill()
        for i in 0..<n {
            var h: CGFloat = 3
            switch mode {
            case .listening:
                h = 3 + CGFloat(min(1, history[i])) * (bounds.height - 14)
            case .processing:
                h = 4 + (sin(phase + CGFloat(i) * 0.55) * 0.5 + 0.5) * 13
            }
            let r = NSRect(x: x, y: midY - h / 2, width: barW, height: h)
            NSBezierPath(roundedRect: r, xRadius: barW / 2, yRadius: barW / 2).fill()
            x += barW + gap
        }
    }
}

final class Overlay {
    private let panel: NSPanel
    private let bars: BarsView
    private let label: NSTextField
    private var hideWork: DispatchWorkItem?
    private let size = NSSize(width: 192, height: 44)

    init() {
        panel = NSPanel(contentRect: NSRect(origin: .zero, size: size),
                        styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: false)
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.alphaValue = 0

        let back = NSView(frame: NSRect(origin: .zero, size: size))
        back.wantsLayer = true
        back.layer?.backgroundColor = Palette.pillBlack.cgColor
        back.layer?.cornerRadius = size.height / 2
        back.layer?.masksToBounds = true
        back.layer?.borderWidth = 1
        back.layer?.borderColor = NSColor.white.withAlphaComponent(0.18).cgColor
        panel.contentView = back

        bars = BarsView(frame: NSRect(origin: .zero, size: size).insetBy(dx: 18, dy: 6))
        bars.autoresizingMask = [.width, .height]
        back.addSubview(bars)

        label = NSTextField(labelWithString: "")
        label.frame = NSRect(x: 10, y: (size.height - 20) / 2, width: size.width - 20, height: 20)
        label.alignment = .center
        label.font = .systemFont(ofSize: 13, weight: .medium)
        label.textColor = .white
        label.lineBreakMode = .byTruncatingTail
        label.isHidden = true
        back.addSubview(label)
    }

    private func position() {
        guard let screen = NSScreen.main else { return }
        let f = screen.visibleFrame
        panel.setFrameOrigin(NSPoint(x: f.midX - size.width / 2, y: f.minY + 28))
    }

    private func show() {
        hideWork?.cancel(); hideWork = nil
        position()
        panel.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.15
            panel.animator().alphaValue = 1
        }
    }

    func showListening(levelProvider: @escaping () -> Float) {
        label.isHidden = true
        bars.isHidden = false
        bars.mode = .listening
        bars.levelProvider = levelProvider
        bars.start()
        show()
    }

    func showProcessing() {
        label.isHidden = true
        bars.isHidden = false
        bars.mode = .processing
        bars.start()
        show()
    }

    func flash(_ message: String, duration: TimeInterval = 1.1) {
        bars.stop()
        bars.isHidden = true
        label.stringValue = message
        label.isHidden = false
        show()
        let work = DispatchWorkItem { [weak self] in self?.hide() }
        hideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + duration, execute: work)
    }

    func hide() {
        hideWork?.cancel(); hideWork = nil
        bars.stop()
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.18
            panel.animator().alphaValue = 0
        }, completionHandler: { [weak panel] in
            panel?.orderOut(nil)
        })
    }
}

// MARK: - Global hotkey (hold-to-talk) via event tap

private func tapCallback(proxy: CGEventTapProxy, type: CGEventType,
                         event: CGEvent, refcon: UnsafeMutableRawPointer?) -> Unmanaged<CGEvent>? {
    guard let refcon = refcon else { return Unmanaged.passUnretained(event) }
    let controller = Unmanaged<HotkeyController>.fromOpaque(refcon).takeUnretainedValue()
    return controller.handle(type: type, event: event)
}

final class HotkeyController {
    var onDown: (() -> Void)?
    var onUp: (() -> Void)?
    var onCancel: (() -> Void)?
    var isActive: (() -> Bool)?   // is a recording in progress

    private var tap: CFMachPort?
    private var hotkeyHeld = false
    /// The keycode of the non-modifier key currently being held as the
    /// hotkey, so its `.keyUp` can be swallowed by the same rule that
    /// swallowed its `.keyDown`.
    private var heldKeyCode: Int64?
    /// Whether the bound modifier key is physically down. Tracked separately
    /// from `hotkeyHeld` because a binding like "⌃ Right ⌥" also depends on
    /// modifiers that change state in events of their own.
    private var boundKeyDown = false
    /// The binding the current press was matched against. Without this, a
    /// binding that changes mid-hold strands `hotkeyHeld`: the branch that
    /// set it may no longer be reachable to clear it.
    private var activeBinding: Hotkey?

    /// Set while the user is choosing a new talk key. The tap stays
    /// installed — tearing it down and rebuilding it would need Accessibility
    /// to still be granted — but it stops acting on what it sees, so pressing
    /// the current talk key to *record over it* doesn't also start dictating.
    var suspended = false {
        didSet { if suspended { forgetPress() } }
    }

    var tapRunning: Bool { tap != nil }

    func startTap() -> Bool {
        guard tap == nil else { return true }
        // `.keyUp` matters as soon as a non-modifier key can be the hotkey:
        // a plain key reports its release there and nowhere else.
        let mask: CGEventMask =
            (1 << CGEventType.flagsChanged.rawValue)
            | (1 << CGEventType.keyDown.rawValue)
            | (1 << CGEventType.keyUp.rawValue)
        let refcon = Unmanaged.passUnretained(self).toOpaque()
        guard let t = CGEvent.tapCreate(tap: .cgSessionEventTap,
                                        place: .headInsertEventTap,
                                        options: .defaultTap,
                                        eventsOfInterest: mask,
                                        callback: tapCallback,
                                        userInfo: refcon) else {
            return false
        }
        tap = t
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, t, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: t, enable: true)
        return true
    }

    func handle(type: CGEventType, event: CGEvent) -> Unmanaged<CGEvent>? {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap = tap { CGEvent.tapEnable(tap: tap, enable: true) }
            return Unmanaged.passUnretained(event)
        }
        if suspended { return Unmanaged.passUnretained(event) }

        // Our own ⌘V from `pasteText` re-enters this tap. Left alone it
        // cancels a recording that started while the previous one was still
        // pasting — and if the user has bound ⌘V as their talk key, it
        // matches the binding and the paste never lands at all.
        if event.getIntegerValueField(.eventSourceUserData) == HotkeyController.selfPostedTag {
            return Unmanaged.passUnretained(event)
        }

        let hk = Config.hotkey
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)

        // A binding changed while its key was still down: the branch that
        // would clear the press may no longer be reachable, so drop it here.
        if hotkeyHeld, activeBinding != hk {
            forgetPress()
            DispatchQueue.main.async { self.onCancel?() }
        }

        if type == .flagsChanged {
            guard let mod = hk.modifierKey else {
                return Unmanaged.passUnretained(event)
            }
            // Only events for the bound key itself say whether *it* is down —
            // the shared flag can't tell one side of the keyboard from the
            // other, and a synthesized event carries no side bits at all.
            if keyCode == hk.keyCode { boundKeyDown = mod.isHeld(in: event.flags) }
            // But a required modifier changes state in its own event, so the
            // verdict has to be recomputed on every flags change, not just on
            // the bound key's. Otherwise "⌃ Right ⌥" would only fire when ⌥
            // happened to be pressed last.
            let pressed = boundKeyDown
                && event.flags.intersection(hk.modifiers) == hk.modifiers
            if pressed && !hotkeyHeld {
                hotkeyHeld = true
                activeBinding = hk
                DispatchQueue.main.async { self.onDown?() }
            } else if !pressed && hotkeyHeld {
                forgetPress()
                DispatchQueue.main.async { self.onUp?() }
            }
            return Unmanaged.passUnretained(event)
        }

        // A non-modifier hotkey lives on .keyDown/.keyUp. Match it before the
        // cancel rule below, which would otherwise read the hotkey's own
        // press as the user reaching for a keyboard shortcut.
        if !hk.isModifierKey {
            if type == .keyDown, keyCode == hk.keyCode, matches(hk, event.flags) {
                // Holding a key long enough to speak generates a stream of
                // repeats; only the first one is a press.
                if event.getIntegerValueField(.keyboardEventAutorepeat) != 0 {
                    return nil
                }
                if !hotkeyHeld {
                    hotkeyHeld = true
                    heldKeyCode = keyCode
                    activeBinding = hk
                    DispatchQueue.main.async { self.onDown?() }
                }
                // Swallowed: the talk key shouldn't also type into whatever
                // is in front of the user.
                return nil
            }
        }
        // Outside the `isModifierKey` check: a key captured under the old
        // binding must still be able to release itself.
        if type == .keyUp, keyCode == heldKeyCode {
            forgetPress()
            DispatchQueue.main.async { self.onUp?() }
            return nil
        }

        if type == .keyDown, isActive?() == true {
            if keyCode == 53 { // Esc cancels dictation and is swallowed
                DispatchQueue.main.async { self.onCancel?() }
                return nil
            }
            // Any other key while holding the hotkey = the user is doing a
            // keyboard shortcut, not dictating. Cancel and let it through.
            DispatchQueue.main.async { self.onCancel?() }
        }
        return Unmanaged.passUnretained(event)
    }

    /// The modifiers held must be exactly the ones the binding asks for, so
    /// that ⌥D doesn't also fire on ⇧⌥D.
    private func matches(_ hk: Hotkey, _ flags: CGEventFlags) -> Bool {
        flags.intersection(Hotkey.significantModifiers) == hk.modifiers
    }

    private func forgetPress() {
        hotkeyHeld = false
        heldKeyCode = nil
        boundKeyDown = false
        activeBinding = nil
    }

    /// Stamped on the events `pasteText` posts so this tap can recognise its
    /// own handiwork. Any value that isn't zero will do.
    static let selfPostedTag: Int64 = 0x564F4943   // "VOIC"
}

// MARK: - Recording a new binding

/// Listens for the next keypress inside the app and turns it into a
/// `Hotkey`. A local monitor is enough — the settings window is frontmost
/// while recording — so choosing a key never depends on Accessibility being
/// granted first.
///
/// Two commit rules, matching how every other shortcut recorder on the
/// platform behaves: a regular key commits the moment it goes down, taking
/// whatever modifiers are held with it; a modifier pressed on its own
/// commits when it is released, which is what leaves room to hold ⌃⌥ and
/// then reach for a letter.
final class HotkeyRecorder {
    private var monitor: Any?
    private var resignObserver: NSObjectProtocol?
    private var pending: (keyCode: Int64, flags: CGEventFlags)?

    /// Fired on every arm and disarm. The owner uses it to suspend the global
    /// tap for exactly as long as the recorder is listening — routed through
    /// here rather than through the call sites so that every way out,
    /// including the deactivation backstop, is covered by construction.
    var onArmedChange: ((Bool) -> Void)?

    var isRecording: Bool { monitor != nil }

    /// `onResult` gets nil when the user pressed Esc to back out.
    func start(_ onResult: @escaping (Hotkey?) -> Void) {
        stop()
        monitor = NSEvent.addLocalMonitorForEvents(
            matching: [.keyDown, .flagsChanged]
        ) { [weak self] event in
            guard let self = self else { return event }
            let code = Int64(event.keyCode)
            let flags = Hotkey.flags(from: event.modifierFlags)

            if event.type == .keyDown {
                self.finish()
                onResult(code == 53 ? nil : Hotkey(keyCode: code, modifiers: flags))
                return nil
            }

            guard let mod = ModifierKey(rawValue: code) else { return nil }
            // `modifierFlags` carries the side-specific bits in its low
            // bits, and the shared ones at the same positions CoreGraphics
            // uses — so the same held/released test works on both. Asking
            // only about the shared bit would report right ⌥ as still down
            // while left ⌥ happens to be held, and the recorder would never
            // commit.
            if mod.isHeld(in: CGEventFlags(rawValue: UInt64(event.modifierFlags.rawValue))) {
                self.pending = (code, flags)
            } else if self.pending?.keyCode == code {
                let held = self.pending?.flags ?? []
                self.finish()
                onResult(Hotkey(keyCode: code, modifiers: held))
            }
            return nil
        }
        // An armed recorder swallows every keystroke in the app, so it must
        // never outlive the window that armed it. The explicit stops on tab
        // switch and window close are the normal path; this is the backstop
        // for any way out of the window nobody thought of.
        resignObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didResignActiveNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.stop()
            onResult(nil)
        }
        onArmedChange?(true)
    }

    func stop() {
        let wasArmed = monitor != nil
        if let m = monitor { NSEvent.removeMonitor(m) }
        monitor = nil
        if let o = resignObserver { NotificationCenter.default.removeObserver(o) }
        resignObserver = nil
        pending = nil
        if wasArmed { onArmedChange?(false) }
    }

    private func finish() { stop() }

    deinit { stop() }
}

extension ModifierKey {
    /// The AppKit spelling of `flag`, for events that arrive as `NSEvent`.
    var appKitFlag: NSEvent.ModifierFlags {
        switch self {
        case .capsLock: return .capsLock
        case .leftShift, .rightShift: return .shift
        case .leftControl, .rightControl: return .control
        case .leftOption, .rightOption: return .option
        case .leftCommand, .rightCommand: return .command
        case .function: return .function
        }
    }
}

// MARK: - Paste into frontmost app

func pasteText(_ text: String) {
    let pb = NSPasteboard.general
    let saved = pb.string(forType: .string)
    pb.clearContents()
    pb.setString(text, forType: .string)

    let src = CGEventSource(stateID: .combinedSessionState)
    let vDown = CGEvent(keyboardEventSource: src, virtualKey: 9, keyDown: true)
    let vUp = CGEvent(keyboardEventSource: src, virtualKey: 9, keyDown: false)
    vDown?.flags = .maskCommand
    vUp?.flags = .maskCommand
    // Marked as ours: these come back around through the global tap, where
    // an unmarked ⌘V would cancel a recording — or, if the user has bound
    // ⌘V as their talk key, be swallowed so the paste never happens.
    vDown?.setIntegerValueField(.eventSourceUserData, value: HotkeyController.selfPostedTag)
    vUp?.setIntegerValueField(.eventSourceUserData, value: HotkeyController.selfPostedTag)
    vDown?.post(tap: .cghidEventTap)
    vUp?.post(tap: .cghidEventTap)

    if let saved = saved {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
            pb.clearContents()
            pb.setString(saved, forType: .string)
        }
    }
}

// MARK: - Entry point

@MainActor
public enum VoiceMain {
    public static func run() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)

        // Minimal main menu so ⌘Q/⌘W/⌘M work while the Voice window is focused.
        let mainMenu = NSMenu()
        let appMenuItem = NSMenuItem()
        mainMenu.addItem(appMenuItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit Voice",
                        action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        mainMenu.setSubmenu(appMenu, for: appMenuItem)
        let windowMenuItem = NSMenuItem()
        mainMenu.addItem(windowMenuItem)
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Close",
                           action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: "Minimize",
                           action: #selector(NSWindow.miniaturize(_:)), keyEquivalent: "m")
        mainMenu.setSubmenu(windowMenu, for: windowMenuItem)
        app.mainMenu = mainMenu

        app.run()
    }
}
