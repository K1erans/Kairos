import AppKit
import Carbon.HIToolbox
import Combine
import KairosCore
import SwiftUI

/// `Kairos --snapshot <dir>` renders every view to PNG files instead of launching the app.
@main
enum Entry {
    @MainActor static func main() {
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "--snapshot") {
            Snapshot.run(into: URL(fileURLWithPath: i + 1 < args.count ? args[i + 1] : "snapshots"))
        } else if handOffToRunningCopy() {
            exit(0)
        } else {
            KairosApp.main()
        }
    }

    /// One Kairos per profile: a live bridge socket means another copy owns the data file and
    /// the Chrome link, so ask it to show its window instead of starting a second one.
    private static func handOffToRunningCopy() -> Bool {
        let client = BridgeClient()
        guard client.connect() else { return false }
        _ = client.sendLine(Data(#"{"type":"app.show"}"#.utf8))
        client.close()
        return true
    }
}

struct KairosApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var model = AppModel.shared

    var body: some Scene {
        Window("Kairos", id: "main") {
            MainWindow()
                .environmentObject(model)
                .frame(minWidth: 860, minHeight: 560)
        }
        .defaultSize(width: 1000, height: 660)

        MenuBarExtra {
            MenuBarMenu().environmentObject(model)
        } label: {
            MenuBarLabel().environmentObject(model)
        }
    }
}

extension AppModel {
    static let shared = AppModel()
}

/// Always on screen, so it is where the app learns how to open the main window from AppKit.
struct MenuBarLabel: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(model.menuTitle)
            .onAppear { AppDelegate.shared?.openMainWindow = { openWindow(id: "main") } }
    }
}

struct MenuBarMenu: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        if let task = model.currentTask {
            Text(task.title)
            Button("Mark done") { model.setDone(task, true) }
        } else {
            Text("No current task")
        }
        Divider()
        if model.tempo.phase == .idle {
            Button("Start focus") { model.startFocus() }.keyboardShortcut(.space, modifiers: [.option, .shift])
        } else {
            Text("\(model.tempo.phase.label) · \(clock(model.remaining))")
            Button(model.tempo.isPaused ? "Resume" : "Pause") { model.toggleTimer() }.keyboardShortcut(.space, modifiers: [.option, .shift])
            Button("Skip") { model.skipPhase() }.disabled(model.strictLocked)
            Button("+5 min") { model.addFiveMinutes() }
            Button("Stop") { model.stopTimer() }.disabled(model.strictLocked)
        }
        if let read = model.readStatus {
            Divider()
            Button(read.state == "playing" ? "Pause reading" : "Resume reading") { model.sendReadCommand("toggle") }
        }
        Divider()
        Picker("Group", selection: Binding(get: { model.data.focus.activeGroupId ?? UUID() }, set: { model.switchGroup($0) })) {
            ForEach(model.data.groups) { Text($0.name).tag($0.id) }
        }
        Button("New Group…") { DockActions.newGroup(model) }
        Picker("Mode", selection: Binding(get: { model.mode }, set: { model.setMode($0) })) {
            ForEach(Mode.allCases, id: \.self) { Text($0.label).tag($0) }
        }
        .disabled(model.tempo.phase != .idle)
        Divider()
        Button("Quick add…") { AppDelegate.shared?.showQuickAdd() }.keyboardShortcut(.space, modifiers: .option)
        Button("Open Kairos") {
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }
        Toggle("Show Kairos Bar", isOn: $model.data.settings.dockVisible)
        Divider()
        Button("Quit Kairos") { NSApp.terminate(nil) }.keyboardShortcut("q")
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static private(set) var shared: AppDelegate?
    var openMainWindow: (() -> Void)?
    private var dock: DockPanelController?
    private var quickAdd: QuickAddPanelController?
    private var overlay: OverlayController?
    private var hotKeys = HotKeys()
    private var pendingHotKeys: DispatchWorkItem?
    private let systemDock = SystemDock()
    private var cancellables: Set<AnyCancellable> = []

    override init() {
        super.init()
        Self.shared = self
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let model = AppModel.shared
        model.start()
        if BridgeInstaller.installedBrowsers().isEmpty { BridgeInstaller.install() }

        dock = DockPanelController(model: model)
        quickAdd = QuickAddPanelController(model: model)
        overlay = OverlayController(model: model)
        registerHotKeys(groups: model.data.settings.groupHotkeys)

        model.$data.map(\.settings.groupHotkeys).removeDuplicates().dropFirst()
            .sink { [weak self] on in self?.registerHotKeys(groups: on) }
            .store(in: &cancellables)
        // Hiding Kairos Bar, by any route, brings the macOS Dock back until the bar returns.
        model.$data.map(\.settings.hidesSystemDock).removeDuplicates()
            .sink { [weak self] hide in
                guard let self else { return }
                if hide { self.systemDock.replace() } else { self.systemDock.restore() }
            }
            .store(in: &cancellables)
        if ProcessInfo.processInfo.environment["KAIROS_START_FOCUS"] == "1" { model.startFocus() } // scripts/bridge-check.py
    }

    /// Switches Mission Control desktop by sending the system's own Control-number shortcut.
    /// Kairos's Control-number hotkeys are released first, or they would swallow the keystroke.
    /// Quick switches share one pending re-register, 0.35 s after the last keystroke is sent.
    func switchDesktop(_ number: Int) {
        hotKeys.removeAll()
        pendingHotKeys?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.pendingHotKeys = nil
            self?.registerHotKeys(groups: AppModel.shared.data.settings.groupHotkeys)
        }
        pendingHotKeys = work
        let sent = Spaces.switchTo(number)
        DispatchQueue.main.asyncAfter(deadline: .now() + (sent ? 0.35 : 0), execute: work)
    }

    /// The macOS Dock comes back whenever Kairos is not running to stand in for it.
    func applicationWillTerminate(_ notification: Notification) {
        systemDock.restore()
        AppModel.shared.shutdown()
    }

    // Closing the main window leaves the dock and menu bar item running.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func showQuickAdd() { quickAdd?.show() }

    func showMainWindow() {
        NSApp.activate(ignoringOtherApps: true)
        if let open = openMainWindow { open(); return }
        NSApp.windows.first { $0.identifier?.rawValue.hasPrefix("main") == true }?.makeKeyAndOrderFront(nil)
    }

    private func registerHotKeys(groups: Bool) {
        // A desktop switch in flight re-registers when it is done.
        guard pendingHotKeys == nil else { return }
        hotKeys.removeAll()
        hotKeys.register(keyCode: UInt32(kVK_Space), modifiers: UInt32(optionKey)) { [weak self] in self?.quickAdd?.toggle() }
        // Start, pause or resume the timer from anywhere.
        hotKeys.register(keyCode: UInt32(kVK_Space), modifiers: UInt32(optionKey | shiftKey)) { AppModel.shared.toggleTimer() }
        guard groups else { return }
        let digits = [kVK_ANSI_1, kVK_ANSI_2, kVK_ANSI_3, kVK_ANSI_4, kVK_ANSI_5, kVK_ANSI_6, kVK_ANSI_7, kVK_ANSI_8, kVK_ANSI_9]
        for (i, code) in digits.enumerated() {
            hotKeys.register(keyCode: UInt32(code), modifiers: UInt32(controlKey)) { AppModel.shared.switchGroup(index: i) }
        }
    }
}

/// Global hotkeys through Carbon, which needs no Accessibility permission.
@MainActor
final class HotKeys {
    private var refs: [EventHotKeyRef] = []
    private static var handlers: [UInt32: () -> Void] = [:]
    private static var installed = false
    private static var nextID: UInt32 = 1

    func register(keyCode: UInt32, modifiers: UInt32, action: @escaping () -> Void) {
        Self.installHandler()
        let id = Self.nextID
        Self.nextID += 1
        var ref: EventHotKeyRef?
        let hkID = EventHotKeyID(signature: OSType(0x4B414952), id: id) // 'KAIR'
        guard RegisterEventHotKey(keyCode, modifiers, hkID, GetApplicationEventTarget(), 0, &ref) == noErr, let ref else { return }
        Self.handlers[id] = action
        refs.append(ref)
    }

    func removeAll() {
        refs.forEach { UnregisterEventHotKey($0) }
        refs = []
        Self.handlers = [:]
    }

    private static func installHandler() {
        guard !installed else { return }
        installed = true
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, event, _ in
            var hkID = EventHotKeyID()
            GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                              MemoryLayout<EventHotKeyID>.size, nil, &hkID)
            let id = hkID.id
            DispatchQueue.main.async { MainActor.assumeIsolated { HotKeys.handlers[id]?() } }
            return noErr
        }, 1, &spec, nil, nil)
    }
}
