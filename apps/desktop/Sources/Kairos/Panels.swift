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

    init(model: AppModel) {
        self.model = model
        panel = FloatingPanel(contentRect: NSRect(x: 0, y: 0, width: 620, height: 96),
                              styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.allowsKey = true
        panel.level = .modalPanel
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        // Clicking anywhere else closes it.
        resignObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: panel, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.panel.orderOut(nil) }
        }
    }

    private var resignObserver: NSObjectProtocol?

    /// The hotkey opens the box, and closes it when it is already open.
    func toggle() {
        if panel.isVisible { panel.orderOut(nil) } else { show() }
    }

    func show() {
        // A fresh view each time so the field starts empty and focused.
        panel.contentView = NSHostingView(rootView: QuickAddView(close: { [weak self] in self?.panel.orderOut(nil) })
            .environmentObject(model))
        if let screen = NSScreen.main {
            let f = screen.visibleFrame
            panel.setFrameOrigin(NSPoint(x: f.midX - 310, y: f.minY + f.height * 0.66))
        }
        panel.makeKeyAndOrderFront(nil)
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
            self.panel.makeKeyAndOrderFront(nil)
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
    @State private var reason = ""
    @State private var error: String?

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
                            .keyboardShortcut(.defaultAction)
                            .controlSize(.large)
                        if model.noticeCanOverride {
                            Divider().frame(width: 260)
                            TextField("Why do you need it?", text: $reason).textFieldStyle(.roundedBorder).frame(width: 260)
                            Button("Allow \(model.data.settings.overrideMinutes) min (\(model.overridesLeft) left)") {
                                error = model.overrideNotice(reason: reason)
                                if error == nil { reason = "" }
                            }
                            .disabled(reason.trimmingCharacters(in: .whitespaces).isEmpty)
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
