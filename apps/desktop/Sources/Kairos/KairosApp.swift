import AppKit
import Carbon.HIToolbox
import Combine
import KairosCore
import SwiftUI

/// `Kairos --snapshot <dir>` renders every view to PNG files instead of launching the app.
/// `Kairos --install-bridge` points the browsers' native messaging manifests at this copy's host
/// and exits (scripts/build-app.sh --install runs it).
@main
enum Entry {
    @MainActor static func main() {
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "--snapshot") {
            Snapshot.run(into: URL(fileURLWithPath: i + 1 < args.count ? args[i + 1] : "snapshots"))
        } else if args.contains("--install-bridge") {
            let browsers = BridgeInstaller.install()
            if DataStore.isScratch {
                print("KAIROS_HOME is set, so the real browser manifests were left alone.")
            } else {
                print(browsers.isEmpty ? "No Chromium browser profile found; nothing to point at \(BridgeInstaller.hostPath)."
                                       : "Kairos Shield and Kairos Read now reach \(BridgeInstaller.hostPath) in: \(browsers.joined(separator: ", ")).")
            }
            exit(0)
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
        .commands {
            CommandGroup(replacing: .appTermination) {
                QuitButton().environmentObject(model)
            }
        }

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
        if model.siteBlockingOff {
            Label("Site blocking is off: Kairos Shield isn\u{2019}t connected", systemImage: "exclamationmark.triangle")
            Divider()
        }
        if let task = model.currentTask {
            Text(task.title)
            Button("Mark done") { model.setDone(task, true) }
        } else {
            Text("No current task")
        }
        Divider()
        if model.tempo.phase == .idle {
            Button("Start focus") { model.requestStartFocus() }.keyboardShortcut(model.timerShortcut)
        } else {
            Text("\(model.tempo.phase.label) · \(clock(model.remaining))")
            Button(model.tempo.isPaused ? "Resume" : "Pause") { model.toggleTimer() }.keyboardShortcut(model.timerShortcut)
            Button("Skip") { model.requestSkip() }.disabled(model.strictLocked)
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
        ModePicker()
        Divider()
        Button("Quick add…") { AppDelegate.shared?.showQuickAdd() }.keyboardShortcut(model.quickAddShortcut)
        Button("Open Kairos") {
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }
        Toggle("Show Kairos Bar", isOn: $model.data.settings.dockVisible)
        Divider()
        QuitButton()
    }
}

/// Quit is off while a strict focus phase is locked; the reason shows where the button was.
struct QuitButton: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        if model.strictLocked, let until = model.data.focus.strictUntil {
            Text("Strict until \(until.formatted(date: .omitted, time: .shortened)): Kairos can\u{2019}t be quit")
        }
        Button("Quit Kairos") { NSApp.terminate(nil) }.keyboardShortcut("q").disabled(model.strictLocked)
    }
}

/// The one Mode control, in the menu bar menu and on Today. While a timer runs the timer owns
/// the mode, so the control is locked (StrictMode.tla, PickerLockedByTimer).
struct ModePicker: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        Picker("Mode", selection: Binding(get: { model.mode }, set: { model.setMode($0) })) {
            ForEach(Mode.allCases, id: \.self) { Text($0.label).tag($0) }
        }
        .disabled(model.tempo.phase != .idle)
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
    private static let dockRestoreScript = DataStore.defaultDirectory.appendingPathComponent("dock-restore.sh")
    private let systemDock = SystemDock(restoreScript: AppDelegate.dockRestoreScript)
    private let dockWatchdog = DockWatchdog(
        agents: FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/LaunchAgents"),
        restoreScript: AppDelegate.dockRestoreScript)
    private var signalTrap: SignalTrap?
    private var cancellables: Set<AnyCancellable> = []

    override init() {
        super.init()
        Self.shared = self
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let model = AppModel.shared
        model.start()
        // `kill`, Ctrl-C or a closed terminal would otherwise end Kairos with the Dock still hidden.
        signalTrap = SignalTrap { [weak self] _ in self?.endOnSignal() }
        if BridgeInstaller.installedBrowsers().isEmpty { BridgeInstaller.install() }

        dock = DockPanelController(model: model)
        quickAdd = QuickAddPanelController(model: model)
        overlay = OverlayController(model: model)
        model.confirmStrict = { confirmation in
            NSApp.activate(ignoringOtherApps: true)
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = confirmation.title
            alert.informativeText = confirmation.message
            alert.addButton(withTitle: "Start Strict Focus")
            alert.addButton(withTitle: "Cancel")
            return alert.runModal() == .alertFirstButtonReturn
        }
        registerHotKeys()

        model.$data.map { HotkeyPlan($0.settings) }.removeDuplicates().dropFirst()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.registerHotKeys() } }
            .store(in: &cancellables)
        // Hiding Kairos Bar, by any route, brings the macOS Dock back until the bar returns.
        model.$data.map(\.settings.hidesSystemDock).removeDuplicates()
            .sink { [weak self] hide in
                guard let self, !DataStore.isScratch else { return }
                if hide { self.systemDock.replace() } else { self.systemDock.restore() }
            }
            .store(in: &cancellables)
        // The watchdog brings the Dock back after a crash or force quit. It exists while the switch is on.
        model.$data.map(\.settings.replacesSystemDock).removeDuplicates()
            .sink { [weak self] on in
                guard let self, !DataStore.isScratch else { return }
                if on { self.dockWatchdog.install() } else { self.dockWatchdog.remove() }
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
            self?.registerHotKeys()
        }
        pendingHotKeys = work
        let sent = Spaces.switchTo(number)
        DispatchQueue.main.asyncAfter(deadline: .now() + (sent ? 0.35 : 0), execute: work)
    }

    /// A strict focus phase cannot be quit. Logging out, restarting or shutting down still
    /// quits, because refusing would hold up the Mac; the phase resumes at the next launch.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard AppModel.shared.strictLocked, !Self.systemIsQuitting else { return .terminateNow }
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = "Strict focus is on"
        let until = AppModel.shared.data.focus.strictUntil?.formatted(date: .omitted, time: .shortened) ?? "the session ends"
        alert.informativeText = "Kairos can\u{2019}t be quit until \(until), when this strict session ends."
        alert.runModal()
        return .terminateCancel
    }

    private static var systemIsQuitting: Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent,
              let reason = event.attributeDescriptor(forKeyword: AEKeyword(kAEQuitReason))?.enumCodeValue else { return false }
        return [kAELogOut, kAEReallyLogOut, kAEShowRestartDialog, kAEShowShutdownDialog, kAERestart, kAEShutDown]
            .map { OSType($0) }.contains(reason)
    }

    /// The macOS Dock comes back whenever Kairos is not running to stand in for it.
    func applicationWillTerminate(_ notification: Notification) {
        if !DataStore.isScratch { systemDock.restore() }
        AppModel.shared.shutdown()
    }

    /// A termination signal cannot be refused, in Strict either: like a force quit, a strict
    /// phase resumes at the next launch.
    private func endOnSignal() {
        if !DataStore.isScratch { systemDock.restore() }
        AppModel.shared.shutdown()
        exit(0)
    }

    // Closing the main window leaves the dock and menu bar item running.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func showQuickAdd() { quickAdd?.show() }

    func showMainWindow() {
        NSApp.activate(ignoringOtherApps: true)
        if let open = openMainWindow { open(); return }
        NSApp.windows.first { $0.identifier?.rawValue.hasPrefix("main") == true }?.makeKeyAndOrderFront(nil)
    }

    private func registerHotKeys() {
        // A desktop switch in flight re-registers when it is done.
        guard pendingHotKeys == nil else { return }
        hotKeys.removeAll()
        var failed: [String] = []
        for entry in HotkeyPlan(AppModel.shared.data.settings).entries {
            let action: () -> Void
            switch entry.action {
            case .quickAdd: action = { [weak self] in self?.quickAdd?.toggle() }
            case .timer: action = { AppModel.shared.toggleTimer() }
            case let .group(i): action = { AppModel.shared.switchGroup(index: i) }
            }
            if !hotKeys.register(keyCode: entry.keyCode, modifiers: entry.modifiers, action: action) { failed.append(entry.label) }
        }
        AppModel.shared.hotkeyFailures = failed
    }
}

/// Global hotkeys through Carbon, which needs no Accessibility permission.
@MainActor
final class HotKeys {
    private var refs: [EventHotKeyRef] = []
    private static var handlers: [UInt32: () -> Void] = [:]
    private static var installed = false
    private static var nextID: UInt32 = 1

    /// False when macOS refused, usually because another app holds the same shortcut.
    @discardableResult
    func register(keyCode: UInt32, modifiers: UInt32, action: @escaping () -> Void) -> Bool {
        Self.installHandler()
        let id = Self.nextID
        Self.nextID += 1
        var ref: EventHotKeyRef?
        let hkID = EventHotKeyID(signature: OSType(0x4B414952), id: id) // 'KAIR'
        guard RegisterEventHotKey(keyCode, modifiers, hkID, GetApplicationEventTarget(), 0, &ref) == noErr, let ref else { return false }
        Self.handlers[id] = action
        refs.append(ref)
        return true
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
