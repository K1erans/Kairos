import AppKit
import Combine
import KairosCore
import SwiftUI
import UniformTypeIdentifiers

// Kairos Bar: a Dock-style bar on a screen edge. Group buttons, a "+" for a new group, a
// divider that resizes the bar when dragged, then the active group's apps.
//
// The panel is larger than the bar, with room for magnified icons and the name label. It
// ignores the mouse everywhere except while the pointer is on the bar, so that room never
// swallows clicks meant for the windows behind it. The controller tracks the pointer and
// feeds it to the view, which lays everything out from `DockGeometry`.

final class FloatingPanel: NSPanel {
    var allowsKey = false
    override var canBecomeKey: Bool { allowsKey }
    override var canBecomeMain: Bool { false }
}

/// The panel's size and the mapping between view points and bar coordinates (along the bar
/// from the leading end of the bar at rest, and across it from the screen edge).
struct DockFrame: Equatable {
    let geometry: DockGeometry
    let edge: DockEdge
    let motion: DockMotion
    let lead: Double, trail: Double
    /// Gap between the screen edge and the bar.
    static let inset = 4.0

    init(data: AppData, running: [String] = [], reduceMotion: Bool = false) {
        self.init(Inputs(data: data, running: running, reduceMotion: reduceMotion))
    }

    init(_ inputs: Inputs) {
        motion = DockMotion(reduced: inputs.reduceMotion)
        geometry = DockGeometry(slots: inputs.slots, icon: inputs.icon, maxScale: motion.maxScale)
        edge = inputs.edge
        (lead, trail) = geometry.overflow
    }

    /// What the frame depends on, compared before building one, because `overflow` samples
    /// every pointer position.
    struct Inputs: Equatable {
        let slots: [DockGeometry.Slot], icon: Double, edge: DockEdge, reduceMotion: Bool
        init(data: AppData, running: [String], reduceMotion: Bool) {
            slots = DockGeometry.slots(for: data, running: running)
            icon = data.settings.iconSize
            edge = data.settings.dockEdge
            self.reduceMotion = reduceMotion
        }
    }

    var labelLane: Double { edge == .bottom ? 44 : 220 }
    var axisLength: Double { lead + geometry.restLength + trail }
    var crossLength: Double { Self.inset + geometry.thickness + geometry.headroom + labelLane }
    var size: CGSize { edge == .bottom ? CGSize(width: axisLength, height: crossLength) : CGSize(width: crossLength, height: axisLength) }

    func point(axis: Double, cross: Double) -> CGPoint {
        switch edge {
        case .left: return CGPoint(x: cross, y: lead + axis)
        case .right: return CGPoint(x: crossLength - cross, y: lead + axis)
        case .bottom: return CGPoint(x: lead + axis, y: crossLength - cross)
        }
    }

    func bar(_ p: CGPoint) -> (axis: Double, cross: Double) {
        switch edge {
        case .left: return (p.y - lead, p.x)
        case .right: return (p.y - lead, crossLength - p.x)
        case .bottom: return (p.x - lead, crossLength - p.y)
        }
    }

    /// A size given as (along the bar, across it), turned to the screen.
    func size(along: Double, across: Double) -> CGSize {
        edge == .bottom ? CGSize(width: along, height: across) : CGSize(width: across, height: along)
    }
}

/// Pointer and frame state shared by the controller and the view.
@MainActor
final class DockTracker: ObservableObject {
    @Published var frame: DockFrame
    /// The pointer along the bar while it is on the bar, else nil.
    @Published var pointer: Double?
    @Published var resizing = false
    /// An app is being dragged over the bar.
    @Published var dropTargeted = false
    /// How much of the bar is on screen: 1 unless auto-hide has slid it off its edge.
    @Published var reveal: Double
    /// Apps clicked on the bar that are still starting, which pulse.
    @Published var launches = DockLaunches()
    @Published var trashFull = false

    init(frame: DockFrame, pointer: Double? = nil, reveal: Double = 1) {
        self.frame = frame
        self.pointer = pointer
        self.reveal = reveal
    }
}

/// Reports pointer movement over the panel even though the panel never becomes key.
final class DockHostingView: NSHostingView<AnyView> {
    var onPointer: (() -> Void)?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.filter { $0.owner === self && $0.userInfo?["dock"] != nil }.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self, userInfo: ["dock": true]))
    }

    override func mouseMoved(with event: NSEvent) { super.mouseMoved(with: event); onPointer?() }
    override func mouseEntered(with event: NSEvent) { super.mouseEntered(with: event); onPointer?() }
    override func mouseExited(with event: NSEvent) { super.mouseExited(with: event); onPointer?() }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

@MainActor
final class DockPanelController {
    private let panel: FloatingPanel
    private let model: AppModel
    private let tracker: DockTracker
    private var cancellables: Set<AnyCancellable> = []
    private var monitors: [Any] = []
    /// Whether the bar is out: pinned, auto-hiding, or out of the way of a full-screen app.
    private var visibility = DockAutoHide(rule: .pinned, now: Date())
    /// A full-screen app has the active Space on the bar's screen.
    private var fullScreen = false
    private var rule: DockAutoHide.Rule {
        let s = model.data.settings
        return .init(autoHide: s.autoHideBar, hidesInFullScreen: s.hidesInFullScreen, fullScreen: fullScreen)
    }
    private let keeper = WindowKeeper()
    private var menuOpen = false
    private var frameTimer: Timer?
    private var decisionTimer: Timer?
    /// System Settings > Accessibility > Display > Reduce motion.
    private let reduceMotion = CurrentValueSubject<Bool, Never>(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)

    init(model: AppModel) {
        self.model = model
        tracker = DockTracker(frame: DockFrame(data: model.data, running: model.openApps, reduceMotion: reduceMotion.value))
        panel = FloatingPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.ignoresMouseEvents = true
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary]
        let host = DockHostingView(rootView: AnyView(DockView(tracker: tracker).environmentObject(model)))
        host.onPointer = { [weak self] in self?.pointerMoved() }
        panel.contentView = host

        // @Published emits before the property changes, so use the emitted value.
        NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification)
            .sink { [weak self] _ in self?.reduceMotion.send(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion) }
            .store(in: &cancellables)
        model.$data.combineLatest(model.$openApps, reduceMotion.removeDuplicates())
            .map { DockFrame.Inputs(data: $0, running: $1, reduceMotion: $2) }
            .removeDuplicates()
            .dropFirst()
            .map(DockFrame.init)
            .sink { [weak self] frame in self?.tracker.frame = frame; self?.layout() }
            .store(in: &cancellables)
        model.$data.map(\.settings.dockVisible).removeDuplicates()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.layout() } }
            .store(in: &cancellables)
        NotificationCenter.default.publisher(for: NSApplication.didChangeScreenParametersNotification)
            .sink { [weak self] _ in self?.layout(); self?.spaceMayHaveChanged() }
            .store(in: &cancellables)
        // A swipe to another Space, or an app coming forward, may put a full-screen app in front.
        for name in [NSWorkspace.activeSpaceDidChangeNotification, NSWorkspace.didActivateApplicationNotification] {
            NSWorkspace.shared.notificationCenter.publisher(for: name)
                .sink { [weak self] _ in self?.spaceMayHaveChanged() }
                .store(in: &cancellables)
        }
        model.$data.map { [$0.settings.autoHideBar, $0.settings.hidesInFullScreen, $0.settings.keepsWindowsClear] }.removeDuplicates()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.pointerMoved(); self?.keepWindowsClear() } }
            .store(in: &cancellables)
        // Windows are kept clear of the bar's final size, not every step of a divider drag.
        tracker.$resizing.removeDuplicates().dropFirst()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.keepWindowsClear() } }
            .store(in: &cancellables)
        // A context menu holds the bar out while it is open.
        for (name, open) in [(NSMenu.didBeginTrackingNotification, true), (NSMenu.didEndTrackingNotification, false)] {
            NotificationCenter.default.publisher(for: name)
                .sink { [weak self] _ in self?.menuOpen = open; self?.pointerMoved() }
                .store(in: &cancellables)
        }
        tracker.$dropTargeted.removeDuplicates().dropFirst()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.pointerMoved() } }
            .store(in: &cancellables)

        let events: NSEvent.EventTypeMask = [.mouseMoved, .leftMouseDragged, .rightMouseDragged]
        if let m = NSEvent.addGlobalMonitorForEvents(matching: events, handler: { [weak self] event in
            MainActor.assumeIsolated { self?.pointerMoved(event) }
        }) { monitors.append(m) }
        if let m = NSEvent.addLocalMonitorForEvents(matching: events, handler: { [weak self] event in
            MainActor.assumeIsolated { self?.pointerMoved(event) }
            return event
        }) { monitors.append(m) }
        layout()
        refreshFullScreen()
    }

    /// The menu bar's screen. NSScreen.main follows the key window, so the bar would jump
    /// between displays.
    private var screen: NSScreen? { NSScreen.screens.first }

    /// The menu bar's height on the bar's screen, from the last Space that showed it.
    private var menuBar = 0.0

    /// Puts the panel where the settings, its size and the screen say, with only the window
    /// calls that change something. A Space change alone changes nothing (DockPlacement).
    private func layout() {
        var target: CGRect?
        if model.data.settings.dockVisible, let screen {
            let f = tracker.frame
            let top = screen.frame.maxY - screen.visibleFrame.maxY
            if top > 0 { menuBar = top }
            target = DockPlacement.frame(edge: f.edge, size: f.size, barMid: f.lead + f.geometry.restLength / 2,
                                         screen: screen.frame, visible: screen.visibleFrame, menuBar: menuBar)
        }
        for step in DockPlacement.steps(from: panel.isVisible ? panel.frame : nil, to: target) {
            switch step {
            case let .setFrame(frame): panel.setFrame(frame, display: true)
            case .orderFront: panel.orderFrontRegardless()
            case .orderOut: panel.orderOut(nil)
            }
        }
        pointerMoved()
        keepWindowsClear()
    }

    /// While the bar is pinned open on a desktop, windows are kept from sitting under it
    /// (WindowKeeper). Never from a scratch copy, which must leave the owner's windows alone.
    private func keepWindowsClear() {
        guard !tracker.resizing else { return }
        guard !DataStore.isScratch, model.data.settings.keepsWindowsClear, rule == .pinned, panel.isVisible,
              let screen, let primary = NSScreen.screens.first?.frame else { keeper.update(nil); return }
        // Accessibility measures from the top of the menu bar's screen, downward.
        func flipped(_ r: CGRect) -> CGRect { CGRect(x: r.minX, y: primary.maxY - r.maxY, width: r.width, height: r.height) }
        let f = tracker.frame
        keeper.update(.init(strip: WindowClearance.strip(edge: f.edge, panel: flipped(panel.frame), lead: f.lead,
                                                         length: f.geometry.restLength, depth: DockFrame.inset + f.geometry.thickness),
                            edge: f.edge, area: flipped(screen.visibleFrame)))
    }

    /// The window list can trail a Space change, so look again shortly after.
    private func spaceMayHaveChanged() {
        refreshFullScreen()
        keepWindowsClear() // Accessibility may have been allowed since
        for delay in [0.3, 1.0] {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in self?.refreshFullScreen() }
        }
    }

    private func refreshFullScreen() {
        guard let screen else { return }
        let now = Self.fullScreen(on: screen)
        guard now != fullScreen else { return }
        fullScreen = now
        pointerMoved()
        keepWindowsClear()
    }

    /// From the window list's bounds, which need no permission, and the menu bar (FullScreenSpace).
    private static func fullScreen(on screen: NSScreen) -> Bool {
        guard let id = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber,
              let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]
        else { return false }
        let windows = list.compactMap { info -> FullScreenSpace.Window? in
            guard let layer = info[kCGWindowLayer as String] as? Int,
                  let dict = info[kCGWindowBounds as String] as? NSDictionary,
                  let bounds = CGRect(dictionaryRepresentation: dict) else { return nil }
            return FullScreenSpace.Window(layer: layer, bounds: bounds, alpha: info[kCGWindowAlpha as String] as? Double ?? 1)
        }
        return FullScreenSpace.detect(windows: windows, display: CGDisplayBounds(id.uint32Value), topInset: screen.safeAreaInsets.top,
                                      visibleReachesTop: screen.visibleFrame.maxY >= screen.frame.maxY - 0.5)
    }

    /// How far a pointer event moved toward the bar's screen edge.
    private func push(_ event: NSEvent) -> Double {
        switch tracker.frame.edge {
        case .left: return -event.deltaX
        case .right: return event.deltaX
        case .bottom: return event.deltaY
        }
    }

    private func pointerMoved(_ event: NSEvent? = nil) {
        guard panel.isVisible, let screen else { return }
        let now = Date()
        let mouse = NSEvent.mouseLocation, fr = panel.frame
        let (axis, cross) = tracker.frame.bar(CGPoint(x: mouse.x - fr.minX, y: fr.maxY - mouse.y))
        let g = tracker.frame.geometry
        let onBar = g.engages(axis: axis, cross: cross, inset: DockFrame.inset, engaged: tracker.pointer != nil)
        let fromEdge: Double
        switch tracker.frame.edge {
        case .left: fromEdge = mouse.x - screen.frame.minX
        case .right: fromEdge = screen.frame.maxX - mouse.x
        case .bottom: fromEdge = mouse.y - screen.frame.minY
        }
        visibility.update(.init(atEdge: g.touchesEdge(axis: axis, fromEdge: fromEdge), onBar: onBar,
                                held: menuOpen || tracker.resizing || tracker.dropTargeted,
                                pushed: event.map(push) ?? 0 > 0),
                          rule: rule, now: now)
        let reveal = visibility.visible(at: now)
        if reveal != tracker.reveal { tracker.reveal = reveal }
        schedule(visibility)
        if tracker.resizing {
            panel.ignoresMouseEvents = false
            return
        }
        let on = onBar && visibility.isOut
        if panel.ignoresMouseEvents == on { panel.ignoresMouseEvents = !on }
        let next = on ? axis : nil
        guard next != tracker.pointer else { return }
        if tracker.pointer == nil, next != nil, tracker.frame.geometry.slots.contains(.folder(.trash)) {
            let full = Folders.trashIsFull
            if full != tracker.trashFull { tracker.trashFull = full }
        }
        // Ease in and out of magnification; follow the pointer exactly while on the bar.
        if (next == nil) != (tracker.pointer == nil), !tracker.frame.motion.reduced {
            withAnimation(.easeOut(duration: 0.16)) { tracker.pointer = next }
        } else {
            tracker.pointer = next
        }
    }

    /// Redraws every frame while the bar slides, and otherwise wakes for the machine's next
    /// timed decision, so a pointer resting on the edge still reveals it.
    private func schedule(_ hide: DockAutoHide) {
        if hide.isSliding {
            if frameTimer == nil {
                frameTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated { self?.pointerMoved() }
                }
            }
        } else {
            frameTimer?.invalidate(); frameTimer = nil
        }
        decisionTimer?.invalidate(); decisionTimer = nil
        if let next = hide.nextDecision(), !hide.isSliding {
            decisionTimer = Timer.scheduledTimer(withTimeInterval: max(0.01, next.timeIntervalSinceNow), repeats: false) { [weak self] _ in
                MainActor.assumeIsolated { self?.pointerMoved() }
            }
        }
    }
}

/// What Kairos Bar is drawn on. Liquid Glass does not render in the snapshot tool, so it draws
/// the material, or a light stand-in for glass in the light appearance.
enum DockSurface { case glass, material, lightGlassStandIn }

private struct DockSurfaceKey: EnvironmentKey { static let defaultValue = DockSurface.glass }

extension EnvironmentValues {
    var dockSurface: DockSurface {
        get { self[DockSurfaceKey.self] }
        set { self[DockSurfaceKey.self] = newValue }
    }
}

struct DockView: View {
    @EnvironmentObject var model: AppModel
    @ObservedObject var tracker: DockTracker
    @Environment(\.dockSurface) private var requestedSurface
    @Environment(\.colorScheme) private var scheme
    @State private var resizeFrom: (mouse: NSPoint, size: Double)?
    @State private var cursorPushed = false

    var body: some View {
        let f = tracker.frame, g = f.geometry
        let layout = g.layout(pointer: tracker.resizing ? nil : tracker.pointer)
        ZStack(alignment: .topLeading) {
            background(f, layout)
            ForEach(layout.keyed, id: \.key) { entry in
                slot(entry.item, f)
            }
            if let h = layout.hovered { label(for: layout.items[h], f) }
        }
        .frame(width: f.size.width, height: f.size.height, alignment: .topLeading)
        .offset(slide(f))
        .opacity(f.motion.appearance(reveal: tracker.reveal).opacity)
        .onDrop(of: [.fileURL], delegate: DockDropTarget(model: model, tracker: tracker))
        .onReceive(model.$runningApps) { running in
            var launches = tracker.launches
            launches.update(running: running, now: Date())
            if launches != tracker.launches { tracker.launches = launches }
        }
    }

    /// Opens an app from the bar. One that is not running yet pulses until it is.
    private func launch(_ bundleId: String) {
        if !model.runningApps.contains(bundleId) {
            tracker.launches.start(bundleId, now: Date())
            DispatchQueue.main.asyncAfter(deadline: .now() + DockLaunches.timeout) { [tracker] in
                tracker.launches.update(running: AppModel.shared.runningApps, now: Date())
            }
        }
        Apps.open(bundleId)
    }

    /// Auto-hide slides the bar past its screen edge, where the panel clips it. With Reduce
    /// motion it fades in place instead.
    private func slide(_ f: DockFrame) -> CGSize {
        let away = f.motion.appearance(reveal: tracker.reveal).away * (DockFrame.inset + f.geometry.thickness + 4)
        switch f.edge {
        case .left: return CGSize(width: -away, height: 0)
        case .right: return CGSize(width: away, height: 0)
        case .bottom: return CGSize(width: 0, height: away)
        }
    }

    // MARK: Pieces

    private var surface: DockSurface {
        if case .glass = requestedSurface, #unavailable(macOS 26.0) { return .material }
        return requestedSurface
    }

    /// Marks on the bar (dots, separators, the + tiles) in the colour that stands out on it. The
    /// material is always dark; Liquid Glass follows the system appearance, so on a light
    /// wallpaper in the light appearance the marks turn dark. Each mark also has a thin edge
    /// in the other colour, for glass that ends up darker or lighter than its appearance.
    private var inkIsLight: Bool {
        switch surface {
        case .material: return true
        case .lightGlassStandIn: return false
        case .glass: return scheme == .dark
        }
    }
    private var ink: Color { inkIsLight ? .white : .black }
    private var halo: Color { inkIsLight ? .black : .white }

    private func background(_ f: DockFrame, _ layout: DockGeometry.Layout) -> some View {
        let g = f.geometry
        let shape = RoundedRectangle(cornerRadius: min(g.thickness * 0.3, 18), style: .continuous)
        let size = f.size(along: layout.end - layout.start, across: g.thickness)
        return Group {
            if #available(macOS 26.0, *), surface == .glass {
                Color.clear.glassEffect(.regular, in: shape)
            } else if surface == .lightGlassStandIn {
                shape.fill(.white.opacity(0.62))
                    .overlay(shape.strokeBorder(.black.opacity(tracker.dropTargeted ? 0.5 : 0.1)))
            } else {
                shape.fill(.ultraThinMaterial)
                    .background(Color.black.opacity(0.42), in: shape)
                    .overlay(shape.strokeBorder(.white.opacity(tracker.dropTargeted ? 0.6 : 0.16)))
                    .environment(\.colorScheme, .dark)
            }
        }
        .frame(width: size.width, height: size.height)
        .contentShape(shape)
        .contextMenu { barMenu }
        .position(f.point(axis: (layout.start + layout.end) / 2, cross: DockFrame.inset + g.thickness / 2))
    }

    @ViewBuilder
    private func slot(_ item: DockGeometry.Placement, _ f: DockFrame) -> some View {
        let g = f.geometry
        // Icons grow away from the screen edge, like the Dock's.
        let cross = DockFrame.inset + g.padding + item.size / 2
        let at = f.point(axis: item.center, cross: cross)
        switch item.slot {
        case let .group(id):
            if let index = model.data.groups.firstIndex(where: { $0.id == id }) {
                groupButton(model.data.groups[index], size: item.size)
                    .frame(width: item.size, height: item.size)
                    .contentShape(Rectangle())
                    .onTapGesture { model.switchGroup(id) }
                    .contextMenu { groupMenu(model.data.groups[index]) }
                    .accessibilityLabel("Switch to \(model.data.groups[index].name)")
                    .accessibilityAddTraits(.isButton)
                    .overlay(alignment: .topTrailing) {
                        if id == model.data.focus.activeGroupId, model.siteBlockingOff { warningBadge(size: item.size) }
                    }
                    .position(at)
                if id == model.data.focus.activeGroupId { activeMark(item, f) }
            }
        case .newGroup:
            placeholder("plus", size: item.size)
                .frame(width: item.size, height: item.size)
                .contentShape(Rectangle())
                .onTapGesture { DockActions.newGroup(model) }
                .contextMenu { barMenu }
                .accessibilityLabel("New group")
                .accessibilityAddTraits(.isButton)
                .position(at)
        case .divider:
            divider(f)
                .position(f.point(axis: item.center, cross: DockFrame.inset + g.thickness / 2))
        case let .app(bundleId):
            Image(nsImage: Apps.icon(bundleId)).resizable().interpolation(.high)
                .frame(width: item.size, height: item.size)
                .modifier(LaunchPulse(active: tracker.launches.ids.contains(bundleId)))
                .contentShape(Rectangle())
                .onTapGesture { launch(bundleId) }
                .contextMenu { appMenu(bundleId) }
                .accessibilityLabel(Apps.name(bundleId))
                .accessibilityAddTraits(.isButton)
                .position(at)
            if model.runningApps.contains(bundleId) { dot(item, f) }
        case .separator:
            rule(f)
                .allowsHitTesting(false)
                .position(f.point(axis: item.center, cross: DockFrame.inset + g.thickness / 2))
        case let .running(bundleId):
            Image(nsImage: Apps.icon(bundleId)).resizable().interpolation(.high)
                .frame(width: item.size, height: item.size)
                .contentShape(Rectangle())
                .onTapGesture { launch(bundleId) }
                .contextMenu {
                    Button("Keep in \(model.activeGroup?.name ?? "Group")") { model.addDockApp(bundleId) }
                    blockButton(bundleId)
                    Divider()
                    Button("Show") { Apps.open(bundleId) }
                    Button("Hide") { Apps.running(bundleId)?.hide() }
                    Button("Quit") { Apps.running(bundleId)?.terminate() }
                }
                .accessibilityLabel(Apps.name(bundleId))
                .accessibilityAddTraits(.isButton)
                .position(at)
            dot(item, f)
        case let .folder(folder):
            Image(nsImage: Folders.icon(folder, trashFull: tracker.trashFull)).resizable().interpolation(.high)
                .frame(width: item.size, height: item.size)
                .contentShape(Rectangle())
                .onTapGesture { Folders.open(folder) }
                .contextMenu {
                    Button("Open in Finder") { Folders.open(folder) }
                    Divider()
                    barMenu
                }
                .accessibilityLabel(folder.name)
                .accessibilityHint(folder == .trash ? "Drop files here to move them to the Trash" : "Opens in Finder")
                .accessibilityAddTraits(.isButton)
                .position(at)
        case .addApp:
            placeholder("square.and.arrow.down", size: item.size)
                .frame(width: item.size, height: item.size)
                .contentShape(Rectangle())
                .onTapGesture { DockActions.addApps(model) }
                .contextMenu { barMenu }
                .accessibilityLabel("Add apps to \(model.activeGroup?.name ?? "this group")")
                .accessibilityAddTraits(.isButton)
                .position(at)
        }
    }

    private func groupButton(_ group: AppGroup, size: Double) -> some View {
        let active = group.id == model.data.focus.activeGroupId
        let color = Color(hex: group.colorHex)
        let shape = RoundedRectangle(cornerRadius: size * 0.225, style: .continuous)
        // Opaque even when inactive, so a magnified tile never shows the bar's edge through it.
        return shape
            .fill(active ? color : color.opacity(0.24))
            .background(Color(white: inkIsLight ? 0.16 : 0.96), in: shape)
            .overlay(shape.strokeBorder(color.opacity(active ? 0 : 0.55), lineWidth: 1))
            .overlay {
                Image(systemName: group.icon)
                    .font(.system(size: size * 0.42, weight: .semibold))
                    .foregroundStyle(active ? .white : color)
            }
            .padding(size * 0.07)
    }

    /// On the active group while blocking is on but no Kairos Shield is connected.
    private func warningBadge(size: Double) -> some View {
        Image(systemName: "exclamationmark")
            .font(.system(size: max(7, size * 0.2), weight: .heavy))
            .foregroundStyle(.white)
            .frame(width: max(11, size * 0.32), height: max(11, size * 0.32))
            .background(Color.orange, in: Circle())
            .overlay(Circle().strokeBorder(Color.black.opacity(0.25)))
            .accessibilityLabel("Site blocking is off")
            .allowsHitTesting(false)
    }

    private func placeholder(_ symbol: String, size: Double) -> some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.2, style: .continuous)
        return shape
            .strokeBorder(ink.opacity(0.55), style: StrokeStyle(lineWidth: 1.2, dash: [3, 3]))
            .shadow(color: halo.opacity(0.35), radius: 0.5)
            .overlay {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.34, weight: .semibold))
                    .foregroundStyle(ink.opacity(0.85))
                    .shadow(color: halo.opacity(0.35), radius: 0.5)
            }
            .padding(size * 0.14)
    }

    /// The running dot sits between the icon and the screen edge.
    private func dot(_ item: DockGeometry.Placement, _ f: DockFrame) -> some View {
        let d = f.geometry.icon < 30 ? 3.0 : 4.0
        return Circle().fill(ink.opacity(0.9)).frame(width: d, height: d)
            .overlay(Circle().strokeBorder(halo.opacity(0.35), lineWidth: 0.5))
            .allowsHitTesting(false)
            .position(f.point(axis: item.center, cross: DockFrame.inset + f.geometry.padding / 2))
    }

    /// The active group gets a short bar where an app gets its running dot, so the two never
    /// look alike.
    private func activeMark(_ item: DockGeometry.Placement, _ f: DockFrame) -> some View {
        let d = f.geometry.icon < 30 ? 3.0 : 4.0
        let size = f.size(along: max(10, f.geometry.icon * 0.36), across: d)
        return Capsule().fill(ink.opacity(0.9))
            .overlay(Capsule().strokeBorder(halo.opacity(0.35), lineWidth: 0.5))
            .frame(width: size.width, height: size.height)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            .position(f.point(axis: item.center, cross: DockFrame.inset + f.geometry.padding / 2))
    }

    /// The line of the divider and the separator: one line in the ink beside a fainter one in
    /// the other colour, so it shows on a light or a dark bar.
    private func rule(_ f: DockFrame) -> some View {
        let line = f.size(along: 1, across: f.geometry.thickness * 0.6)
        let pair = f.edge == .bottom
            ? AnyView(HStack(spacing: 0) { Rectangle().fill(ink.opacity(0.4)); Rectangle().fill(halo.opacity(0.18)) })
            : AnyView(VStack(spacing: 0) { Rectangle().fill(ink.opacity(0.4)); Rectangle().fill(halo.opacity(0.18)) })
        return pair.frame(width: f.edge == .bottom ? 2 : line.width, height: f.edge == .bottom ? line.height : 2)
    }

    private func divider(_ f: DockFrame) -> some View {
        let g = f.geometry
        let hit = f.size(along: DockGeometry.dividerLength, across: g.thickness)
        return rule(f)
            .frame(width: hit.width, height: hit.height)
            .contentShape(Rectangle())
            .onHover { inside in
                let cursor: NSCursor = f.edge == .bottom ? .resizeUpDown : .resizeLeftRight
                if inside, !cursorPushed { cursor.push(); cursorPushed = true }
                if !inside, cursorPushed { NSCursor.pop(); cursorPushed = false }
            }
            .onDisappear { if cursorPushed { NSCursor.pop(); cursorPushed = false } }
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { _ in resize(edge: f.edge) }
                .onEnded { _ in
                    resizeFrom = nil
                    tracker.resizing = false
                })
            .contextMenu { barMenu }
            .help("Drag to resize")
            .accessibilityLabel("Resize Kairos Bar")
    }

    /// Screen coordinates, because the panel moves and resizes under the pointer as it drags.
    private func resize(edge: DockEdge) {
        let mouse = NSEvent.mouseLocation
        let from = resizeFrom ?? (mouse, model.data.settings.iconSize)
        resizeFrom = from
        tracker.resizing = true
        let pull: Double
        switch edge {
        case .left: pull = mouse.x - from.mouse.x
        case .right: pull = from.mouse.x - mouse.x
        case .bottom: pull = mouse.y - from.mouse.y
        }
        let next = (min(64, max(20, from.size + pull * 0.75)) / 2).rounded() * 2
        if next != model.data.settings.iconSize { model.data.settings.iconSize = next }
    }

    private func label(for item: DockGeometry.Placement, _ f: DockFrame) -> some View {
        let (title, detail) = names(item.slot)
        let g = f.geometry
        let lane = DockFrame.inset + g.padding + item.size + 10
        let pill = HStack(spacing: 8) {
            Text(title).lineLimit(1).truncationMode(.tail)
            if let detail { Text(detail).foregroundStyle(.white.opacity(0.55)).fixedSize() }
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(.white)
        .padding(.horizontal, 11)
        .frame(height: 26)
        .background(Color(white: 0.12).opacity(0.86), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(.white.opacity(0.14)))
        .shadow(color: .black.opacity(0.25), radius: 4, y: 1)
        .allowsHitTesting(false)
        // A box the label sizes itself inside, pinned to the side away from the screen edge.
        let room: CGSize, alignment: Alignment, center: CGPoint
        switch f.edge {
        case .left, .right:
            let width = f.crossLength - lane - 4
            room = CGSize(width: width, height: 30)
            alignment = f.edge == .left ? .leading : .trailing
            center = f.point(axis: item.center, cross: lane + width / 2)
        case .bottom:
            // Centred over the icon, as wide as the panel allows on both sides.
            let x = f.lead + item.center
            room = CGSize(width: max(40, min(320, 2 * min(x, f.axisLength - x) - 4)), height: 30)
            alignment = .center
            center = CGPoint(x: x, y: f.crossLength - lane - 15)
        }
        return pill.frame(width: room.width, height: room.height, alignment: alignment).position(center)
    }

    private func names(_ slot: DockGeometry.Slot) -> (String, String?) {
        switch slot {
        case let .group(id):
            let index = model.data.groups.firstIndex { $0.id == id } ?? 0
            let shortcut = model.data.settings.groupHotkeys && index < 9 ? "⌃\(index + 1)" : nil
            if id == model.data.focus.activeGroupId, model.siteBlockingOff { return (model.data.group(id)?.name ?? "", "Site blocking off") }
            return (model.data.group(id)?.name ?? "", shortcut)
        case .newGroup: return ("New Group…", nil)
        case .divider, .separator: return ("", nil)
        case let .app(bundleId), let .running(bundleId): return (Apps.name(bundleId), nil)
        case let .folder(folder): return (folder.name, nil)
        case .addApp: return ("Add Apps to \(model.activeGroup?.name ?? "Group")…", nil)
        }
    }

    // MARK: Menus

    @ViewBuilder
    private var barMenu: some View {
        Button("New Group…") { DockActions.newGroup(model) }
        Button("Add Apps to \(model.activeGroup?.name ?? "Group")…") { DockActions.addApps(model) }
        Button("Edit Groups…") { DockActions.editGroups(model, nil) }
        Divider()
        Toggle("Automatically Hide and Show", isOn: $model.data.settings.autoHideBar)
        Toggle("Hide in Full-Screen Apps", isOn: model.hidesInFullScreenBinding)
            .disabled(model.data.settings.autoHideBar)
        Picker("Position on Screen", selection: $model.data.settings.dockEdge) {
            ForEach(DockEdge.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
        }
        Button("Hide Kairos Bar") { model.data.settings.dockVisible = false }
    }

    @ViewBuilder
    private func groupMenu(_ group: AppGroup) -> some View {
        if group.id != model.data.focus.activeGroupId {
            Button("Switch to \(group.name)") { model.switchGroup(group.id) }
        }
        Button("Rename…") { DockActions.rename(model, group.id) }
        Button("Edit \(group.name)…") { DockActions.editGroups(model, group.id) }
        Button("Delete \(group.name)…") { DockActions.delete(model, group.id) }
            .disabled(model.data.groups.count < 2)
        Divider()
        Button("New Group…") { DockActions.newGroup(model) }
    }

    @ViewBuilder
    private func appMenu(_ bundleId: String) -> some View {
        Button("Open") { launch(bundleId) }
        if model.runningApps.contains(bundleId) {
            Button("Hide") { Apps.running(bundleId)?.hide() }
            Button("Quit") { Apps.running(bundleId)?.terminate() }
        }
        Divider()
        let apps = model.activeGroup?.dockApps ?? []
        let across = tracker.frame.edge == .bottom
        Button(across ? "Move Left" : "Move Up") { model.moveDockApp(bundleId, by: -1) }
            .disabled(apps.first == bundleId)
        Button(across ? "Move Right" : "Move Down") { model.moveDockApp(bundleId, by: 1) }
            .disabled(apps.last == bundleId)
        Divider()
        blockButton(bundleId)
        Button("Remove from \(model.activeGroup?.name ?? "group")") { model.removeDockApp(bundleId) }
    }

    /// Blocks the app in the active group, or unblocks it (not while Strict is locked).
    @ViewBuilder
    private func blockButton(_ bundleId: String) -> some View {
        let group = model.activeGroup?.name ?? "group"
        if model.isAppBlocked(bundleId) {
            Button("Unblock in \(group)") { model.setAppBlocked(bundleId, false) }.disabled(model.strictLocked)
        } else {
            Button("Block in \(group)") { model.setAppBlocked(bundleId, true) }
        }
    }
}

/// Dims and brightens an app's icon while it starts, in place of the Dock's bounce, so it
/// moves nothing even with Reduce motion on.
private struct LaunchPulse: ViewModifier {
    let active: Bool
    @State private var dim = false

    func body(content: Content) -> some View {
        content
            .opacity(dim ? 0.4 : 1)
            .onChange(of: active, initial: true) { _, on in
                if on {
                    withAnimation(.easeInOut(duration: 0.55).repeatForever(autoreverses: true)) { dim = true }
                } else {
                    var still = Transaction()
                    still.disablesAnimations = true
                    withTransaction(still) { dim = false }
                }
            }
    }
}

/// Files dropped on Kairos Bar: on Trash they go to the Trash, on an app the documents open in
/// it, and apps dropped anywhere else join the active group (DockDrop).
private struct DockDropTarget: DropDelegate {
    let model: AppModel
    let tracker: DockTracker

    func dropEntered(info: DropInfo) { tracker.dropTargeted = true }
    func dropExited(info: DropInfo) { tracker.dropTargeted = false }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: slot(at: info.location) == .folder(.trash) ? .move : .copy)
    }

    func performDrop(info: DropInfo) -> Bool {
        tracker.dropTargeted = false
        let slot = slot(at: info.location)
        let box = URLBox()
        let group = DispatchGroup()
        for provider in info.itemProviders(for: [.fileURL]) {
            group.enter()
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                if let url { box.append(url) }
                group.leave()
            }
        }
        group.notify(queue: .main) { [model, tracker] in
            MainActor.assumeIsolated {
                Self.perform(DockDrop.plan(box.urls, on: slot, bundleId: { Bundle(url: $0)?.bundleIdentifier }), model: model, tracker: tracker)
            }
        }
        return true
    }

    private func slot(at point: CGPoint) -> DockGeometry.Slot? {
        tracker.frame.geometry.slot(at: tracker.frame.bar(point).axis)
    }

    @MainActor
    private static func perform(_ drop: DockDrop, model: AppModel, tracker: DockTracker) {
        switch drop {
        case let .addApps(ids):
            ids.forEach { model.addDockApp($0) }
        case let .open(urls, app):
            guard let appURL = Apps.running(app)?.bundleURL ?? Apps.url(app) else { return }
            NSWorkspace.shared.open(urls, withApplicationAt: appURL, configuration: NSWorkspace.OpenConfiguration())
        case let .moveToTrash(urls):
            NSWorkspace.shared.recycle(urls) { _, _ in
                DispatchQueue.main.async { tracker.trashFull = Folders.trashIsFull }
            }
        case .nothing:
            break
        }
    }
}

private final class URLBox: @unchecked Sendable {
    private let lock = NSLock()
    private var list: [URL] = []
    func append(_ url: URL) { lock.lock(); list.append(url); lock.unlock() }
    var urls: [URL] { lock.lock(); defer { lock.unlock() }; return list }
}

/// Group and app management started from the bar. Each runs after the menu closes, and
/// brings Kairos forward because the bar's panel never activates the app.
@MainActor
enum DockActions {
    static func newGroup(_ model: AppModel) {
        later {
            guard let name = ask("New Group", info: "It starts empty. Add apps from Kairos Bar, or drag them onto it.",
                                 value: "", button: "Create") else { return }
            model.switchGroup(model.addGroup(name: name))
        }
    }

    static func rename(_ model: AppModel, _ id: UUID) {
        later {
            guard let group = model.data.group(id),
                  let name = ask("Rename \u{201C}\(group.name)\u{201D}", info: nil, value: group.name, button: "Rename") else { return }
            model.renameGroup(id, to: name)
        }
    }

    static func delete(_ model: AppModel, _ id: UUID) {
        later {
            guard let group = model.data.group(id), model.data.groups.count > 1 else { return }
            NSApp.activate(ignoringOtherApps: true)
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = "Delete \u{201C}\(group.name)\u{201D}?"
            alert.informativeText = "Its apps and blocking rules are removed. Its tasks are kept, with no group."
            alert.addButton(withTitle: "Delete").hasDestructiveAction = true
            alert.addButton(withTitle: "Cancel")
            if alert.runModal() == .alertFirstButtonReturn { model.deleteGroup(id) }
        }
    }

    static func addApps(_ model: AppModel) {
        later {
            NSApp.activate(ignoringOtherApps: true)
            let panel = NSOpenPanel()
            panel.title = "Add Apps to \(model.activeGroup?.name ?? "Group")"
            panel.prompt = "Add"
            panel.allowedContentTypes = [.application]
            panel.allowsMultipleSelection = true
            panel.directoryURL = URL(fileURLWithPath: "/Applications")
            guard panel.runModal() == .OK else { return }
            for url in panel.urls {
                if let id = Bundle(url: url)?.bundleIdentifier { model.addDockApp(id) }
            }
        }
    }

    static func editGroups(_ model: AppModel, _ id: UUID?) {
        model.selectedGroupId = id ?? model.data.focus.activeGroupId
        model.page = .groups
        AppDelegate.shared?.showMainWindow()
    }

    private static func later(_ work: @escaping @MainActor () -> Void) {
        DispatchQueue.main.async { MainActor.assumeIsolated { work() } }
    }

    private static func ask(_ title: String, info: String?, value: String, button: String) -> String? {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = title
        if let info { alert.informativeText = info }
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        field.stringValue = value
        field.placeholderString = "Group name"
        alert.accessoryView = field
        alert.addButton(withTitle: button)
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty ? nil : name
    }
}
