import AppKit
import KairosCore
import SwiftUI

/// Renders every Kairos view offscreen to PNG files, against a scratch store inside the output
/// folder. Run `scripts/snapshot.sh [dir]`, or `Kairos --snapshot <dir> [--light]`.
///
/// Dock shots sit on a stand-in wallpaper. Red outlines the panel, green the zone where the
/// pointer engages the bar (the only place the panel takes clicks), and a yellow dot marks the
/// simulated pointer.
@MainActor
enum Snapshot {
    static func run(into dir: URL) {
        NSApplication.shared.setActivationPolicy(.accessory)
        let dark = !CommandLine.arguments.contains("--light")
        NSApp.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        let fm = FileManager.default
        try? fm.removeItem(at: dir)
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let store = DataStore(url: dir.appendingPathComponent(".scratch/kairos.json"))
        let model = AppModel(store: store, readKeys: false)
        model.data = sample()
        var log: [String] = []

        func shot<V: View>(_ name: String, size: CGSize? = nil, _ view: V) {
            let (px, focus) = render(AnyView(view.environmentObject(model)), size: size, to: dir.appendingPathComponent("\(name).png"))
            log.append("\(name).png \(Int(px.width))x\(Int(px.height))" + (focus.map { " focus=\($0)" } ?? ""))
        }

        func dock(_ name: String, pointer: DockGeometry.Slot? = nil, offset: Double = 0, reveal: Double = 1,
                  reduceMotion: Bool = false, surface: DockSurface = .material, launching: String? = nil) {
            let frame = DockFrame(data: model.data, running: model.openApps, reduceMotion: reduceMotion)
            let rest = frame.geometry.layout(pointer: nil).items
            let p = pointer.flatMap { slot in rest.first { $0.slot == slot }?.center }.map { $0 + offset }
            let light = surface == .lightGlassStandIn
            let tracker = DockTracker(frame: frame, pointer: p, reveal: reveal)
            if let launching { tracker.launches.start(launching, now: Date()) }
            shot("dock-\(name)", DockStage(tracker: tracker,
                                           autoHide: model.data.settings.autoHideBar, light: light)
                .environment(\.dockSurface, surface)
                .environment(\.colorScheme, light ? .light : (NSApp.appearance?.name == .aqua ? .light : .dark)))
        }

        let work = model.data.groups[0]
        let current = model.data.focus.currentTaskId
        for edge in DockEdge.allCases {
            model.data.settings.dockEdge = edge
            for size in [20.0, 36, 64] {
                model.data.settings.iconSize = size
                dock("\(edge.rawValue)-\(Int(size))")
            }
            model.data.settings.iconSize = 36
            dock("\(edge.rawValue)-36-hover-app", pointer: .app("com.apple.Safari"))
            dock("\(edge.rawValue)-36-hover-between", pointer: .app("com.apple.Safari"), offset: 22)
            dock("\(edge.rawValue)-36-hover-group", pointer: .group(work.id))
            dock("\(edge.rawValue)-36-hover-last", pointer: .app("com.apple.iCal"))
        }
        // Auto-hide: off screen, mid-slide (the eased halfway point), and back with the pointer on it.
        model.data.settings.autoHideBar = true
        for edge in DockEdge.allCases {
            model.data.settings.dockEdge = edge
            dock("\(edge.rawValue)-36-autohide-hidden", reveal: 0)
            dock("\(edge.rawValue)-36-autohide-sliding", reveal: 0.5)
            dock("\(edge.rawValue)-36-autohide-revealed", pointer: .app("com.apple.Safari"))
        }
        model.data.settings.autoHideBar = false
        model.data.settings.dockEdge = .left
        model.data.settings.iconSize = 64
        dock("left-64-hover-app", pointer: .app("com.apple.Notes"))
        model.data.settings.iconSize = 20
        dock("left-20-hover-app", pointer: .app("com.apple.Notes"))
        model.data.settings.iconSize = 36
        dock("left-36-hover-new-group", pointer: .newGroup)

        var empty = AppGroup(name: "Empty", icon: "pencil", colorHex: GroupPalette.colors[3])
        empty.dockApps = []
        model.data.groups.append(empty)
        model.switchGroup(empty.id)
        dock("left-36-empty-group")
        dock("left-36-empty-group-hover-add", pointer: .addApp)
        for (name, icon) in [("Design", "paintbrush.fill"), ("Music", "music.note"), ("Admin", "hammer.fill"), ("Games", "gamecontroller.fill")] {
            model.data.groups.append(AppGroup(name: name, icon: icon, colorHex: GroupPalette.colors[model.data.groups.count % 8]))
        }
        model.switchGroup(work.id)
        dock("left-36-many-groups")
        model.data.settings.dockEdge = .bottom
        dock("bottom-36-many-groups", pointer: .group(model.data.groups[5].id))
        model.data.settings.dockEdge = .left
        model.data.groups.removeLast(5)
        model.data.focus.currentTaskId = current
        // Open apps: running dots, and an app that is not in the group after a separator.
        model.showRunningForSnapshot(["com.apple.Safari", "com.apple.Notes", "com.hnc.Discord"])
        dock("left-36-running")
        dock("left-36-running-light-glass", surface: .lightGlassStandIn)
        model.data.settings.dockEdge = .bottom
        dock("bottom-36-running-light-glass", surface: .lightGlassStandIn)
        model.data.settings.dockEdge = .left
        dock("left-36-hover-trash", pointer: .folder(.trash))
        dock("left-36-launching", launching: "com.apple.iCal")
        // Reduce motion: no magnification under the pointer, and auto-hide fades instead of sliding.
        dock("left-36-hover-app-reduce-motion", pointer: .app("com.apple.Safari"), reduceMotion: true)
        model.data.settings.autoHideBar = true
        dock("left-36-autohide-sliding-reduce-motion", reveal: 0.5, reduceMotion: true)
        model.data.settings.autoHideBar = false
        model.showRunningForSnapshot([])

        for page in Page.allCases {
            model.page = page
            model.selectedGroupId = work.id
            shot("main-\(page.rawValue.lowercased())", size: CGSize(width: 1000, height: 660), MainWindow())
            shot("main-\(page.rawValue.lowercased())-min", size: CGSize(width: 860, height: 560), MainWindow())
        }
        // Every Settings section at once, tall enough that nothing scrolls.
        model.page = .settings
        shot("main-settings-full", size: CGSize(width: 1000, height: 3000), MainWindow())
        model.hotkeyFailures = ["⌥Space"]
        shot("main-settings-hotkey-taken", size: CGSize(width: 1000, height: 660), MainWindow())
        model.hotkeyFailures = []
        model.page = .today
        model.setMode(.focus)
        shot("main-today-focus", size: CGSize(width: 1000, height: 660), MainWindow())
        dock("left-36-focus-no-shield", pointer: .group(work.id))
        model.setMode(.off)
        // A focus phase 10 minutes in, then a break that ran out with the timer waiting.
        let preset = model.data.preset(named: work.tempoPreset)
        var running = TempoEngine(preset: preset)
        running.start(.focus, now: model.now.addingTimeInterval(-600))
        model.showTimerForSnapshot(running)
        shot("main-today-timer", size: CGSize(width: 1000, height: 660), MainWindow())
        var waiting = TempoEngine(preset: preset)
        waiting.start(.shortBreak, now: model.now.addingTimeInterval(-Double(preset.shortBreakMin * 60) - 1))
        waiting.tick(now: model.now, startsFocusAfterBreak: false)
        model.showTimerForSnapshot(waiting)
        model.setMode(.off) // apply(.awaitingFocus) turns the mode off
        shot("main-today-break-over", size: CGSize(width: 1000, height: 660), MainWindow())
        model.showTimerForSnapshot(TempoEngine(preset: preset))
        model.setMode(.off)
        let added = model.addGroup()
        model.page = .groups
        model.selectedGroupId = added
        model.renameGroupId = added
        shot("main-groups-new-group", size: CGSize(width: 1000, height: 660), MainWindow())
        model.deleteGroup(added)
        shot("main-tasks-selected", size: CGSize(width: 1000, height: 660),
             TasksPage(selected: model.data.tasks.first?.id))
        shot("onboarding", OnboardingSheet().background(Color(nsColor: .windowBackgroundColor)))
        shot("quickadd-empty", QuickAddView(close: {}).padding(30).background(Wallpaper()))
        shot("quickadd-typed", QuickAddView(close: {}, text: "Draft blog post tomorrow 3pm @Work ~45m !high").padding(30).background(Wallpaper()))

        model.setMode(.focus)
        model.notice = BlockNotice(kind: .app(bundleId: "com.hnc.Discord"), title: "Discord is hidden",
                                   detail: "Discord is blocked in Work.")
        shot("overlay-block", size: CGSize(width: 900, height: 600), OverlayView().background(Wallpaper()))
        shot("overlay-block-reason-typed", size: CGSize(width: 900, height: 600), OverlayView(reason: "Need the API docs").background(Wallpaper()))
        let allowed = model.data.settings.maxOverridesPerSession
        model.data.settings.maxOverridesPerSession = 0 // overrides used up: no "Allow"
        shot("overlay-block-no-overrides", size: CGSize(width: 900, height: 600), OverlayView().background(Wallpaper()))
        model.data.settings.maxOverridesPerSession = allowed
        model.notice = BlockNotice(kind: .rest, title: "Short break", detail: "Step away from the screen.")
        shot("overlay-rest", size: CGSize(width: 900, height: 600), OverlayView().background(Wallpaper()))

        let confirm = StrictConfirmation(minutes: 50, endsAt: Date().addingTimeInterval(50 * 60))
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = confirm.title
        alert.informativeText = confirm.message
        alert.addButton(withTitle: "Start Strict Focus")
        alert.addButton(withTitle: "Cancel")
        log.append(renderAlert(alert, to: dir.appendingPathComponent("alert-strict-confirm.png")))

        try? (log.joined(separator: "\n") + "\n").write(to: dir.appendingPathComponent("index.txt"), atomically: true, encoding: .utf8)
        print(log.joined(separator: "\n"))
        print("Wrote \(log.count) snapshots to \(dir.path)")
        exit(0)
    }

    /// Lays the view out in an offscreen window, lets SwiftUI settle, then caches its display.
    /// Returns the pixel size and, when a text field has keyboard focus, which one. Focus does
    /// not draw while the tool's app is inactive, so it is reported instead.
    private static func render(_ view: AnyView, size: CGSize?, to url: URL) -> (CGSize, String?) {
        let host = NSHostingView(rootView: view)
        let window = SnapshotWindow(contentRect: NSRect(x: -30000, y: -30000, width: size?.width ?? 100, height: size?.height ?? 100),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSApp.appearance
        window.contentView = host
        if size == nil {
            host.layoutSubtreeIfNeeded()
            window.setContentSize(host.fittingSize)
        }
        window.makeKeyAndOrderFront(nil)
        for _ in 0..<6 {
            RunLoop.main.run(until: Date().addingTimeInterval(0.06))
            host.layoutSubtreeIfNeeded()
        }
        if size == nil, host.fittingSize != host.frame.size {
            window.setContentSize(host.fittingSize)
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        }
        let bounds = host.bounds
        var focus: String?
        if let editor = window.firstResponder as? NSTextView, editor.isFieldEditor {
            focus = "\"\(editor.string)\" selected=\(editor.selectedRange().length)"
        }
        guard let rep = host.bitmapImageRepForCachingDisplay(in: bounds) else { return (.zero, focus) }
        host.cacheDisplay(in: bounds, to: rep)
        // The hosting view does not draw the window background, so lay it on an opaque one.
        let image = NSImage(size: bounds.size)
        image.addRepresentation(rep)
        let out = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: rep.pixelsWide, pixelsHigh: rep.pixelsHigh, bitsPerSample: 8,
                                   samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        out.size = bounds.size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: out)
        window.appearance?.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            bounds.fill()
        }
        image.draw(in: bounds)
        NSGraphicsContext.restoreGraphicsState()
        try? out.representation(using: .png, properties: [:])?.write(to: url)
        // Earlier windows would keep observing the model and react to later scenes' state.
        window.orderOut(nil)
        window.contentView = nil
        window.close()
        return (CGSize(width: rep.pixelsWide, height: rep.pixelsHigh), focus)
    }

    /// An NSAlert as the app shows it, laid out but never run.
    private static func renderAlert(_ alert: NSAlert, to url: URL) -> String {
        alert.window.appearance = NSApp.appearance
        alert.layout()
        guard let view = alert.window.contentView, let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            return "\(url.lastPathComponent) failed"
        }
        alert.window.setFrameOrigin(NSPoint(x: -30000, y: -30000))
        alert.window.orderFront(nil)
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        view.cacheDisplay(in: view.bounds, to: rep)
        alert.window.orderOut(nil)
        let image = NSImage(size: view.bounds.size)
        image.addRepresentation(rep)
        let out = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: rep.pixelsWide, pixelsHigh: rep.pixelsHigh, bitsPerSample: 8,
                                   samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        out.size = view.bounds.size
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: out)
        alert.window.appearance?.performAsCurrentDrawingAppearance {
            NSColor.windowBackgroundColor.setFill()
            view.bounds.fill()
        }
        image.draw(in: view.bounds)
        NSGraphicsContext.restoreGraphicsState()
        try? out.representation(using: .png, properties: [:])?.write(to: url)
        return "\(url.lastPathComponent) \(rep.pixelsWide)x\(rep.pixelsHigh)"
    }

    private static func sample() -> AppData {
        var d = AppData.starter()
        d.onboarded = true
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        let work = d.groups[0].id, study = d.groups[1].id, personal = d.groups[2].id
        func task(_ title: String, _ group: UUID, dueHour: Int? = nil, est: Int? = nil, priority: Int = 0) -> TaskItem {
            var t = TaskItem(title: title, groupId: group)
            t.due = dueHour.flatMap { cal.date(byAdding: .hour, value: $0, to: today) }
            t.estimateMin = est
            t.priority = priority
            return t
        }
        var blog = task("Draft the Q4 product blog post with screenshots", work, dueHour: 15, est: 45, priority: 3)
        blog.notes = "Outline first, then the intro."
        d.tasks = [
            blog,
            task("Review open pull requests", work, dueHour: 11, est: 30, priority: 2),
            task("Read chapter 4 of Designing Data-Intensive Applications", study, dueHour: 20, est: 60),
            task("Book dentist", personal, priority: 1),
            task("Reply to Sam about the offsite", work),
        ]
        var sub = TaskItem(title: "Collect screenshots", groupId: work)
        sub.parentId = blog.id
        d.tasks.append(sub)
        var done = task("Send invoice", work)
        done.status = .done
        done.completedAt = Date()
        d.tasks.append(done)
        d.focus.currentTaskId = blog.id
        for day in 0..<7 {
            guard let start = cal.date(byAdding: .day, value: -day, to: today)?.addingTimeInterval(9 * 3600) else { continue }
            for (i, g) in [work, study, personal].enumerated() where (day + i) % 3 != 2 {
                let s = start.addingTimeInterval(Double(i) * 3600)
                d.sessions.append(Session(taskId: nil, groupId: g, start: s, end: s.addingTimeInterval(Double(25 + 10 * i) * 60),
                                          kind: .focus, interruptions: i))
            }
        }
        for (target, n) in [("youtube.com", 5), ("reddit.com", 3), ("Discord", 2)] {
            for k in 0..<n { d.blockEvents.append(BlockEvent(groupId: work, target: target, layer: "rule", overridden: k == 0)) }
        }
        return d
    }
}

/// Borderless windows cannot become key, and SwiftUI focus needs a key window, as the real
/// main window is.
private final class SnapshotWindow: NSWindow {
    override var canBecomeKey: Bool { true }
}

/// A stand-in desktop so translucent surfaces have something to sit on.
private struct Wallpaper: View {
    var light = false

    var body: some View {
        light
            ? LinearGradient(colors: [Color(red: 0.93, green: 0.94, blue: 0.97), Color(red: 0.98, green: 0.93, blue: 0.86)],
                             startPoint: .topLeading, endPoint: .bottomTrailing)
            : LinearGradient(colors: [Color(red: 0.25, green: 0.35, blue: 0.55), Color(red: 0.75, green: 0.55, blue: 0.45)],
                             startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

/// The dock as its panel would show it, with the panel, its click zone and the pointer marked.
/// With auto-hide, orange marks the strip along the screen edge that calls the bar back, and
/// the green click zone shows only while the bar is out.
private struct DockStage: View {
    @ObservedObject var tracker: DockTracker
    var autoHide = false
    var light = false

    var body: some View {
        let f = tracker.frame, g = f.geometry
        let zone = f.size(along: g.restLength, across: DockFrame.inset + g.thickness)
        let strip = f.size(along: g.restLength, across: DockAutoHide.edgeStrip)
        ZStack(alignment: .topLeading) {
            DockView(tracker: tracker).clipped() // the panel's window clips the slide
            if autoHide {
                Rectangle().fill(Color.orange)
                    .frame(width: strip.width, height: strip.height)
                    .position(f.point(axis: g.restLength / 2, cross: DockAutoHide.edgeStrip / 2))
                    .allowsHitTesting(false)
            }
            Rectangle().strokeBorder(Color.green.opacity(tracker.reveal == 1 ? 0.9 : 0), lineWidth: 1)
                .frame(width: zone.width, height: zone.height)
                .position(f.point(axis: g.restLength / 2, cross: (DockFrame.inset + g.thickness) / 2))
                .allowsHitTesting(false)
            if let p = tracker.pointer {
                Circle().fill(Color.yellow).frame(width: 5, height: 5)
                    .position(f.point(axis: p, cross: DockFrame.inset + g.thickness / 2))
            }
        }
        .border(Color.red.opacity(0.8), width: 1)
        .padding(24)
        .background(Wallpaper(light: light))
    }
}
