import AppKit
import AVFoundation
import ApplicationServices
import ServiceManagement

// MARK: - Status model

struct StatusInfo {
    let text: String
    let color: NSColor
    let needsAccessibility: Bool
}

// MARK: - App delegate

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private let recorder = Recorder()
    private let overlay = Overlay()
    private let hotkeys = HotkeyController()
    private let downloader = ModelDownloader()
    private var engine: WhisperEngine?

    let historyStore = HistoryStore()
    let snippetStore = SnippetStore()

    private var mainWindow: MainWindow!
    private var onboarding: OnboardingWindow?

    private var setupProgress: Double?
    private var setupFailed = false

    private enum State { case idle, recording, transcribing }
    private var state: State = .idle
    private var previewActive = false

    private var titleMenuItem: NSMenuItem!
    private var statusMenuItem: NSMenuItem!
    private var copyLastItem: NSMenuItem!
    private var axRetryTimer: Timer?

    var hotkeysRunning: Bool { hotkeys.tapRunning }
    var mainWindowVisible: Bool { mainWindow?.isVisible ?? false }

    func applicationDidFinishLaunching(_ notification: Notification) {
        downloader.onProgress = { [weak self] _, p in
            self?.setupProgress = p
            self?.refreshUI()
        }
        downloader.onFinished = { [weak self] _, url in
            guard let self = self else { return }
            self.setupProgress = nil
            if let url = url { self.startEngine(with: url) }
            else { self.setupFailed = true }
            self.refreshUI()
        }

        setupStatusItem()
        mainWindow = MainWindow(app: self)

        if let model = Config.findModel() {
            startEngine(with: model)
        } else {
            setupProgress = 0
            downloader.download(ModelCatalog.defaultSpec)
        }

        hotkeys.onDown = { [weak self] in self?.beginRecording() }
        hotkeys.onUp = { [weak self] in self?.endRecording() }
        hotkeys.onCancel = { [weak self] in self?.cancelRecording() }
        hotkeys.isActive = { [weak self] in self?.state == .recording }

        if UserDefaults.standard.bool(forKey: "onboarded") {
            AVCaptureDevice.requestAccess(for: .audio) { _ in }
            ensureEventTap(prompt: true)
            mainWindow.show()
        } else {
            // First run: the onboarding flow drives permission prompts itself.
            ensureEventTap(prompt: false)
            showOnboarding()
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication,
                                       hasVisibleWindows: Bool) -> Bool {
        if let ob = onboarding, ob.isVisible { ob.show() }
        else if !UserDefaults.standard.bool(forKey: "onboarded") { showOnboarding() }
        else { mainWindow.show() }
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        engine?.stop()
    }

    // MARK: windows

    func showMainWindow() { mainWindow.show() }

    func showOnboarding() {
        if onboarding == nil { onboarding = OnboardingWindow(app: self) }
        onboarding?.show()
    }

    // MARK: engine

    private func startEngine(with url: URL) {
        engine?.stop()
        let e = WhisperEngine(modelURL: url)
        e.onStatusChange = { [weak self] in self?.refreshUI() }
        engine = e
        e.start()
        refreshUI()
    }

    // MARK: status

    private var recentlyAutoRelaunched: Bool {
        Date().timeIntervalSince1970 - UserDefaults.standard.double(forKey: "lastAXRelaunch") < 600
    }

    func statusInfo() -> StatusInfo {
        if AVCaptureDevice.authorizationStatus(for: .audio) == .denied {
            return StatusInfo(text: "Microphone access is off — enable it in System Settings, Privacy & Security",
                              color: .systemRed, needsAccessibility: false)
        }
        if !hotkeys.tapRunning {
            if AXIsProcessTrusted() {
                if recentlyAutoRelaunched {
                    return StatusInfo(text: "Permission granted but blocked — toggle Voice off and on in Accessibility settings",
                                      color: .systemOrange, needsAccessibility: true)
                }
                return StatusInfo(text: "Permission granted — restarting Voice to apply it",
                                  color: .systemOrange, needsAccessibility: false)
            }
            return StatusInfo(text: "Grant Accessibility permission to enable the talk key. Already listed? Toggle Voice off and on.",
                              color: .systemOrange, needsAccessibility: true)
        }
        if let p = setupProgress {
            return StatusInfo(text: "Setting up — downloading the speech engine (\(Int(p * 100))%)",
                              color: .systemOrange, needsAccessibility: false)
        }
        if setupFailed {
            return StatusInfo(text: "Setup failed — check your connection and relaunch Voice",
                              color: .systemRed, needsAccessibility: false)
        }
        guard let engine = engine else {
            return StatusInfo(text: "Preparing", color: .systemOrange, needsAccessibility: false)
        }
        if engine.ready {
            return StatusInfo(text: "Ready — hold \(Config.hotkey.label) and speak",
                              color: .systemGreen, needsAccessibility: false)
        }
        if engine.statusText == "whisper-server not installed" {
            return StatusInfo(text: "Speech engine missing — run: brew install whisper-cpp",
                              color: .systemRed, needsAccessibility: false)
        }
        return StatusInfo(text: "Starting the speech engine",
                          color: .systemOrange, needsAccessibility: false)
    }

    func refreshUI() {
        refreshMenu()
        if mainWindow != nil, mainWindow.isVisible { mainWindow.refresh() }
    }

    // MARK: permissions

    func requestAccessibility() {
        let opts = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(opts)
    }

    private func ensureEventTap(prompt: Bool) {
        if prompt { requestAccessibility() }
        if hotkeys.startTap() {
            axRetryTimer?.invalidate(); axRetryTimer = nil
            refreshUI()
        } else if axRetryTimer == nil {
            axRetryTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
                guard let self = self else { return }
                if self.hotkeys.startTap() {
                    self.axRetryTimer?.invalidate(); self.axRetryTimer = nil
                    self.refreshUI()
                } else if AXIsProcessTrusted() {
                    self.autoRelaunchIfNeeded()
                    self.refreshUI()
                }
            }
        }
    }

    private func autoRelaunchIfNeeded() {
        guard !recentlyAutoRelaunched else { return }
        // Never yank the app out from under the onboarding flow.
        guard onboarding?.isVisible != true else { return }
        UserDefaults.standard.set(Date().timeIntervalSince1970, forKey: "lastAXRelaunch")
        let path = Bundle.main.bundlePath
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "sleep 0.7; /usr/bin/open -n \"\(path)\""]
        try? p.run()
        NSApp.terminate(nil)
    }

    // MARK: mic preview

    var micLevel: Float { recorder.level }

    func previewMic() {
        guard state == .idle, !previewActive else { return }
        do { try recorder.start() } catch {
            overlay.flash("Mic error: \(error.localizedDescription)", duration: 2.0)
            return
        }
        previewActive = true
        overlay.showListening { [weak self] in self?.recorder.level ?? 0 }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.0) { [weak self] in
            guard let self = self, self.previewActive else { return }
            self.previewActive = false
            self.recorder.cancel()
            self.overlay.hide()
        }
    }

    // MARK: recording flow

    private func beginRecording() {
        guard state == .idle, !previewActive else { return }
        guard engine != nil else {
            overlay.flash("Voice is still setting up", duration: 1.4)
            return
        }
        do { try recorder.start() } catch {
            overlay.flash("Mic error: \(error.localizedDescription)", duration: 2.0)
            return
        }
        state = .recording
        setIcon(recording: true)
        if Config.soundsEnabled { NSSound(named: "Tink")?.play() }
        overlay.showListening { [weak self] in self?.recorder.level ?? 0 }
    }

    private func endRecording() {
        guard state == .recording else { return }
        let duration = recorder.duration
        let samples = recorder.stop()
        setIcon(recording: false)

        guard duration >= 0.35, samples.count > 4000 else {
            state = .idle
            overlay.hide()
            return
        }

        state = .transcribing
        overlay.showProcessing()
        let wav = Recorder.wavData(samples: samples)
        let sentAt = Date()

        engine?.transcribe(wav: wav) { [weak self] result in
            guard let self = self else { return }
            self.state = .idle
            switch result {
            case .failure(let error):
                self.overlay.flash("Error: \(error.localizedDescription)", duration: 2.2)
            case .success(let raw):
                let cleaned = cleanTranscript(raw)
                guard !cleaned.isEmpty else {
                    self.overlay.flash("No speech detected")
                    return
                }
                let text = self.snippetStore.expand(cleaned)
                let latency = Date().timeIntervalSince(sentAt)
                self.historyStore.add(DictationEntry(text: text, date: Date(),
                                                     duration: duration, latency: latency))
                pasteText(Config.trailingSpace ? text + " " : text)
                if Config.soundsEnabled { NSSound(named: "Pop")?.play() }
                let snippet = text.count > 24 ? String(text.prefix(24)) + "…" : text
                self.overlay.flash("Typed: \(snippet)")
                self.onboarding?.dictationLanded()
                self.refreshUI()
            }
        }
    }

    private func cancelRecording() {
        guard state == .recording else { return }
        recorder.cancel()
        state = .idle
        setIcon(recording: false)
        overlay.hide()
    }

    // MARK: status item / menu

    private func setupStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        setIcon(recording: false)

        let menu = NSMenu()
        menu.autoenablesItems = false

        titleMenuItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        titleMenuItem.isEnabled = false
        menu.addItem(titleMenuItem)

        statusMenuItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)

        menu.addItem(.separator())

        let openItem = NSMenuItem(title: "Open Voice",
                                  action: #selector(openMainWindowAction), keyEquivalent: "")
        openItem.target = self
        menu.addItem(openItem)

        copyLastItem = NSMenuItem(title: "Copy Last Dictation",
                                  action: #selector(copyLast), keyEquivalent: "")
        copyLastItem.target = self
        copyLastItem.isEnabled = false
        menu.addItem(copyLastItem)

        let setupItem = NSMenuItem(title: "Setup Assistant…",
                                   action: #selector(replayOnboarding), keyEquivalent: "")
        setupItem.target = self
        menu.addItem(setupItem)

        menu.addItem(.separator())
        menu.addItem(NSMenuItem(title: "Quit Voice",
                                action: #selector(NSApplication.terminate(_:)),
                                keyEquivalent: "q"))

        statusItem.menu = menu
        refreshMenu()
    }

    private func refreshMenu() {
        guard titleMenuItem != nil else { return }
        titleMenuItem.title = "Voice — hold \(Config.hotkey.label) to dictate"
        statusMenuItem.title = statusInfo().text
        copyLastItem.isEnabled = !historyStore.entries.isEmpty
    }

    private func setIcon(recording: Bool) {
        guard let button = statusItem.button else { return }
        let name = recording ? "record.circle.fill" : "waveform.circle"
        let img = NSImage(systemSymbolName: name, accessibilityDescription: "Voice")
        img?.isTemplate = !recording
        button.image = img
        button.contentTintColor = recording ? .systemRed : nil
    }

    // MARK: menu actions

    @objc private func openMainWindowAction() { mainWindow.show() }

    @objc private func replayOnboarding() { showOnboarding() }

    @objc private func copyLast() {
        guard let last = historyStore.entries.first?.text else { return }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(last, forType: .string)
    }
}
