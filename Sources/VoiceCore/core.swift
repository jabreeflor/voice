import AppKit
import AVFoundation
import ApplicationServices
import ServiceManagement

// MARK: - Config

enum Hotkey: String, CaseIterable {
    case rightOption, rightCommand, fn

    var keyCode: Int64 {
        switch self {
        case .rightOption: return 61
        case .rightCommand: return 54
        case .fn: return 63
        }
    }

    var flag: CGEventFlags {
        switch self {
        case .rightOption: return .maskAlternate
        case .rightCommand: return .maskCommand
        case .fn: return .maskSecondaryFn
        }
    }

    var label: String {
        switch self {
        case .rightOption: return "Right ⌥ Option"
        case .rightCommand: return "Right ⌘ Command"
        case .fn: return "fn"
        }
    }

    var shortLabel: String {
        switch self {
        case .rightOption: return "Right ⌥"
        case .rightCommand: return "Right ⌘"
        case .fn: return "fn"
        }
    }
}

enum Config {
    static let serverPort: UInt16 = 8178

    static var hotkey: Hotkey {
        get {
            if let raw = UserDefaults.standard.string(forKey: "hotkey"),
               let h = Hotkey(rawValue: raw) { return h }
            return .rightOption
        }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "hotkey") }
    }

    static var soundsEnabled: Bool {
        UserDefaults.standard.object(forKey: "sounds") == nil
            ? true : UserDefaults.standard.bool(forKey: "sounds")
    }

    static var trailingSpace: Bool {
        UserDefaults.standard.object(forKey: "trailingSpace") == nil
            ? true : UserDefaults.standard.bool(forKey: "trailingSpace")
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
    static let defaultSpec = all[1]

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

    init(modelURL: URL?, port: UInt16 = Config.serverPort) {
        self.modelURL = modelURL
        self.port = port
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
        DispatchQueue.global(qos: .utility).async { [weak self, port] in
            let url = URL(string: "http://127.0.0.1:\(port)/")!
            for _ in 0..<120 {
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
            if let self = self {
                self.process?.terminate()
                self.process = nil
                self.setStatus("engine did not start")
            }
        }
    }

    func stop() {
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

    var tapRunning: Bool { tap != nil }

    func startTap() -> Bool {
        guard tap == nil else { return true }
        let mask: CGEventMask =
            (1 << CGEventType.flagsChanged.rawValue) | (1 << CGEventType.keyDown.rawValue)
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
        let hk = Config.hotkey

        if type == .flagsChanged {
            let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
            if keyCode == hk.keyCode {
                let pressed = event.flags.contains(hk.flag)
                if pressed && !hotkeyHeld {
                    hotkeyHeld = true
                    DispatchQueue.main.async { self.onDown?() }
                } else if !pressed && hotkeyHeld {
                    hotkeyHeld = false
                    DispatchQueue.main.async { self.onUp?() }
                }
            }
        } else if type == .keyDown, isActive?() == true {
            let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
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
