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
    let lead: Double, trail: Double
    /// Gap between the screen edge and the bar.
    static let inset = 4.0

    init(data: AppData, running: [String] = []) {
        self.init(Inputs(data: data, running: running))
    }

    init(_ inputs: Inputs) {
        geometry = DockGeometry(slots: inputs.slots, icon: inputs.icon)
        edge = inputs.edge
        (lead, trail) = geometry.overflow
    }

    /// What the frame depends on, compared before building one, because `overflow` samples
    /// every pointer position.
    struct Inputs: Equatable {
        let slots: [DockGeometry.Slot], icon: Double, edge: DockEdge
        init(data: AppData, running: [String]) {
            slots = DockGeometry.slots(for: data, running: running)
            icon = data.settings.iconSize
            edge = data.settings.dockEdge
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
    /// Nil while auto-hide is off.
    private var autoHide: DockAutoHide?
    private var menuOpen = false
    private var frameTimer: Timer?
    private var decisionTimer: Timer?

    init(model: AppModel) {
        self.model = model
        tracker = DockTracker(frame: DockFrame(data: model.data, running: model.openApps))
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
        model.$data.combineLatest(model.$openApps)
            .map { DockFrame.Inputs(data: $0, running: $1) }
            .removeDuplicates()
            .dropFirst()
            .map(DockFrame.init)
            .sink { [weak self] frame in self?.tracker.frame = frame; self?.layout() }
            .store(in: &cancellables)
        model.$data.map(\.settings.dockVisible).removeDuplicates()
            .sink { [weak self] _ in DispatchQueue.main.async { self?.layout() } }
            .store(in: &cancellables)
        NotificationCenter.default.publisher(for: NSApplication.didChangeScreenParametersNotification)
            .sink { [weak self] _ in self?.layout() }
            .store(in: &cancellables)
        model.$data.map(\.settings.autoHideBar).removeDuplicates()
            .sink { [weak self] on in
                self?.autoHide = on ? DockAutoHide(now: Date()) : nil
                DispatchQueue.main.async { self?.pointerMoved() }
            }
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
        if let m = NSEvent.addGlobalMonitorForEvents(matching: events, handler: { [weak self] _ in
            MainActor.assumeIsolated { self?.pointerMoved() }
        }) { monitors.append(m) }
        if let m = NSEvent.addLocalMonitorForEvents(matching: events, handler: { [weak self] event in
            MainActor.assumeIsolated { self?.pointerMoved() }
            return event
        }) { monitors.append(m) }
        layout()
    }

    /// The menu bar's screen. NSScreen.main follows the key window, so the bar would jump
    /// between displays.
    private var screen: NSScreen? { NSScreen.screens.first }

    private func layout() {
        guard model.data.settings.dockVisible, let screen else {
            panel.orderOut(nil)
            return
        }
        let f = tracker.frame
        let size = f.size
        let area = screen.visibleFrame
        // Centre the bar at rest, not the panel: the room at each end can differ.
        let barMid = f.lead + f.geometry.restLength / 2
        var origin: NSPoint
        switch f.edge {
        case .left: origin = NSPoint(x: area.minX, y: area.midY + barMid - size.height)
        case .right: origin = NSPoint(x: area.maxX - size.width, y: area.midY + barMid - size.height)
        case .bottom: origin = NSPoint(x: area.midX - barMid, y: area.minY)
        }
        if f.edge == .bottom {
            origin.x = min(max(origin.x, screen.frame.minX), screen.frame.maxX - size.width)
        } else {
            origin.y = min(max(origin.y, screen.frame.minY), screen.frame.maxY - size.height)
        }
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
        panel.orderFrontRegardless()
        pointerMoved()
    }

    private func pointerMoved() {
        guard panel.isVisible, let screen else { return }
        let now = Date()
        let mouse = NSEvent.mouseLocation, fr = panel.frame
        let (axis, cross) = tracker.frame.bar(CGPoint(x: mouse.x - fr.minX, y: fr.maxY - mouse.y))
        let g = tracker.frame.geometry
        let onBar = g.engages(axis: axis, cross: cross, inset: DockFrame.inset, engaged: tracker.pointer != nil)
        if var hide = autoHide {
            let fromEdge: Double
            switch tracker.frame.edge {
            case .left: fromEdge = mouse.x - screen.frame.minX
            case .right: fromEdge = screen.frame.maxX - mouse.x
            case .bottom: fromEdge = mouse.y - screen.frame.minY
            }
            hide.update(.init(atEdge: g.touchesEdge(axis: axis, fromEdge: fromEdge), onBar: onBar,
                              held: menuOpen || tracker.resizing || tracker.dropTargeted), now: now)
            autoHide = hide
            let reveal = hide.visible(at: now)
            if reveal != tracker.reveal { tracker.reveal = reveal }
            schedule(hide)
        } else {
            frameTimer?.invalidate(); frameTimer = nil
            decisionTimer?.invalidate(); decisionTimer = nil
            if tracker.reveal != 1 { tracker.reveal = 1 }
        }
        if tracker.resizing {
            panel.ignoresMouseEvents = false
            return
        }
        let on = onBar && autoHide?.isOut ?? true
        if panel.ignoresMouseEvents == on { panel.ignoresMouseEvents = !on }
        let next = on ? axis : nil
        guard next != tracker.pointer else { return }
        // Ease in and out of magnification; follow the pointer exactly while on the bar.
        if (next == nil) != (tracker.pointer == nil) {
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

private struct DockGlassKey: EnvironmentKey { static let defaultValue = true }

extension EnvironmentValues {
    /// Off in the snapshot tool, where Liquid Glass does not render.
    var dockGlass: Bool {
        get { self[DockGlassKey.self] }
        set { self[DockGlassKey.self] = newValue }
    }
}

struct DockView: View {
    @EnvironmentObject var model: AppModel
    @ObservedObject var tracker: DockTracker
    @Environment(\.dockGlass) private var glass
    @State private var resizeFrom: (mouse: NSPoint, size: Double)?
    @State private var cursorPushed = false

    var body: some View {
        let f = tracker.frame, g = f.geometry
        let layout = g.layout(pointer: tracker.resizing ? nil : tracker.pointer)
        ZStack(alignment: .topLeading) {
            background(f, layout)
            ForEach(layout.items, id: \.slot) { item in
                slot(item, f)
            }
            if let h = layout.hovered { label(for: layout.items[h], f) }
        }
        .frame(width: f.size.width, height: f.size.height, alignment: .topLeading)
        .offset(slide(f))
        .onDrop(of: [.fileURL], isTargeted: $tracker.dropTargeted) { providers in
            for provider in providers {
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    guard let url, url.pathExtension == "app", let id = Bundle(url: url)?.bundleIdentifier else { return }
                    Task { @MainActor in model.addDockApp(id) }
                }
            }
            return true
        }
    }

    /// Auto-hide slides the bar past its screen edge, where the panel clips it.
    private func slide(_ f: DockFrame) -> CGSize {
        let away = (1 - tracker.reveal) * (DockFrame.inset + f.geometry.thickness + 4)
        switch f.edge {
        case .left: return CGSize(width: -away, height: 0)
        case .right: return CGSize(width: away, height: 0)
        case .bottom: return CGSize(width: 0, height: away)
        }
    }

    // MARK: Pieces

    private func background(_ f: DockFrame, _ layout: DockGeometry.Layout) -> some View {
        let g = f.geometry
        let shape = RoundedRectangle(cornerRadius: min(g.thickness * 0.3, 18), style: .continuous)
        let size = f.size(along: layout.end - layout.start, across: g.thickness)
        return Group {
            if #available(macOS 26.0, *), glass {
                Color.clear.glassEffect(.regular, in: shape)
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
                if id == model.data.focus.activeGroupId { dot(item, f) }
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
                .contentShape(Rectangle())
                .onTapGesture { Apps.open(bundleId) }
                .contextMenu { appMenu(bundleId) }
                .accessibilityLabel(Apps.name(bundleId))
                .accessibilityAddTraits(.isButton)
                .position(at)
            if model.runningApps.contains(bundleId) { dot(item, f) }
        case .separator:
            let line = f.size(along: 1, across: g.thickness * 0.6)
            Rectangle().fill(.white.opacity(0.32))
                .frame(width: line.width, height: line.height)
                .allowsHitTesting(false)
                .position(f.point(axis: item.center, cross: DockFrame.inset + g.thickness / 2))
        case let .running(bundleId):
            Image(nsImage: Apps.icon(bundleId)).resizable().interpolation(.high)
                .frame(width: item.size, height: item.size)
                .contentShape(Rectangle())
                .onTapGesture { Apps.open(bundleId) }
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
            .background(Color(white: 0.16), in: shape)
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
            .strokeBorder(.white.opacity(0.5), style: StrokeStyle(lineWidth: 1.2, dash: [3, 3]))
            .overlay {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.34, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.8))
            }
            .padding(size * 0.14)
    }

    /// The running dot sits between the icon and the screen edge.
    private func dot(_ item: DockGeometry.Placement, _ f: DockFrame) -> some View {
        let d = f.geometry.icon < 30 ? 3.0 : 4.0
        return Circle().fill(.white.opacity(0.85)).frame(width: d, height: d)
            .shadow(color: .black.opacity(0.4), radius: 1)
            .allowsHitTesting(false)
            .position(f.point(axis: item.center, cross: DockFrame.inset + f.geometry.padding / 2))
    }

    private func divider(_ f: DockFrame) -> some View {
        let g = f.geometry
        let line = f.size(along: 1, across: g.thickness * 0.6)
        let hit = f.size(along: DockGeometry.dividerLength, across: g.thickness)
        return Rectangle().fill(.white.opacity(0.32))
            .frame(width: line.width, height: line.height)
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
        Button("Open") { Apps.open(bundleId) }
        if model.runningApps.contains(bundleId) {
            Button("Hide") { Apps.running(bundleId)?.hide() }
            Button("Quit") { Apps.running(bundleId)?.terminate() }
        }
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

/// Group and app management started from the bar. Each runs after the menu closes, and
/// brings Kairos forward because the bar's panel never activates the app.
@MainActor
enum DockActions {
    static func newGroup(_ model: AppModel) {
        later {
            guard let name = ask("New Group", info: "It starts empty. Add apps from the bar, or drag them onto it.",
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
