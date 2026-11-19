import Foundation

/// Geometry of Kairos Bar. Positions are measured along the bar (the axis, 0 at the leading end
/// of the bar at rest) and across it (from the screen edge inward). Pure, so magnification and
/// the pointer's hit zone are testable without a screen.
public struct DockGeometry: Equatable, Sendable {
    public enum Slot: Hashable, Sendable {
        /// `running` is an open app that is not pinned to the active group; `separator` sets
        /// those apart, as the Dock does for recent apps, and sets the folders apart at the end.
        case group(UUID), newGroup, divider, app(String), addApp, separator, running(String), folder(DockFolder)
        public var magnifies: Bool { self != .divider && self != .separator }
    }

    public struct Placement: Equatable, Sendable {
        public let slot: Slot
        /// Centre along the axis.
        public let center: Double
        /// Extent along the axis. Icons are square, so also their extent across the bar.
        public let size: Double
        public let scale: Double

        public init(slot: Slot, center: Double, size: Double, scale: Double) {
            (self.slot, self.center, self.size, self.scale) = (slot, center, size, scale)
        }
    }

    public struct Layout: Equatable, Sendable {
        public let items: [Placement]
        /// Where the bar's background starts and ends along the axis.
        public let start: Double, end: Double
        /// The magnifying slot under the pointer, which gets the name label.
        public let hovered: Int?
    }

    public let slots: [Slot]
    public let icon: Double
    public let maxScale: Double

    public init(slots: [Slot], icon: Double, maxScale: Double = DockMotion.magnification) {
        self.slots = slots
        self.icon = icon
        self.maxScale = maxScale
    }

    /// `running` is the open apps in launch order. Those not pinned to the active group follow
    /// the pinned ones after a separator when group isolation is off.
    /// Downloads and Trash come last, after another separator, unless turned off in Settings.
    public static func slots(for data: AppData, running: [String] = []) -> [Slot] {
        let apps = data.activeGroup?.dockApps ?? []
        var seen = Set(apps)
        let isolatesGroup = data.activeGroup != nil && data.settings.switchBehavior == .hide
        let extra = isolatesGroup ? [] : running.filter { seen.insert($0).inserted }
        let folders: [Slot] = data.settings.showsDockFolders ? [.separator] + DockFolder.allCases.map { .folder($0) } : []
        return data.groups.map { .group($0.id) } + [.newGroup, .divider] + (apps.isEmpty ? [.addApp] : apps.map { .app($0) })
            + (extra.isEmpty ? [] : [.separator] + extra.map { .running($0) }) + folders
    }

    /// The slot a pointer at `axis` is over, as the bar lays itself out under that pointer.
    public func slot(at axis: Double) -> Slot? {
        let l = layout(pointer: axis)
        return l.hovered.map { l.items[$0].slot }
    }

    public static let dividerLength = 11.0
    public var gap: Double { max(3, (icon * 0.14).rounded()) }
    /// The Dock keeps about 7 pt between its icons and its edges at small sizes, growing with
    /// larger icons: 24 pt icons make a bar 38 pt thick, as the owner's Dock is 1.6 times its icons.
    public var padding: Double { max(7, (icon * 0.2).rounded()) }
    /// How far from the pointer, along the axis, an icon still grows.
    public var range: Double { icon * 2.5 }
    public var thickness: Double { icon + 2 * padding }
    /// The Dock's ends are rounded to about a third of its thickness.
    public var cornerRadius: Double { (thickness / 3).rounded() }
    /// How far a magnified icon reaches past the bar, away from the screen edge.
    public var headroom: Double { (maxScale - 1) * icon }

    public func baseLength(_ slot: Slot) -> Double { slot.magnifies ? icon : Self.dividerLength }

    public var restLength: Double {
        2 * padding + slots.reduce(0) { $0 + baseLength($1) } + gap * Double(max(0, slots.count - 1))
    }

    /// The Dock's falloff: full size under the pointer, easing on a cosine to rest size at `range`.
    public func scale(distance: Double) -> Double {
        let d = min(abs(distance), range)
        return 1 + (maxScale - 1) * (cos(Double.pi * d / range) + 1) / 2
    }

    /// Lays the slots out for a pointer at `pointer` along the axis, or at rest for nil. `amount`
    /// (0 at rest, 1 fully magnified) eases magnification in and out around the pointer.
    ///
    /// The bar is stretched like a lens: every point of it, icons, gaps and the spaces the
    /// divider and separators take alike, is spread apart by the falloff around the pointer,
    /// and the point under the pointer stays exactly under it. An icon is as long as its
    /// stretched span. So nothing overlaps, and the bar's ends follow the pointer smoothly and
    /// hold still while the falloff is clear of them. Growing only the icons instead makes the
    /// bar rock once per icon, and shrink whenever the pointer crosses a slot that does not
    /// grow, which reads as a wobble on a fast sweep.
    public func layout(pointer: Double?, amount: Double = 1) -> Layout {
        let rest = spans(scales: Array(repeating: 1, count: slots.count))
        guard let p = pointer, !slots.isEmpty else {
            return Layout(items: place(rest, scales: Array(repeating: 1, count: slots.count), shift: 0), start: 0, end: restLength, hovered: nil)
        }
        let k = stretch * amount
        // Where a point at `x` on the bar at rest lands; the pointer's own point stays put.
        func at(_ x: Double) -> Double { x + k * (bulge(x - p) - range / 2) }
        let items = slots.indices.map { i in
            let lo = at(rest[i].lo), hi = at(rest[i].hi)
            let size = slots[i].magnifies ? hi - lo : baseLength(slots[i])
            return Placement(slot: slots[i], center: (lo + hi) / 2, size: size, scale: size / baseLength(slots[i]))
        }
        let half = gap / 2
        let hovered = items.indices.first { i in
            items[i].slot.magnifies && abs(p - items[i].center) <= items[i].size / 2 + half
        }
        return Layout(items: items, start: at(0), end: at(restLength), hovered: hovered)
    }

    /// The falloff's shape, 1 under the pointer easing to 0 at `range`, summed from `-range`
    /// up to `u`: how much extra length the bar gains between there and a point `u` from the
    /// pointer, per unit of stretch.
    private func bulge(_ u: Double) -> Double {
        guard range > 0 else { return 0 }
        let v = min(max(u, -range), range)
        return (v + range + range / Double.pi * sin(Double.pi * v / range)) / 2
    }

    /// How hard the bar is stretched, set so the icon centred under the pointer comes out at
    /// `maxScale` times its size: its span averages the falloff over the icon's length.
    private var stretch: Double {
        guard range > 0 else { return 0 }
        let a = Double.pi * icon / 2 / range
        return (maxScale - 1) / ((1 + sin(a) / a) / 2)
    }

    /// Tells items apart for drawing. Only separators repeat on the bar, so each is keyed by its
    /// slot and how many of the same slot came before it.
    public struct ItemKey: Hashable, Sendable {
        public let slot: Slot
        public let occurrence: Int
    }

    /// How far the bar can reach past its rest extent at each end, over every pointer position.
    public var overflow: (lead: Double, trail: Double) {
        var lead = 0.0, trail = 0.0
        var p = -range
        while p <= restLength + range {
            let l = layout(pointer: p)
            lead = max(lead, -l.start)
            trail = max(trail, l.end - restLength)
            p += 1
        }
        return (lead.rounded(.up), trail.rounded(.up))
    }

    /// Whether a pointer at (axis, cross) is on the bar. It takes the bar at rest to engage, and
    /// once engaged the zone grows to cover the magnified icons, so the edge of the zone does not
    /// flicker as icons grow and shrink. `cross` counts from the screen edge, and `inset` is the
    /// gap between the screen edge and the bar.
    public func engages(axis: Double, cross: Double, inset: Double, engaged: Bool) -> Bool {
        guard cross >= -1 else { return false }
        if !engaged {
            return cross <= inset + thickness && axis >= 0 && axis <= restLength
        }
        let l = layout(pointer: axis)
        return cross <= inset + thickness + headroom && axis >= l.start && axis <= l.end
    }

    private struct Span { var lo: Double, hi: Double }

    private func spans(scales: [Double]) -> [Span] {
        var x = padding
        return slots.indices.map { i in
            let length = baseLength(slots[i]) * scales[i]
            defer { x += length + gap }
            return Span(lo: x, hi: x + length)
        }
    }

    private func place(_ spans: [Span], scales: [Double], shift: Double) -> [Placement] {
        slots.indices.map { i in
            Placement(slot: slots[i], center: (spans[i].lo + spans[i].hi) / 2 + shift, size: spans[i].hi - spans[i].lo, scale: scales[i])
        }
    }
}

/// How Kairos Bar moves, from the macOS "Reduce motion" setting. With it on, icons keep their
/// size under the pointer and auto-hide fades the bar out and in instead of sliding it.
public struct DockMotion: Equatable, Sendable {
    public let reduced: Bool
    public init(reduced: Bool) { self.reduced = reduced }

    public static let magnification = 1.6
    public var maxScale: Double { reduced ? 1 : Self.magnification }

    /// For `reveal` (0 hidden, 1 shown): the share of the slide still to go past the screen
    /// edge, and the bar's opacity.
    public func appearance(reveal: Double) -> (away: Double, opacity: Double) {
        reduced ? (0, reveal) : (1 - reveal, 1)
    }
}

extension DockGeometry.Layout {
    /// The items with keys that stay unique when a slot repeats.
    public var keyed: [(key: DockGeometry.ItemKey, item: DockGeometry.Placement)] {
        var seen: [DockGeometry.Slot: Int] = [:]
        return items.map { item in
            let n = seen[item.slot, default: 0]
            seen[item.slot] = n + 1
            return (DockGeometry.ItemKey(slot: item.slot, occurrence: n), item)
        }
    }
}
