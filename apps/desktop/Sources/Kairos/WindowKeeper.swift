import AppKit
import ApplicationServices
import KairosCore

/// Keeps other apps' windows clear of Kairos Bar while it is pinned open, through the
/// Accessibility API. WindowClearance decides each frame; this only reads windows, watches for
/// them to appear, move or resize, and applies the decision. Without the Accessibility
/// permission it does nothing.
@MainActor
final class WindowKeeper {
    /// The bar's strip and the screen area, in Accessibility coordinates (top-left origin).
    struct Target: Equatable {
        let strip: CGRect
        let edge: DockEdge
        let area: CGRect
    }

    private var target: Target?
    private var observers: [pid_t: AXObserver] = [:]
    private var workspace: [NSObjectProtocol] = []
    private var mouseUp: Any?
    /// Apps with a window that moved or resized while a mouse button was down: the owner may be
    /// dragging it, so it waits for the button to come up.
    private var pending: Set<pid_t> = []
    /// The frame each window was left at after Kairos set it, so an app that refuses a frame
    /// is not asked again and again.
    private var leftAt: [CFHashCode: CGRect] = [:]

    init() {
        // An app that stops answering must not stall Kairos.
        AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 0.25)
    }

    /// Nil, or no Accessibility permission, stops it.
    func update(_ next: Target?) {
        let wanted = AXIsProcessTrusted() ? next : nil
        guard wanted != target else { return }
        target = wanted
        if wanted == nil {
            stop()
        } else {
            start()
            sweep()
        }
    }

    private func start() {
        guard workspace.isEmpty else { return }
        let wc = NSWorkspace.shared.notificationCenter
        workspace.append(wc.addObserver(forName: NSWorkspace.didLaunchApplicationNotification, object: nil, queue: .main) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated { if let app { self?.observe(app) } }
        })
        workspace.append(wc.addObserver(forName: NSWorkspace.didTerminateApplicationNotification, object: nil, queue: .main) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated { if let app { self?.forget(app.processIdentifier) } }
        })
        workspace.append(wc.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            MainActor.assumeIsolated { if let app { self?.sweep(app.processIdentifier) } }
        })
        mouseUp = NSEvent.addGlobalMonitorForEvents(matching: .leftMouseUp) { [weak self] _ in
            MainActor.assumeIsolated { self?.flushPending() }
        }
        NSWorkspace.shared.runningApplications.forEach(observe)
    }

    private func stop() {
        workspace.forEach(NSWorkspace.shared.notificationCenter.removeObserver)
        workspace = []
        if let mouseUp { NSEvent.removeMonitor(mouseUp) }
        mouseUp = nil
        for pid in Array(observers.keys) { forget(pid) }
        pending = []
        leftAt = [:]
    }

    private func observe(_ app: NSRunningApplication) {
        let pid = app.processIdentifier
        guard app.activationPolicy == .regular, pid != getpid(), observers[pid] == nil else { return }
        var made: AXObserver?
        guard AXObserverCreate(pid, windowChanged, &made) == .success, let observer = made else { return }
        let element = AXUIElementCreateApplication(pid)
        let refcon = Unmanaged.passUnretained(self).toOpaque()
        for name in [kAXWindowCreatedNotification, kAXWindowMovedNotification, kAXWindowResizedNotification, kAXWindowDeminiaturizedNotification] {
            AXObserverAddNotification(observer, element, name as CFString, refcon)
        }
        CFRunLoopAddSource(CFRunLoopGetMain(), AXObserverGetRunLoopSource(observer), .defaultMode)
        observers[pid] = observer
    }

    private func forget(_ pid: pid_t) {
        guard let observer = observers.removeValue(forKey: pid) else { return }
        CFRunLoopRemoveSource(CFRunLoopGetMain(), AXObserverGetRunLoopSource(observer), .defaultMode)
    }

    fileprivate func changed(_ window: AXUIElement) {
        guard target != nil else { return }
        var pid: pid_t = 0
        AXUIElementGetPid(window, &pid)
        guard pid != getpid() else { return }
        if NSEvent.pressedMouseButtons & 1 != 0 {
            pending.insert(pid)
        } else {
            clear(window)
        }
    }

    private func flushPending() {
        let pids = pending
        pending = []
        pids.forEach(sweep)
    }

    /// Every window of every app, or of one app.
    private func sweep(_ only: pid_t? = nil) {
        guard target != nil else { return }
        let apps = NSWorkspace.shared.runningApplications.filter {
            $0.activationPolicy == .regular && !$0.isHidden && $0.processIdentifier != getpid()
                && (only == nil || $0.processIdentifier == only)
        }
        for app in apps {
            var value: CFTypeRef?
            guard AXUIElementCopyAttributeValue(AXUIElementCreateApplication(app.processIdentifier), kAXWindowsAttribute as CFString, &value) == .success,
                  let windows = value as? [AXUIElement] else { continue }
            windows.forEach(clear)
        }
    }

    private func clear(_ window: AXUIElement) {
        guard let target, let now = Self.read(window),
              let next = WindowClearance.clear(now, of: target.strip, edge: target.edge, area: target.area) else { return }
        let key = CFHash(window)
        guard leftAt[key] != now.frame else { return }
        // Size, position, then size again: some apps clamp a size against the old position.
        if next.size != now.frame.size { Self.set(window, kAXSizeAttribute, next.size) }
        if next.origin != now.frame.origin { Self.set(window, kAXPositionAttribute, next.origin) }
        if next.size != now.frame.size { Self.set(window, kAXSizeAttribute, next.size) }
        leftAt[key] = Self.read(window)?.frame
    }

    // MARK: Accessibility values

    private static func read(_ window: AXUIElement) -> WindowClearance.Window? {
        guard let origin: CGPoint = value(window, kAXPositionAttribute, .cgPoint),
              let size: CGSize = value(window, kAXSizeAttribute, .cgSize) else { return nil }
        return WindowClearance.Window(frame: CGRect(origin: origin, size: size),
                                      canMove: settable(window, kAXPositionAttribute),
                                      canResize: settable(window, kAXSizeAttribute),
                                      isStandard: attribute(window, kAXSubroleAttribute) as? String == kAXStandardWindowSubrole,
                                      isFullScreen: attribute(window, "AXFullScreen") as? Bool ?? false,
                                      isMinimized: attribute(window, kAXMinimizedAttribute) as? Bool ?? false)
    }

    private static func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
        var value: CFTypeRef?
        return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
    }

    private static func value<T>(_ element: AXUIElement, _ name: String, _ type: AXValueType) -> T? {
        guard let raw = attribute(element, name), CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
        let ax = raw as! AXValue
        let out = UnsafeMutablePointer<T>.allocate(capacity: 1)
        defer { out.deallocate() }
        return AXValueGetValue(ax, type, out) ? out.pointee : nil
    }

    private static func settable(_ element: AXUIElement, _ name: String) -> Bool {
        var ok: DarwinBoolean = false
        return AXUIElementIsAttributeSettable(element, name as CFString, &ok) == .success && ok.boolValue
    }

    private static func set(_ element: AXUIElement, _ name: String, _ size: CGSize) {
        var v = size
        if let value = AXValueCreate(.cgSize, &v) { AXUIElementSetAttributeValue(element, name as CFString, value) }
    }

    private static func set(_ element: AXUIElement, _ name: String, _ point: CGPoint) {
        var v = point
        if let value = AXValueCreate(.cgPoint, &v) { AXUIElementSetAttributeValue(element, name as CFString, value) }
    }
}

/// Accessibility calls back on the main run loop, where the observers' sources are added.
private func windowChanged(_ observer: AXObserver, _ element: AXUIElement, _ notification: CFString, _ refcon: UnsafeMutableRawPointer?) {
    guard let refcon else { return }
    let keeper = Unmanaged<WindowKeeper>.fromOpaque(refcon).takeUnretainedValue()
    MainActor.assumeIsolated { keeper.changed(element) }
}
