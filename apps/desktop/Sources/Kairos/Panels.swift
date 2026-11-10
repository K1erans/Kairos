import AppKit
import Combine
import KairosCore
import SwiftUI
import UniformTypeIdentifiers

// MARK: - Quick add

@MainActor
final class QuickAddPanelController {
    private let panel: FloatingPanel
    private let model: AppModel
    private var resignObserver: NSObjectProtocol?
    /// A click on another window can leave the box key, and one on the bare desktop always
    /// does, so any mouse-down outside it closes it too. Mouse monitors need no permission.
    private let clickAway: ClickAway<Any>

    init(model: AppModel) {
        self.model = model
        let panel = FloatingPanel(contentRect: NSRect(x: 0, y: 0, width: 620, height: 96),
                                  styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        self.panel = panel
        panel.allowsKey = true
        panel.level = .modalPanel
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        clickAway = ClickAway(install: { [weak panel] report in
            let downs: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
            var monitors: [Any] = []
            // Clicks in other apps and on the desktop.
            if let m = NSEvent.addGlobalMonitorForEvents(matching: downs, handler: { _ in
                MainActor.assumeIsolated { report(false) }
            }) { monitors.append(m) }
            // Clicks in Kairos's own windows, such as Kairos Bar; they still go through.
            if let m = NSEvent.addLocalMonitorForEvents(matching: downs, handler: { event in
                MainActor.assumeIsolated { report(event.window === panel) }
                return event
            }) { monitors.append(m) }
            return monitors
        }, remove: NSEvent.removeMonitor)
        resignObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: panel, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.close() }
        }
    }

    /// The hotkey opens the box, and closes it when it is already open.
    func toggle() {
        if panel.isVisible { close() } else { show() }
    }

    func show() {
        // A fresh view each time so the field starts empty and focused.
        panel.contentView = NSHostingView(rootView: QuickAddView(close: { [weak self] in self?.close() })
            .environmentObject(model))
        if let screen = NSScreen.main {
            let f = screen.visibleFrame
            panel.setFrameOrigin(NSPoint(x: f.midX - 310, y: f.minY + f.height * 0.66))
        }
        panel.makeKeyAndOrderFront(nil)
        clickAway.start { [weak self] in self?.panel.orderOut(nil) }
    }

    private func close() {
        clickAway.stop()
        panel.orderOut(nil)
    }
}

struct QuickAddView: View {
    @EnvironmentObject var model: AppModel
    let close: () -> Void
    @State var text = ""
    @SwiftUI.FocusState private var focused: Bool

    private var parsed: ParsedTask { QuickAdd.parse(text, groups: model.data.groups.map(\.name)) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField("New task — try: Draft blog post tomorrow 3pm @Work ~45m !high", text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: 20))
                .focused($focused)
                .onSubmit {
                    if model.addTask(from: text) != nil { close() }
                }
                .onExitCommand(perform: close)
            HStack(spacing: 6) {
                if let due = parsed.due { tag("calendar", due.formatted(date: .abbreviated, time: .shortened)) }
                if let g = parsed.groupName { tag("square.grid.2x2", g) }
                if let e = parsed.estimateMin { tag("clock", "\(e) min") }
                if parsed.priority > 0 { tag("flag.fill", ["", "Low", "Medium", "High"][parsed.priority]) }
                Spacer()
                Text("Return to add · Esc to close").font(.caption).foregroundStyle(.secondary)
            }
            .frame(height: 20)
        }
        .padding(16)
        .frame(width: 620)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .onAppear { focused = true }
    }

    private func tag(_ icon: String, _ text: String) -> some View {
        Label(text, systemImage: icon)
            .font(.caption)
            .padding(.horizontal, 8).padding(.vertical, 3)
            .background(.quaternary, in: Capsule())
    }
}

// MARK: - Block and break overlay

@MainActor
final class OverlayController {
    private let panel: FloatingPanel
    private var cancellable: AnyCancellable?

    init(model: AppModel) {
        panel = FloatingPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.allowsKey = true
        panel.level = .screenSaver
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: OverlayView().environmentObject(model))
        cancellable = model.$notice.removeDuplicates { $0?.id == $1?.id }.receive(on: DispatchQueue.main).sink { [weak self] notice in
            guard let self else { return }
            guard let notice else { self.panel.orderOut(nil); return }
            if let screen = Self.screen(for: notice.screen) { self.panel.setFrame(screen.frame, display: true) }
            if notice.kind == .rest {
                // A break arrives while the owner may be typing: show it without taking the keyboard.
                self.panel.orderFrontRegardless()
            } else {
                // The blocked app was just hidden; the reason field needs the keyboard.
                self.panel.makeKeyAndOrderFront(nil)
            }
        }
    }
}

extension OverlayController {
    static func screen(for display: CGDirectDisplayID?) -> NSScreen? {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        return NSScreen.screens.first { ($0.deviceDescription[key] as? NSNumber)?.uint32Value == display } ?? NSScreen.main
    }
}

struct OverlayView: View {
    @EnvironmentObject var model: AppModel
    @State private var reason: String
    @State private var error: String?

    init(reason: String = "") { _reason = State(initialValue: reason) }

    /// Once a reason is typed, Return means "Allow"; before that it means "Back to work".
    private var allowIsDefault: Bool {
        model.noticeCanOverride && !reason.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private func allow() {
        guard allowIsDefault, model.notice != nil else { return }
        error = model.overrideNotice(reason: reason)
        if error == nil { reason = "" }
    }

    var body: some View {
        ZStack {
            Color.black.opacity(0.55).ignoresSafeArea()
            if let notice = model.notice {
                VStack(spacing: 14) {
                    Image(systemName: notice.kind == .rest ? "cup.and.saucer.fill" : "hand.raised.fill")
                        .font(.system(size: 34))
                        .foregroundStyle(Color(hex: model.activeGroup?.colorHex ?? "#4F7CFF"))
                    Text(notice.title).font(.title2.weight(.semibold))
                    Text(notice.detail).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    if model.tempo.phase != .idle {
                        Text(clock(model.remaining)).font(.system(size: 30, weight: .medium).monospacedDigit())
                    }
                    if notice.kind == .rest {
                        HStack {
                            Button("Hide") { model.notice = nil }
                            Button("Skip break") { model.dismissNotice() }
                        }
                    } else {
                        Button(model.currentTask.map { "Back to \u{201C}\($0.title)\u{201D}" } ?? "Back to work") { model.notice = nil }
                            .keyboardShortcut(allowIsDefault ? nil : .defaultAction)
                            .controlSize(.large)
                        if model.noticeCanOverride {
                            Divider().frame(width: 260)
                            TextField("Why do you need it?", text: $reason).textFieldStyle(.roundedBorder).frame(width: 260)
                                .onSubmit(allow)
                            Button("Allow \(model.data.settings.overrideMinutes) min (\(model.overridesLeft) left)", action: allow)
                                .keyboardShortcut(allowIsDefault ? .defaultAction : nil)
                                .disabled(!allowIsDefault)
                        } else if model.mode == .strict {
                            Text("Strict mode: no overrides until this session ends.").font(.caption).foregroundStyle(.secondary)
                        }
                        if let error { Text(error).font(.caption).foregroundStyle(.red) }
                    }
                }
                .padding(28)
                .frame(width: 380)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            }
        }
    }
}
