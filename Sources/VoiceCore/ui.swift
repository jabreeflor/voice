import AppKit
import AVFoundation
import ServiceManagement

// MARK: - Design system

enum Palette {
    static let bg = NSColor(red: 0.969, green: 0.955, blue: 0.925, alpha: 1)
    static let paper = NSColor(red: 1.0, green: 0.992, blue: 0.965, alpha: 1)
    static let ink = NSColor(red: 0.129, green: 0.125, blue: 0.110, alpha: 1)
    static let inkSoft = NSColor(red: 0.129, green: 0.125, blue: 0.110, alpha: 0.60)
    static let faint = NSColor(red: 0.129, green: 0.125, blue: 0.110, alpha: 0.36)
    static let dash = NSColor(red: 0.129, green: 0.125, blue: 0.110, alpha: 0.18)
    static let lav = NSColor(red: 0.898, green: 0.831, blue: 0.976, alpha: 1)
    static let pillBlack = NSColor(red: 0.086, green: 0.086, blue: 0.070, alpha: 1)
    static let green = NSColor(red: 0.184, green: 0.478, blue: 0.298, alpha: 1)
}

func serifFont(ofSize size: CGFloat, weight: NSFont.Weight, italic: Bool = false) -> NSFont {
    let base = NSFont.systemFont(ofSize: size, weight: weight)
    var desc = base.fontDescriptor
    if let serif = desc.withDesign(.serif) { desc = serif }
    if italic { desc = desc.withSymbolicTraits(.italic) }
    return NSFont(descriptor: desc, size: size) ?? base
}

func makeLabel(_ text: String, size: CGFloat, weight: NSFont.Weight = .regular,
               color: NSColor = Palette.ink, serif: Bool = false, mono: Bool = false) -> NSTextField {
    let l = NSTextField(labelWithString: text)
    if mono { l.font = .monospacedSystemFont(ofSize: size, weight: weight) }
    else if serif { l.font = serifFont(ofSize: size, weight: weight) }
    else { l.font = .systemFont(ofSize: size, weight: weight) }
    l.textColor = color
    return l
}

func microcaps(_ text: String) -> NSTextField {
    let l = NSTextField(labelWithString: text.uppercased())
    l.font = .systemFont(ofSize: 10.5, weight: .bold)
    l.textColor = Palette.faint
    if let f = l.font {
        l.attributedStringValue = NSAttributedString(string: text.uppercased(), attributes: [
            .font: f, .foregroundColor: Palette.faint, .kern: 1.5])
    }
    return l
}

func vstack(_ views: [NSView] = [], spacing: CGFloat = 10) -> NSStackView {
    let s = NSStackView(views: views)
    s.orientation = .vertical
    s.alignment = .leading
    s.spacing = spacing
    return s
}

func hstack(_ views: [NSView] = [], spacing: CGFloat = 10) -> NSStackView {
    let s = NSStackView(views: views)
    s.orientation = .horizontal
    s.alignment = .centerY
    s.spacing = spacing
    return s
}

// MARK: - Sticker card (ink outline, no offset accent)

final class StickerCard: NSView {
    var cardFill = Palette.paper { didSet { needsDisplay = true } }
    var cornerRadius: CGFloat = 16 { didSet { needsDisplay = true } }
    let content = NSView()

    init(padding: NSEdgeInsets = NSEdgeInsets(top: 14, left: 18, bottom: 14, right: 18)) {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        content.translatesAutoresizingMaskIntoConstraints = false
        addSubview(content)
        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo: topAnchor, constant: padding.top),
            content.leadingAnchor.constraint(equalTo: leadingAnchor, constant: padding.left),
            content.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -padding.right),
            content.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -padding.bottom),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }

    override func draw(_ dirtyRect: NSRect) {
        let card = NSRect(x: 0.75, y: 0.75,
                          width: bounds.width - 1.5, height: bounds.height - 1.5)
        let p = NSBezierPath(roundedRect: card, xRadius: cornerRadius, yRadius: cornerRadius)
        cardFill.setFill(); p.fill()
        Palette.ink.setStroke(); p.lineWidth = 1.5; p.stroke()
    }
}

// MARK: - Capsule button

final class CapsuleButton: NSButton {
    enum Style { case ink, lav, ghost, granted }
    private var style: Style = .ink
    private var baseTitle = ""
    private var storedAction: Selector?

    /// Instead of `isEnabled` (whose cell dims the title into a gray blob),
    /// an inert capsule keeps full control of its look: muted outline, no action.
    var actionable = true {
        didSet {
            action = actionable ? storedAction : nil
            restyle()
        }
    }

    init(_ title: String, style: Style, target: AnyObject?, action: Selector?) {
        super.init(frame: .zero)
        self.target = target
        self.action = action
        self.storedAction = action
        isBordered = false
        wantsLayer = true
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 34).isActive = true
        apply(style, title: title)
    }
    required init?(coder: NSCoder) { fatalError() }

    func apply(_ style: Style, title: String) {
        self.style = style
        self.baseTitle = title
        restyle()
    }

    private func restyle() {
        layer?.cornerRadius = 17
        var color = NSColor.white
        if !actionable && style != .granted {
            layer?.backgroundColor = NSColor.clear.cgColor
            layer?.borderWidth = 1.5
            layer?.borderColor = Palette.dash.cgColor
            color = Palette.faint
        } else {
            switch style {
            case .ink:
                layer?.backgroundColor = Palette.ink.cgColor
                layer?.borderWidth = 0
                color = .white
            case .lav:
                layer?.backgroundColor = Palette.lav.cgColor
                layer?.borderWidth = 1.5
                layer?.borderColor = Palette.ink.cgColor
                color = Palette.ink
            case .ghost:
                layer?.backgroundColor = .clear
                layer?.borderWidth = 0
                color = Palette.inkSoft
            case .granted:
                layer?.backgroundColor = Palette.paper.cgColor
                layer?.borderWidth = 1.5
                layer?.borderColor = Palette.green.cgColor
                color = Palette.green
            }
        }
        attributedTitle = NSAttributedString(string: baseTitle, attributes: [
            .font: NSFont.systemFont(ofSize: 13, weight: .semibold),
            .foregroundColor: color])
        invalidateIntrinsicContentSize()
    }

    override var intrinsicContentSize: NSSize {
        var s = super.intrinsicContentSize
        s.width += 40   // 20pt of breathing room each side
        s.height = 34
        return s
    }
}

// MARK: - Waveform glyph

final class MiniWave: NSView {
    var barWidth: CGFloat = 4
    var gap: CGFloat = 4
    var maxBar: CGFloat = 22
    var count = 7
    var color = NSColor.white
    var animated = true
    private var phase: CGFloat = 0
    private var timer: Timer?
    private let idle: [CGFloat] = [0.35, 0.6, 0.9, 1.0, 0.65, 0.85, 0.45]

    override var intrinsicContentSize: NSSize {
        NSSize(width: CGFloat(count) * barWidth + CGFloat(count - 1) * gap, height: maxBar)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        timer?.invalidate(); timer = nil
        if window != nil, animated {
            let t = Timer(timeInterval: 1.0 / 24.0, repeats: true) { [weak self] _ in
                self?.phase += 0.14
                self?.needsDisplay = true
            }
            RunLoop.main.add(t, forMode: .common)
            timer = t
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        color.setFill()
        var x: CGFloat = 0
        for i in 0..<count {
            var f = idle[i % idle.count]
            if animated { f *= 0.55 + 0.45 * (sin(phase + CGFloat(i) * 0.9) * 0.5 + 0.5) }
            let h = max(3, maxBar * f)
            let r = NSRect(x: x, y: (bounds.height - h) / 2, width: barWidth, height: h)
            NSBezierPath(roundedRect: r, xRadius: barWidth / 2, yRadius: barWidth / 2).fill()
            x += barWidth + gap
        }
    }
}

// MARK: - Small pieces

final class DashedLine: NSView {
    init() {
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 1.5).isActive = true
    }
    required init?(coder: NSCoder) { fatalError() }
    override func draw(_ dirtyRect: NSRect) {
        let p = NSBezierPath()
        p.move(to: NSPoint(x: 0, y: 0.75))
        p.line(to: NSPoint(x: bounds.width, y: 0.75))
        p.setLineDash([4, 4], count: 2, phase: 0)
        p.lineWidth = 1.5
        Palette.dash.setStroke()
        p.stroke()
    }
}

class HoverRow: NSView {
    var onHover: ((Bool) -> Void)?
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero,
            options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self, userInfo: nil))
    }
    override func mouseEntered(with event: NSEvent) { onHover?(true) }
    override func mouseExited(with event: NSEvent) { onHover?(false) }
}

/// Dictation history row: the whole row copies on click. No capsule button —
/// hover tints the row and fades in a text hint; a successful copy swaps the
/// hint to "Copied" and holds a stronger tint briefly.
final class LedgerRow: NSView {
    private let timeLabel: NSTextField
    private let bodyLabel: NSTextField
    private let hintLabel: NSTextField
    private let text: String
    private var hovered = false { didSet { updateChrome() } }
    private var copied = false { didSet { updateChrome() } }
    private var copiedReset: DispatchWorkItem?
    private var pressed = false

    init(time: String, text: String) {
        self.text = text
        timeLabel = makeLabel(time.uppercased(), size: 11, weight: .semibold,
                              color: Palette.faint, mono: true)
        bodyLabel = makeLabel(text, size: 13.5)
        hintLabel = makeLabel("Click to copy", size: 11, weight: .semibold,
                              color: Palette.inkSoft)
        super.init(frame: .zero)

        translatesAutoresizingMaskIntoConstraints = false
        toolTip = "Click to copy"
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Copy dictation")
        setAccessibilityHelp("Copies this dictation to the clipboard")
        setAccessibilityValue(text)

        bodyLabel.lineBreakMode = .byWordWrapping
        bodyLabel.maximumNumberOfLines = 3
        bodyLabel.preferredMaxLayoutWidth = 640
        bodyLabel.setContentHuggingPriority(.defaultLow, for: .horizontal)
        bodyLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        hintLabel.alignment = .right
        hintLabel.alphaValue = 0
        hintLabel.setContentHuggingPriority(.required, for: .horizontal)
        hintLabel.setContentCompressionResistancePriority(.required, for: .horizontal)

        timeLabel.translatesAutoresizingMaskIntoConstraints = false
        bodyLabel.translatesAutoresizingMaskIntoConstraints = false
        hintLabel.translatesAutoresizingMaskIntoConstraints = false
        addSubview(timeLabel)
        addSubview(bodyLabel)
        addSubview(hintLabel)
        NSLayoutConstraint.activate([
            timeLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 20),
            timeLabel.topAnchor.constraint(equalTo: topAnchor, constant: 16),
            timeLabel.widthAnchor.constraint(equalToConstant: 66),
            bodyLabel.leadingAnchor.constraint(equalTo: timeLabel.trailingAnchor, constant: 16),
            bodyLabel.topAnchor.constraint(equalTo: topAnchor, constant: 13),
            bodyLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -20),
            bodyLabel.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -13),
            hintLabel.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -18),
            hintLabel.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }
    required init?(coder: NSCoder) { fatalError() }

    deinit { copiedReset?.cancel() }

    override func layout() {
        super.layout()
        let width = max(80, bounds.width - 20 - 66 - 16 - 20)
        if bodyLabel.preferredMaxLayoutWidth != width {
            bodyLabel.preferredMaxLayoutWidth = width
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero,
            options: [.mouseEnteredAndExited, .cursorUpdate, .activeInActiveApp, .inVisibleRect],
            owner: self, userInfo: nil))
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .pointingHand)
    }

    override func cursorUpdate(with event: NSEvent) {
        NSCursor.pointingHand.set()
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        bounds.contains(point) ? self : nil
    }

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseEntered(with event: NSEvent) { hovered = true }
    override func mouseExited(with event: NSEvent) { hovered = false }

    override func mouseDown(with event: NSEvent) {
        pressed = true
    }

    override func mouseUp(with event: NSEvent) {
        defer { pressed = false }
        guard pressed else { return }
        let loc = convert(event.locationInWindow, from: nil)
        guard bounds.contains(loc) else { return }
        copyToClipboard()
    }

    override func accessibilityPerformPress() -> Bool {
        copyToClipboard()
        return true
    }

    override func draw(_ dirtyRect: NSRect) {
        if copied {
            Palette.lav.setFill()
            bounds.fill()
        } else if hovered {
            Palette.lav.withAlphaComponent(0.42).setFill()
            bounds.fill()
        }
    }

    func copyToClipboard() {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        copied = true
        copiedReset?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.copied = false
        }
        copiedReset = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.4, execute: work)
        NSAccessibility.post(element: self, notification: .announcementRequested,
                             userInfo: [.announcement: "Copied" as NSString])
    }

    private func updateChrome() {
        needsDisplay = true
        let showHint = copied || hovered
        if copied {
            hintLabel.stringValue = "Copied"
            hintLabel.textColor = Palette.green
        } else {
            hintLabel.stringValue = "Click to copy"
            hintLabel.textColor = Palette.inkSoft
        }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.14
            hintLabel.animator().alphaValue = showHint ? 1 : 0
        }
        window?.invalidateCursorRects(for: self)
    }
}

func stickerWindow(size: NSSize) -> NSWindow {
    let w = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                     styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
                     backing: .buffered, defer: false)
    w.title = "Voice"
    w.titleVisibility = .hidden
    w.titlebarAppearsTransparent = true
    w.isMovableByWindowBackground = true
    w.appearance = NSAppearance(named: .aqua)
    w.backgroundColor = Palette.bg
    w.isReleasedWhenClosed = false
    w.center()
    return w
}

// MARK: - Main window (Dictations / Snippets / Settings)

final class MainWindow: NSObject, NSWindowDelegate {
    private weak var app: AppDelegate?
    private var window: NSWindow!

    private var tabButtons: [String: NSButton] = [:]
    private var underlines: [String: NSView] = [:]
    private var gearButton: NSButton!
    private var container: NSView!
    private var views: [String: NSView] = [:]
    private var current = "dictations"

    // dictations
    private var dictStack: NSStackView!
    private var lastHistoryStamp = -1
    // snippets
    private var snipStack: NSStackView!
    private var lastSnippetStamp = -1
    private var addRow: NSView!
    private var trigField: NSTextField!
    private var bodyField: NSTextField!
    // settings
    private var statusDot: NSView!
    private var statusLabel: NSTextField!
    private var fixButton: CapsuleButton!
    private var hkPopup: NSPopUpButton!
    private var soundSwitch: NSSwitch!
    private var loginSwitch: NSSwitch!

    private var refreshTimer: Timer?

    init(app: AppDelegate) {
        self.app = app
        super.init()
        build()
    }

    var isVisible: Bool { window.isVisible }

    func show() {
        NSApp.setActivationPolicy(.regular)
        refresh()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        if refreshTimer == nil {
            let t = Timer(timeInterval: 1.0, repeats: true) { [weak self] _ in self?.refresh() }
            RunLoop.main.add(t, forMode: .common)
            refreshTimer = t
        }
    }

    func windowWillClose(_ notification: Notification) {
        refreshTimer?.invalidate(); refreshTimer = nil
        NSApp.setActivationPolicy(.accessory)
    }

    // MARK: build

    private func build() {
        window = stickerWindow(size: NSSize(width: 1000, height: 680))
        window.delegate = self
        let content = NSView()
        window.contentView = content

        // top bar
        let topbar = NSView()
        topbar.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(topbar)
        let rule = NSView()
        rule.translatesAutoresizingMaskIntoConstraints = false
        rule.wantsLayer = true
        rule.layer?.backgroundColor = Palette.ink.cgColor
        topbar.addSubview(rule)

        let brandCap = NSView()
        brandCap.translatesAutoresizingMaskIntoConstraints = false
        brandCap.wantsLayer = true
        brandCap.layer?.backgroundColor = Palette.pillBlack.cgColor
        brandCap.layer?.cornerRadius = 14
        let brandWave = MiniWave()
        brandWave.barWidth = 3; brandWave.gap = 3; brandWave.maxBar = 12; brandWave.count = 4
        brandWave.animated = false
        brandWave.translatesAutoresizingMaskIntoConstraints = false
        brandCap.addSubview(brandWave)
        NSLayoutConstraint.activate([
            brandWave.centerXAnchor.constraint(equalTo: brandCap.centerXAnchor),
            brandWave.centerYAnchor.constraint(equalTo: brandCap.centerYAnchor),
            brandCap.widthAnchor.constraint(equalToConstant: 44),
            brandCap.heightAnchor.constraint(equalToConstant: 28),
        ])
        let brandName = makeLabel("Voice", size: 20, weight: .bold, serif: true)

        let tabD = tabButton("Dictations", key: "dictations")
        let tabS = tabButton("Snippets", key: "snippets")

        gearButton = NSButton(image: NSImage(systemSymbolName: "gearshape",
                                             accessibilityDescription: "Settings")!
            .withSymbolConfiguration(.init(pointSize: 14, weight: .semibold))!,
                              target: self, action: #selector(openSettings))
        gearButton.isBordered = false
        gearButton.wantsLayer = true
        gearButton.layer?.cornerRadius = 8
        gearButton.contentTintColor = Palette.ink
        gearButton.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            gearButton.widthAnchor.constraint(equalToConstant: 32),
            gearButton.heightAnchor.constraint(equalToConstant: 32),
        ])

        let bar = hstack([brandCap, brandName, spacer(width: 14), tabD, tabS], spacing: 10)
        bar.translatesAutoresizingMaskIntoConstraints = false
        topbar.addSubview(bar)
        topbar.addSubview(gearButton)

        container = NSView()
        container.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(container)

        NSLayoutConstraint.activate([
            topbar.topAnchor.constraint(equalTo: content.topAnchor),
            topbar.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            topbar.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            topbar.heightAnchor.constraint(equalToConstant: 62),
            rule.leadingAnchor.constraint(equalTo: topbar.leadingAnchor),
            rule.trailingAnchor.constraint(equalTo: topbar.trailingAnchor),
            rule.bottomAnchor.constraint(equalTo: topbar.bottomAnchor),
            rule.heightAnchor.constraint(equalToConstant: 1.5),
            bar.leadingAnchor.constraint(equalTo: topbar.leadingAnchor, constant: 92),
            bar.centerYAnchor.constraint(equalTo: topbar.centerYAnchor),
            bar.heightAnchor.constraint(equalTo: topbar.heightAnchor),
            gearButton.trailingAnchor.constraint(equalTo: topbar.trailingAnchor, constant: -20),
            gearButton.centerYAnchor.constraint(equalTo: topbar.centerYAnchor),
            container.topAnchor.constraint(equalTo: topbar.bottomAnchor),
            container.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            container.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            container.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])

        views["dictations"] = makeScrollView { self.dictStack = $0 }
        views["snippets"] = makeScrollView { self.snipStack = $0 }
        views["settings"] = buildSettings()
        for (_, v) in views {
            container.addSubview(v)
            NSLayoutConstraint.activate([
                v.topAnchor.constraint(equalTo: container.topAnchor),
                v.leadingAnchor.constraint(equalTo: container.leadingAnchor),
                v.trailingAnchor.constraint(equalTo: container.trailingAnchor),
                v.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            ])
        }
        select("dictations")
    }

    private func spacer(width: CGFloat) -> NSView {
        let v = NSView()
        v.translatesAutoresizingMaskIntoConstraints = false
        v.widthAnchor.constraint(equalToConstant: width).isActive = true
        return v
    }

    private func tabButton(_ title: String, key: String) -> NSView {
        let b = NSButton(title: title, target: self, action: #selector(tabTapped(_:)))
        b.isBordered = false
        b.identifier = NSUserInterfaceItemIdentifier(key)
        tabButtons[key] = b
        let underline = NSView()
        underline.translatesAutoresizingMaskIntoConstraints = false
        underline.wantsLayer = true
        underline.layer?.backgroundColor = Palette.ink.cgColor
        underline.heightAnchor.constraint(equalToConstant: 3).isActive = true
        underlines[key] = underline
        let wrap = NSView()
        wrap.translatesAutoresizingMaskIntoConstraints = false
        b.translatesAutoresizingMaskIntoConstraints = false
        wrap.addSubview(b)
        wrap.addSubview(underline)
        NSLayoutConstraint.activate([
            b.topAnchor.constraint(equalTo: wrap.topAnchor),
            b.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 4),
            b.trailingAnchor.constraint(equalTo: wrap.trailingAnchor, constant: -4),
            b.centerYAnchor.constraint(equalTo: wrap.centerYAnchor),
            underline.leadingAnchor.constraint(equalTo: b.leadingAnchor),
            underline.trailingAnchor.constraint(equalTo: b.trailingAnchor),
            underline.bottomAnchor.constraint(equalTo: wrap.bottomAnchor),
            wrap.heightAnchor.constraint(equalToConstant: 60),
        ])
        return wrap
    }

    private func makeScrollView(assign: (NSStackView) -> Void) -> NSView {
        let scroll = NSScrollView()
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true

        let flip = FlippedView()
        flip.translatesAutoresizingMaskIntoConstraints = false
        let stack = vstack([], spacing: 14)
        stack.translatesAutoresizingMaskIntoConstraints = false
        flip.addSubview(stack)
        scroll.documentView = flip

        NSLayoutConstraint.activate([
            flip.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
            stack.topAnchor.constraint(equalTo: flip.topAnchor, constant: 30),
            stack.leadingAnchor.constraint(equalTo: flip.leadingAnchor, constant: 44),
            stack.trailingAnchor.constraint(equalTo: flip.trailingAnchor, constant: -44),
            stack.bottomAnchor.constraint(equalTo: flip.bottomAnchor, constant: -44),
        ])
        assign(stack)
        return scroll
    }

    // MARK: tabs

    @objc private func tabTapped(_ sender: NSButton) {
        select(sender.identifier?.rawValue ?? "dictations")
    }
    @objc private func openSettings() { select("settings") }

    private func select(_ key: String) {
        current = key
        for (k, v) in views { v.isHidden = (k != key) }
        for (k, b) in tabButtons {
            let active = k == key
            b.attributedTitle = NSAttributedString(string: b.title, attributes: [
                .font: NSFont.systemFont(ofSize: 13.5, weight: .semibold),
                .foregroundColor: active ? Palette.ink : Palette.inkSoft])
            underlines[k]?.isHidden = !active
        }
        gearButton.layer?.backgroundColor = key == "settings" ? Palette.lav.cgColor : NSColor.clear.cgColor
        refresh(force: true)
    }

    // MARK: refresh

    func refresh(force: Bool = false) {
        guard let app = app else { return }
        if current == "dictations", force || app.historyStore.stamp != lastHistoryStamp {
            lastHistoryStamp = app.historyStore.stamp
            rebuildDictations()
        }
        if current == "snippets", force || app.snippetStore.stamp != lastSnippetStamp {
            lastSnippetStamp = app.snippetStore.stamp
            rebuildSnippets()
        }
        if current == "settings" { refreshSettings() }
    }

    // MARK: dictations view

    private func rebuildDictations() {
        guard let app = app else { return }
        dictStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        dictStack.addArrangedSubview(makeLabel("Dictations", size: 32, weight: .bold, serif: true))

        let fmtWords = NumberFormatter()
        fmtWords.numberStyle = .decimal
        let words = fmtWords.string(from: NSNumber(value: app.historyStore.totalWords)) ?? "0"
        let wpm = app.historyStore.averageWPM
        let lat = app.historyStore.averageLatency

        let strip = hstack([
            statChip(words, "words spoken"),
            statChip(wpm > 0 ? "\(wpm)" : "—", "words per minute"),
            statChip(lat > 0 ? String(format: "%.1fs", lat) : "—", "average latency"),
        ], spacing: 14)
        strip.alignment = .top

        let live = NSView()
        live.translatesAutoresizingMaskIntoConstraints = false
        live.wantsLayer = true
        live.layer?.backgroundColor = Palette.pillBlack.cgColor
        live.layer?.cornerRadius = 32
        let lwave = MiniWave()
        lwave.translatesAutoresizingMaskIntoConstraints = false
        let llabel = makeLabel("Hold \(Config.hotkey.shortLabel) anywhere",
                               size: 12.5, weight: .medium,
                               color: NSColor.white.withAlphaComponent(0.75))
        let lrow = hstack([lwave, llabel], spacing: 12)
        lrow.translatesAutoresizingMaskIntoConstraints = false
        live.addSubview(lrow)
        NSLayoutConstraint.activate([
            lrow.centerXAnchor.constraint(equalTo: live.centerXAnchor),
            lrow.centerYAnchor.constraint(equalTo: live.centerYAnchor),
            live.heightAnchor.constraint(equalToConstant: 64),
            live.widthAnchor.constraint(greaterThanOrEqualToConstant: 240),
        ])
        let stripRow = hstack([strip, NSView(), live], spacing: 14)
        stripRow.distribution = .fill
        dictStack.addArrangedSubview(stripRow)
        stripRow.widthAnchor.constraint(equalTo: dictStack.widthAnchor).isActive = true
        dictStack.setCustomSpacing(24, after: stripRow)

        let groups = app.historyStore.grouped()
        if groups.isEmpty {
            let empty = StickerCard()
            let msg = makeLabel("Hold \(Config.hotkey.shortLabel) in any app and your dictations will appear here.",
                                size: 13, color: Palette.inkSoft)
            msg.translatesAutoresizingMaskIntoConstraints = false
            empty.content.addSubview(msg)
            pinToContent(msg, of: empty)
            dictStack.addArrangedSubview(empty)
            empty.widthAnchor.constraint(equalTo: dictStack.widthAnchor).isActive = true
            return
        }

        let timeFmt = DateFormatter()
        timeFmt.dateFormat = "h:mm a"
        for (title, items) in groups {
            let cap = microcaps(title)
            dictStack.addArrangedSubview(cap)
            dictStack.setCustomSpacing(8, after: cap)
            let card = StickerCard(padding: NSEdgeInsets(top: 4, left: 0, bottom: 4, right: 0))
            let rows = vstack([], spacing: 0)
            rows.translatesAutoresizingMaskIntoConstraints = false
            for (i, e) in items.enumerated() {
                if i > 0 {
                    let d = DashedLine()
                    rows.addArrangedSubview(d)
                    d.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
                }
                let row = ledgerRow(time: timeFmt.string(from: e.date), text: e.text)
                rows.addArrangedSubview(row)
                row.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
            }
            card.content.addSubview(rows)
            pinToContent(rows, of: card)
            dictStack.addArrangedSubview(card)
            card.widthAnchor.constraint(equalTo: dictStack.widthAnchor).isActive = true
            dictStack.setCustomSpacing(22, after: card)
        }
    }

    private func statChip(_ number: String, _ label: String) -> StickerCard {
        let c = StickerCard(padding: NSEdgeInsets(top: 12, left: 20, bottom: 12, right: 20))
        let col = vstack([
            makeLabel(number, size: 24, weight: .bold, serif: true),
            makeLabel(label, size: 11.5, color: Palette.inkSoft),
        ], spacing: 1)
        col.translatesAutoresizingMaskIntoConstraints = false
        c.content.addSubview(col)
        pinToContent(col, of: c)
        return c
    }

    private func ledgerRow(time: String, text: String) -> NSView {
        LedgerRow(time: time, text: text)
    }

    // MARK: snippets view

    private func rebuildSnippets() {
        guard let app = app else { return }
        snipStack.arrangedSubviews.forEach { $0.removeFromSuperview() }

        let head = hstack([
            makeLabel("Snippets", size: 32, weight: .bold, serif: true),
            NSView(),
            CapsuleButton("+ New snippet", style: .ink, target: self, action: #selector(showAddRow)),
        ], spacing: 10)
        snipStack.addArrangedSubview(head)
        head.widthAnchor.constraint(equalTo: snipStack.widthAnchor).isActive = true
        snipStack.setCustomSpacing(20, after: head)

        // promo card
        let promo = StickerCard(padding: NSEdgeInsets(top: 22, left: 26, bottom: 22, right: 26))
        promo.cardFill = Palette.lav
        let h3 = NSTextField(labelWithString: "")
        let a = NSMutableAttributedString(string: "Your voice, ", attributes: [
            .font: serifFont(ofSize: 24, weight: .semibold), .foregroundColor: Palette.ink])
        a.append(NSAttributedString(string: "abbreviated.", attributes: [
            .font: serifFont(ofSize: 24, weight: .semibold, italic: true),
            .foregroundColor: Palette.ink]))
        h3.attributedStringValue = a
        let sub = makeLabel("Save the things you re-type constantly. Say the trigger while dictating and Voice swaps in the full text before it lands.",
                            size: 13, color: Palette.inkSoft)
        sub.lineBreakMode = .byWordWrapping
        sub.maximumNumberOfLines = 2
        sub.preferredMaxLayoutWidth = 560
        let ex1 = exampleRow(say: "my email", out: "you@example.com")
        let ex2 = exampleRow(say: "standup", out: "Yesterday: … Today: … Blockers: none")
        let col = vstack([h3, sub, ex1, ex2], spacing: 10)
        col.setCustomSpacing(16, after: sub)
        col.translatesAutoresizingMaskIntoConstraints = false
        promo.content.addSubview(col)
        pinToContent(col, of: promo)
        snipStack.addArrangedSubview(promo)
        promo.widthAnchor.constraint(equalTo: snipStack.widthAnchor).isActive = true
        snipStack.setCustomSpacing(22, after: promo)

        // list card
        let card = StickerCard(padding: NSEdgeInsets(top: 4, left: 0, bottom: 4, right: 0))
        let rows = vstack([], spacing: 0)
        rows.translatesAutoresizingMaskIntoConstraints = false

        // add-row
        trigField = NSTextField(string: "")
        trigField.placeholderString = "trigger"
        trigField.font = serifFont(ofSize: 13, weight: .medium, italic: true)
        bodyField = NSTextField(string: "")
        bodyField.placeholderString = "Text to insert…"
        bodyField.font = .systemFont(ofSize: 13)
        for f in [trigField!, bodyField!] {
            f.wantsLayer = true
            f.bezelStyle = .roundedBezel
            f.translatesAutoresizingMaskIntoConstraints = false
        }
        trigField.widthAnchor.constraint(equalToConstant: 150).isActive = true
        let save = CapsuleButton("Save", style: .ink, target: self, action: #selector(saveSnippet))
        let addStack = hstack([trigField, bodyField, save], spacing: 10)
        addStack.translatesAutoresizingMaskIntoConstraints = false
        bodyField.setContentHuggingPriority(.init(1), for: .horizontal)
        let addWrap = NSView()
        addWrap.translatesAutoresizingMaskIntoConstraints = false
        addWrap.addSubview(addStack)
        NSLayoutConstraint.activate([
            addStack.topAnchor.constraint(equalTo: addWrap.topAnchor, constant: 12),
            addStack.leadingAnchor.constraint(equalTo: addWrap.leadingAnchor, constant: 20),
            addStack.trailingAnchor.constraint(equalTo: addWrap.trailingAnchor, constant: -20),
            addStack.bottomAnchor.constraint(equalTo: addWrap.bottomAnchor, constant: -12),
        ])
        addWrap.isHidden = true
        addRow = addWrap
        rows.addArrangedSubview(addWrap)
        addWrap.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true

        if app.snippetStore.snippets.isEmpty {
            let empty = makeLabel("No snippets yet — add one and say its trigger while dictating.",
                                  size: 13, color: Palette.inkSoft)
            empty.translatesAutoresizingMaskIntoConstraints = false
            let wrap = NSView()
            wrap.translatesAutoresizingMaskIntoConstraints = false
            wrap.addSubview(empty)
            NSLayoutConstraint.activate([
                empty.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 14),
                empty.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 20),
                empty.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -14),
            ])
            rows.addArrangedSubview(wrap)
            wrap.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
        }

        for (i, s) in app.snippetStore.snippets.enumerated() {
            if i > 0 || !addRow.isHidden || !app.snippetStore.snippets.isEmpty && i > 0 {
                // separators handled below
            }
            if i > 0 {
                let d = DashedLine()
                rows.addArrangedSubview(d)
                d.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
            }
            let row = snippetRow(s, index: i)
            rows.addArrangedSubview(row)
            row.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
        }

        card.content.addSubview(rows)
        pinToContent(rows, of: card)
        snipStack.addArrangedSubview(card)
        card.widthAnchor.constraint(equalTo: snipStack.widthAnchor).isActive = true
    }

    private func exampleRow(say: String, out: String) -> NSView {
        let sayChip = paddedChip("\u{201C}\(say)\u{201D}",
                                 font: serifFont(ofSize: 13, weight: .semibold, italic: true),
                                 fg: Palette.ink, bg: Palette.paper, border: true)
        let arrow = makeLabel("→", size: 13, weight: .semibold, color: Palette.inkSoft, mono: true)
        let outChip = paddedChip(out, font: .systemFont(ofSize: 12.5),
                                 fg: .white, bg: Palette.pillBlack, border: false)
        return hstack([sayChip, arrow, outChip], spacing: 10)
    }

    private func paddedChip(_ text: String, font: NSFont, fg: NSColor, bg: NSColor,
                            border: Bool) -> NSView {
        let v = NSView()
        v.translatesAutoresizingMaskIntoConstraints = false
        v.wantsLayer = true
        v.layer?.backgroundColor = bg.cgColor
        v.layer?.cornerRadius = 14
        if border { v.layer?.borderWidth = 1.5; v.layer?.borderColor = Palette.ink.cgColor }
        let l = NSTextField(labelWithString: text)
        l.font = font
        l.textColor = fg
        l.lineBreakMode = .byTruncatingTail
        l.translatesAutoresizingMaskIntoConstraints = false
        v.addSubview(l)
        NSLayoutConstraint.activate([
            l.topAnchor.constraint(equalTo: v.topAnchor, constant: 6),
            l.leadingAnchor.constraint(equalTo: v.leadingAnchor, constant: 14),
            l.trailingAnchor.constraint(equalTo: v.trailingAnchor, constant: -14),
            l.bottomAnchor.constraint(equalTo: v.bottomAnchor, constant: -6),
            l.widthAnchor.constraint(lessThanOrEqualToConstant: 420),
        ])
        return v
    }

    private func snippetRow(_ s: Snippet, index: Int) -> NSView {
        let row = HoverRow()
        row.translatesAutoresizingMaskIntoConstraints = false
        let chip = paddedChip("\u{201C}\(s.trigger)\u{201D}",
                              font: serifFont(ofSize: 13, weight: .semibold, italic: true),
                              fg: Palette.ink, bg: Palette.lav, border: true)
        let arrow = makeLabel("→", size: 13, weight: .semibold, color: Palette.faint, mono: true)
        let body = makeLabel(s.text, size: 13, color: Palette.inkSoft)
        body.lineBreakMode = .byTruncatingTail
        body.setContentHuggingPriority(.defaultLow, for: .horizontal)
        body.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let del = NSButton(title: "", target: self, action: #selector(deleteSnippet(_:)))
        del.isBordered = false
        del.attributedTitle = NSAttributedString(string: "✕", attributes: [
            .font: NSFont.systemFont(ofSize: 13, weight: .semibold),
            .foregroundColor: Palette.faint])
        del.tag = index
        del.isHidden = true
        let stack = hstack([chip, arrow, body], spacing: 12)
        stack.translatesAutoresizingMaskIntoConstraints = false
        del.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(stack); row.addSubview(del)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: row.topAnchor, constant: 11),
            stack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: row.trailingAnchor, constant: -54),
            stack.bottomAnchor.constraint(equalTo: row.bottomAnchor, constant: -11),
            del.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -18),
            del.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ])
        row.onHover = { del.isHidden = !$0 }
        return row
    }

    @objc private func showAddRow() {
        addRow.isHidden = false
        window.makeFirstResponder(trigField)
    }

    @objc private func saveSnippet() {
        app?.snippetStore.add(trigger: trigField.stringValue, text: bodyField.stringValue)
        refresh(force: true)
    }

    @objc private func deleteSnippet(_ sender: NSButton) {
        app?.snippetStore.remove(at: sender.tag)
        refresh(force: true)
    }

    // MARK: settings view

    private func buildSettings() -> NSView {
        let root = NSView()
        root.translatesAutoresizingMaskIntoConstraints = false

        let title = makeLabel("Settings", size: 32, weight: .bold, serif: true)

        statusDot = NSView()
        statusDot.translatesAutoresizingMaskIntoConstraints = false
        statusDot.wantsLayer = true
        statusDot.layer?.cornerRadius = 4.5
        NSLayoutConstraint.activate([
            statusDot.widthAnchor.constraint(equalToConstant: 9),
            statusDot.heightAnchor.constraint(equalToConstant: 9),
        ])
        statusLabel = makeLabel("", size: 13, weight: .medium)
        statusLabel.lineBreakMode = .byWordWrapping
        statusLabel.maximumNumberOfLines = 2
        statusLabel.preferredMaxLayoutWidth = 380
        fixButton = CapsuleButton("Open Settings…", style: .lav,
                                  target: self, action: #selector(openAX))
        fixButton.isHidden = true
        let statusRow = hstack([statusDot, statusLabel, fixButton], spacing: 10)

        hkPopup = NSPopUpButton(frame: .zero, pullsDown: false)
        for hk in Hotkey.allCases { hkPopup.addItem(withTitle: "Hold \(hk.label)") }
        hkPopup.target = self
        hkPopup.action = #selector(hkChanged)
        soundSwitch = NSSwitch()
        soundSwitch.target = self
        soundSwitch.action = #selector(soundChanged)
        loginSwitch = NSSwitch()
        loginSwitch.target = self
        loginSwitch.action = #selector(loginChanged)
        let micBtn = CapsuleButton("Run 3-second test", style: .lav,
                                   target: self, action: #selector(micTest))

        let card = StickerCard(padding: NSEdgeInsets(top: 6, left: 22, bottom: 6, right: 22))
        let rows = vstack([
            settingRow("Talk key", control: hkPopup),
            DashedLine(),
            settingRow("Sound effects", control: soundSwitch),
            DashedLine(),
            settingRow("Start at login", control: loginSwitch),
            DashedLine(),
            settingRow("Microphone check", control: micBtn),
        ], spacing: 0)
        rows.translatesAutoresizingMaskIntoConstraints = false
        for v in rows.arrangedSubviews {
            v.widthAnchor.constraint(equalTo: rows.widthAnchor).isActive = true
        }
        card.content.addSubview(rows)
        pinToContent(rows, of: card)

        let foot = makeLabel("Everything runs on this Mac — audio, transcription, history. Nothing leaves your computer.",
                             size: 12, color: Palette.faint)
        foot.lineBreakMode = .byWordWrapping
        foot.maximumNumberOfLines = 2
        foot.preferredMaxLayoutWidth = 520

        let col = vstack([title, statusRow, card, foot], spacing: 18)
        col.setCustomSpacing(14, after: title)
        col.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(col)
        NSLayoutConstraint.activate([
            col.topAnchor.constraint(equalTo: root.topAnchor, constant: 30),
            col.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 44),
            col.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -44),
            card.widthAnchor.constraint(equalToConstant: 560),
        ])
        return root
    }

    private func settingRow(_ label: String, control: NSView) -> NSView {
        let l = makeLabel(label, size: 13.5)
        control.translatesAutoresizingMaskIntoConstraints = false
        let row = NSView()
        row.translatesAutoresizingMaskIntoConstraints = false
        l.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(l); row.addSubview(control)
        NSLayoutConstraint.activate([
            l.leadingAnchor.constraint(equalTo: row.leadingAnchor),
            l.centerYAnchor.constraint(equalTo: row.centerYAnchor),
            control.trailingAnchor.constraint(equalTo: row.trailingAnchor),
            control.centerYAnchor.constraint(equalTo: row.centerYAnchor),
            row.heightAnchor.constraint(equalToConstant: 52),
        ])
        return row
    }

    private func refreshSettings() {
        guard let app = app else { return }
        let s = app.statusInfo()
        statusDot.layer?.backgroundColor = s.color.cgColor
        statusLabel.stringValue = s.text
        fixButton.isHidden = !s.needsAccessibility
        hkPopup.selectItem(at: Hotkey.allCases.firstIndex(of: Config.hotkey) ?? 0)
        soundSwitch.state = Config.soundsEnabled ? .on : .off
        loginSwitch.state = SMAppService.mainApp.status == .enabled ? .on : .off
    }

    @objc private func openAX() {
        NSWorkspace.shared.open(URL(string:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
    }
    @objc private func hkChanged() {
        Config.hotkey = Hotkey.allCases[hkPopup.indexOfSelectedItem]
        app?.refreshUI()
    }
    @objc private func soundChanged() {
        UserDefaults.standard.set(soundSwitch.state == .on, forKey: "sounds")
    }
    @objc private func loginChanged() {
        if loginSwitch.state == .on { try? SMAppService.mainApp.register() }
        else { try? SMAppService.mainApp.unregister() }
    }
    @objc private func micTest() { app?.previewMic() }

    private func pinToContent(_ v: NSView, of card: StickerCard) {
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: card.content.topAnchor),
            v.leadingAnchor.constraint(equalTo: card.content.leadingAnchor),
            v.trailingAnchor.constraint(equalTo: card.content.trailingAnchor),
            v.bottomAnchor.constraint(equalTo: card.content.bottomAnchor),
        ])
    }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

// MARK: - Onboarding window

final class OnboardingWindow: NSObject, NSWindowDelegate {
    private weak var app: AppDelegate?
    private var window: NSWindow!
    private var container: NSView!
    private var stepViews: [NSView] = []
    private var stepNo: NSTextField!
    private var progress: NSView!
    private var progressWidth: NSLayoutConstraint!
    private var step = 1

    private var micButton: CapsuleButton!
    private var axButton: CapsuleButton!
    private var permContinue: CapsuleButton!
    private var tryContinue: CapsuleButton!
    private var tryText: NSTextView!
    private var hkTiles: [NSButton] = []
    private var backButton: CapsuleButton!
    private var pollTimer: Timer?

    init(app: AppDelegate) {
        self.app = app
        super.init()
        build()
    }

    var isVisible: Bool { window.isVisible }

    func show() {
        NSApp.setActivationPolicy(.regular)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        goTo(1)
        if pollTimer == nil {
            let t = Timer(timeInterval: 0.8, repeats: true) { [weak self] _ in self?.poll() }
            RunLoop.main.add(t, forMode: .common)
            pollTimer = t
        }
    }

    func close() {
        pollTimer?.invalidate(); pollTimer = nil
        window.close()
    }

    func windowWillClose(_ notification: Notification) {
        pollTimer?.invalidate(); pollTimer = nil
        UserDefaults.standard.set(true, forKey: "onboarded")
        if app?.mainWindowVisible != true { NSApp.setActivationPolicy(.accessory) }
    }

    func dictationLanded() {
        guard step == 4 else { return }
        tryContinue.actionable = true
    }

    // MARK: build

    private func build() {
        window = stickerWindow(size: NSSize(width: 760, height: 610))
        window.delegate = self
        let content = NSView()
        window.contentView = content

        stepNo = makeLabel("01 / 05", size: 12, weight: .semibold, color: Palette.faint, mono: true)
        stepNo.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stepNo)

        container = NSView()
        container.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(container)

        progress = NSView()
        progress.translatesAutoresizingMaskIntoConstraints = false
        progress.wantsLayer = true
        progress.layer?.backgroundColor = Palette.ink.cgColor
        content.addSubview(progress)
        progressWidth = progress.widthAnchor.constraint(equalTo: content.widthAnchor, multiplier: 0.2)

        backButton = CapsuleButton("Back", style: .ghost, target: self, action: #selector(goBack))
        backButton.isHidden = true
        content.addSubview(backButton)
        NSLayoutConstraint.activate([
            backButton.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 16),
            backButton.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -14),
        ])

        NSLayoutConstraint.activate([
            stepNo.topAnchor.constraint(equalTo: content.topAnchor, constant: 22),
            stepNo.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
            container.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            container.centerYAnchor.constraint(equalTo: content.centerYAnchor),
            container.widthAnchor.constraint(lessThanOrEqualToConstant: 540),
            progress.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            progress.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            progress.heightAnchor.constraint(equalToConstant: 4),
            progressWidth,
        ])

        stepViews = [buildStep1(), buildStep2(), buildStep3(), buildStep4(), buildStep5()]
        for v in stepViews {
            container.addSubview(v)
            NSLayoutConstraint.activate([
                v.topAnchor.constraint(equalTo: container.topAnchor),
                v.bottomAnchor.constraint(equalTo: container.bottomAnchor),
                v.centerXAnchor.constraint(equalTo: container.centerXAnchor),
                v.widthAnchor.constraint(lessThanOrEqualTo: container.widthAnchor),
            ])
        }
    }

    private func goTo(_ n: Int) {
        step = n
        for (i, v) in stepViews.enumerated() { v.isHidden = (i + 1 != n) }
        stepNo.stringValue = "0\(n) / 05"
        backButton.isHidden = (n == 1)
        progressWidth.isActive = false
        progressWidth = progress.widthAnchor.constraint(
            equalTo: window.contentView!.widthAnchor, multiplier: CGFloat(n) * 0.2)
        progressWidth.isActive = true
        if n == 4 { window.makeFirstResponder(tryText) }
    }

    @objc private func goBack() {
        if step > 1 { goTo(step - 1) }
    }

    private func centered(_ views: [NSView], spacing: CGFloat = 14) -> NSStackView {
        let s = NSStackView(views: views)
        s.orientation = .vertical
        s.alignment = .centerX
        s.spacing = spacing
        s.translatesAutoresizingMaskIntoConstraints = false
        return s
    }

    private func heroTitle(_ regular: String, _ italic: String, size: CGFloat = 40) -> NSTextField {
        let l = NSTextField(labelWithString: "")
        let a = NSMutableAttributedString(string: regular, attributes: [
            .font: serifFont(ofSize: size, weight: .bold), .foregroundColor: Palette.ink])
        a.append(NSAttributedString(string: italic, attributes: [
            .font: serifFont(ofSize: size, weight: .bold, italic: true),
            .foregroundColor: Palette.ink]))
        l.attributedStringValue = a
        return l
    }

    private func subText(_ text: String) -> NSTextField {
        let l = makeLabel(text, size: 14.5, color: Palette.inkSoft)
        l.lineBreakMode = .byWordWrapping
        l.maximumNumberOfLines = 4
        l.alignment = .center
        l.preferredMaxLayoutWidth = 460
        return l
    }

    private func blackCapsule(_ inner: NSView, w: CGFloat, h: CGFloat) -> StickerCard {
        let c = StickerCard(padding: NSEdgeInsets(top: 0, left: 0, bottom: 0, right: 0))
        c.cardFill = Palette.pillBlack
        c.cornerRadius = h / 2
        inner.translatesAutoresizingMaskIntoConstraints = false
        c.content.addSubview(inner)
        NSLayoutConstraint.activate([
            inner.centerXAnchor.constraint(equalTo: c.content.centerXAnchor),
            inner.centerYAnchor.constraint(equalTo: c.content.centerYAnchor),
            c.widthAnchor.constraint(equalToConstant: w),
            c.heightAnchor.constraint(equalToConstant: h),
        ])
        return c
    }

    private func buildStep1() -> NSView {
        let wave = MiniWave()
        wave.barWidth = 5; wave.gap = 5; wave.maxBar = 28
        let cap = blackCapsule(wave, w: 190, h: 84)
        let title = heroTitle("Say it. ", "It's typed.")
        let sub = subText("Voice turns speech into text in any app on your Mac. It runs entirely on this computer — no accounts, no subscriptions, and nothing you say ever leaves your machine.")
        let cta = CapsuleButton("Set up Voice", style: .ink, target: self, action: #selector(toStep2))
        let v = centered([cap, title, sub, cta], spacing: 16)
        v.setCustomSpacing(30, after: cap)
        v.setCustomSpacing(28, after: sub)
        return v
    }

    private func permCard(symbol: String, title: String, sub: String,
                          button: CapsuleButton) -> StickerCard {
        let card = StickerCard(padding: NSEdgeInsets(top: 14, left: 16, bottom: 14, right: 16))
        let glyphBox = NSView()
        glyphBox.translatesAutoresizingMaskIntoConstraints = false
        glyphBox.wantsLayer = true
        glyphBox.layer?.backgroundColor = Palette.lav.cgColor
        glyphBox.layer?.cornerRadius = 12
        glyphBox.layer?.borderWidth = 1.5
        glyphBox.layer?.borderColor = Palette.ink.cgColor
        let img = NSImageView(image: NSImage(systemSymbolName: symbol,
                                             accessibilityDescription: title)!
            .withSymbolConfiguration(.init(pointSize: 17, weight: .semibold))!)
        img.contentTintColor = Palette.ink
        img.translatesAutoresizingMaskIntoConstraints = false
        glyphBox.addSubview(img)
        NSLayoutConstraint.activate([
            glyphBox.widthAnchor.constraint(equalToConstant: 42),
            glyphBox.heightAnchor.constraint(equalToConstant: 42),
            img.centerXAnchor.constraint(equalTo: glyphBox.centerXAnchor),
            img.centerYAnchor.constraint(equalTo: glyphBox.centerYAnchor),
        ])
        let t = makeLabel(title, size: 14, weight: .semibold)
        let s = makeLabel(sub, size: 12.5, color: Palette.inkSoft)
        s.lineBreakMode = .byWordWrapping
        s.maximumNumberOfLines = 2
        s.preferredMaxLayoutWidth = 250
        let texts = vstack([t, s], spacing: 2)
        let row = hstack([glyphBox, texts, NSView(), button], spacing: 14)
        row.translatesAutoresizingMaskIntoConstraints = false
        card.content.addSubview(row)
        NSLayoutConstraint.activate([
            row.topAnchor.constraint(equalTo: card.content.topAnchor),
            row.leadingAnchor.constraint(equalTo: card.content.leadingAnchor),
            row.trailingAnchor.constraint(equalTo: card.content.trailingAnchor),
            row.bottomAnchor.constraint(equalTo: card.content.bottomAnchor),
            card.widthAnchor.constraint(equalToConstant: 500),
        ])
        return card
    }

    private func buildStep2() -> NSView {
        let title = heroTitle("Two permissions, ", "once", size: 32)
        let sub = subText("Voice only listens while you hold the hotkey — never in the background.")
        micButton = CapsuleButton("Allow", style: .lav, target: self, action: #selector(askMic))
        axButton = CapsuleButton("Open Settings", style: .lav, target: self, action: #selector(askAX))
        let mic = permCard(symbol: "mic", title: "Microphone",
                           sub: "To hear you while the hotkey is held.", button: micButton)
        let ax = permCard(symbol: "keyboard", title: "Accessibility",
                          sub: "To catch the hotkey anywhere and type at your cursor.", button: axButton)
        permContinue = CapsuleButton("Continue", style: .ink, target: self, action: #selector(toStep3))
        permContinue.actionable = false
        let skip = CapsuleButton("Skip for now", style: .ghost, target: self, action: #selector(toStep3))
        let v = centered([title, sub, mic, ax, permContinue, skip], spacing: 14)
        v.setCustomSpacing(22, after: sub)
        v.setCustomSpacing(22, after: ax)
        v.setCustomSpacing(2, after: permContinue)
        return v
    }

    private func buildStep3() -> NSView {
        let title = heroTitle("Pick your ", "talk key", size: 32)
        let sub = subText("Hold it down to speak. Let go and your words are typed. Tap Esc to cancel.")
        let row = hstack([], spacing: 12)
        for (i, hk) in Hotkey.allCases.enumerated() {
            let b = NSButton(title: "", target: self, action: #selector(pickHK(_:)))
            b.isBordered = false
            b.wantsLayer = true
            b.tag = i
            b.translatesAutoresizingMaskIntoConstraints = false
            b.widthAnchor.constraint(equalToConstant: 132).isActive = true
            b.heightAnchor.constraint(equalToConstant: 62).isActive = true
            b.layer?.cornerRadius = 14
            styleTile(b, selected: hk == Config.hotkey)
            let sel = hk == Config.hotkey
            b.attributedTitle = tileTitle(hk.shortLabel,
                                          sub: i == 0 ? "recommended" : "alternative",
                                          selected: sel)
            hkTiles.append(b)
            row.addArrangedSubview(b)
        }
        let cta = CapsuleButton("Continue", style: .ink, target: self, action: #selector(toStep4))
        let v = centered([title, sub, row, cta], spacing: 16)
        v.setCustomSpacing(26, after: sub)
        v.setCustomSpacing(28, after: row)
        return v
    }

    private func styleTile(_ b: NSButton, selected: Bool) {
        b.layer?.backgroundColor = (selected ? Palette.lav : Palette.paper).cgColor
        b.layer?.borderWidth = 1.5
        b.layer?.borderColor = (selected ? Palette.ink : Palette.dash).cgColor
    }

    private func tileTitle(_ main: String, sub: String, selected: Bool) -> NSAttributedString {
        let p = NSMutableParagraphStyle()
        p.alignment = .center
        let a = NSMutableAttributedString(string: main + "\n", attributes: [
            .font: NSFont.systemFont(ofSize: 14, weight: .semibold),
            .foregroundColor: Palette.ink, .paragraphStyle: p])
        a.append(NSAttributedString(string: sub, attributes: [
            .font: NSFont.systemFont(ofSize: 10.5, weight: .medium),
            .foregroundColor: Palette.inkSoft, .paragraphStyle: p]))
        return a
    }

    private func buildStep4() -> NSView {
        let title = heroTitle("Take it for ", "a spin", size: 32)
        let sub = subText("Click into the box, hold your talk key, say anything, then release.")
        let box = StickerCard(padding: NSEdgeInsets(top: 12, left: 14, bottom: 12, right: 14))
        tryText = NSTextView()
        tryText.font = .systemFont(ofSize: 15)
        tryText.textColor = Palette.ink
        tryText.backgroundColor = .clear
        tryText.isRichText = false
        tryText.insertionPointColor = Palette.ink
        tryText.translatesAutoresizingMaskIntoConstraints = false
        box.content.addSubview(tryText)
        NSLayoutConstraint.activate([
            tryText.topAnchor.constraint(equalTo: box.content.topAnchor),
            tryText.leadingAnchor.constraint(equalTo: box.content.leadingAnchor),
            tryText.trailingAnchor.constraint(equalTo: box.content.trailingAnchor),
            tryText.bottomAnchor.constraint(equalTo: box.content.bottomAnchor),
            box.widthAnchor.constraint(equalToConstant: 480),
            box.heightAnchor.constraint(equalToConstant: 110),
        ])
        tryContinue = CapsuleButton("Continue", style: .ink, target: self, action: #selector(toStep5))
        tryContinue.actionable = false
        let skip = CapsuleButton("Skip for now", style: .ghost, target: self, action: #selector(toStep5))
        let v = centered([title, sub, box, tryContinue, skip], spacing: 16)
        v.setCustomSpacing(24, after: sub)
        v.setCustomSpacing(26, after: box)
        v.setCustomSpacing(2, after: tryContinue)
        return v
    }

    private func buildStep5() -> NSView {
        let done = makeLabel("done", size: 26, weight: .semibold, color: .white, serif: true)
        if let f = done.font,
           let it = NSFont(descriptor: f.fontDescriptor.withSymbolicTraits(.italic), size: 26) {
            done.font = it
        }
        let cap = blackCapsule(done, w: 150, h: 72)
        let title = heroTitle("That's the ", "whole app.", size: 34)
        let sub = subText("Voice waits in your menu bar. Hold \(Config.hotkey.shortLabel) in any app to dictate, and come back here for your dictation history and snippets.")
        let cta = CapsuleButton("Open Voice", style: .ink, target: self, action: #selector(finish))
        let v = centered([cap, title, sub, cta], spacing: 16)
        v.setCustomSpacing(28, after: cap)
        v.setCustomSpacing(26, after: sub)
        return v
    }

    // MARK: actions & polling

    @objc private func toStep2() { goTo(2) }
    @objc private func toStep3() { goTo(3) }
    @objc private func toStep4() { goTo(4) }
    @objc private func toStep5() { goTo(5) }

    @objc private func askMic() {
        AVCaptureDevice.requestAccess(for: .audio) { _ in }
    }

    @objc private func askAX() {
        app?.requestAccessibility()
        NSWorkspace.shared.open(URL(string:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
    }

    @objc private func pickHK(_ sender: NSButton) {
        Config.hotkey = Hotkey.allCases[sender.tag]
        for (i, b) in hkTiles.enumerated() {
            let sel = i == sender.tag
            styleTile(b, selected: sel)
            b.attributedTitle = tileTitle(Hotkey.allCases[i].shortLabel,
                                          sub: i == 0 ? "recommended" : "alternative",
                                          selected: sel)
        }
        app?.refreshUI()
    }

    @objc private func finish() {
        UserDefaults.standard.set(true, forKey: "onboarded")
        close()
        app?.showMainWindow()
    }

    private func poll() {
        guard step == 2 else { return }
        if AVCaptureDevice.authorizationStatus(for: .audio) == .authorized {
            micButton.apply(.granted, title: "Granted")
            micButton.actionable = false
        }
        if app?.hotkeysRunning == true {
            axButton.apply(.granted, title: "Granted")
            axButton.actionable = false
        }
        let ok = AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
              && app?.hotkeysRunning == true
        if permContinue.actionable != ok { permContinue.actionable = ok }
    }
}
