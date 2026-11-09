import CoreGraphics
import Foundation

/// Keeps other apps' windows from sitting under Kairos Bar while it is pinned open. macOS keeps
/// windows clear only of its own Dock, so Kairos moves or shrinks a window that overlaps the
/// bar's strip until it ends at the bar's inner edge. Everything here is in the Accessibility
/// API's coordinates: global, top-left origin, y down.
public enum WindowClearance {
    /// The strip the bar takes at rest: from the screen edge to the bar's inner edge, along the
    /// bar's length. `panel` is the bar's panel; the bar starts `lead` along it, runs `length`,
    /// and reaches `depth` in from the edge.
    public static func strip(edge: DockEdge, panel: CGRect, lead: Double, length: Double, depth: Double) -> CGRect {
        switch edge {
        case .bottom: return CGRect(x: panel.minX + lead, y: panel.maxY - depth, width: length, height: depth)
        case .left: return CGRect(x: panel.minX, y: panel.minY + lead, width: depth, height: length)
        case .right: return CGRect(x: panel.maxX - depth, y: panel.minY + lead, width: depth, height: length)
        }
    }

    public struct Window: Equatable, Sendable {
        public var frame: CGRect
        public var canMove: Bool
        public var canResize: Bool
        /// A standard window: not a sheet, dialog, panel or menu.
        public var isStandard: Bool
        public var isFullScreen: Bool
        public var isMinimized: Bool
        public init(frame: CGRect, canMove: Bool = true, canResize: Bool = true, isStandard: Bool = true,
                    isFullScreen: Bool = false, isMinimized: Bool = false) {
            self.frame = frame; self.canMove = canMove; self.canResize = canResize
            self.isStandard = isStandard; self.isFullScreen = isFullScreen; self.isMinimized = isMinimized
        }
    }

    /// The frame `window` should have so it ends at the bar's inner edge, or nil to leave it.
    /// `area` is the screen without the menu bar, which the window must stay inside. A window
    /// that fits beside the bar keeps its size and moves; one that does not is shrunk from the
    /// side facing the bar. One that can do neither is left alone.
    public static func clear(_ window: Window, of strip: CGRect, edge: DockEdge, area: CGRect) -> CGRect? {
        let f = window.frame
        guard window.isStandard, !window.isFullScreen, !window.isMinimized,
              f.width > 0, f.height > 0, f.intersects(strip) else { return nil }
        var target = f
        switch edge {
        case .bottom:
            let limit = strip.minY
            if f.height <= limit - area.minY {
                target.origin.y = min(f.minY, limit - f.height)
            } else {
                target.origin.y = max(f.minY, area.minY)
                target.size.height = limit - target.minY
            }
        case .left:
            let limit = strip.maxX
            if f.width <= area.maxX - limit {
                target.origin.x = max(f.minX, limit)
            } else {
                target.origin.x = limit
                target.size.width = min(f.maxX, area.maxX) - limit
            }
        case .right:
            let limit = strip.minX
            if f.width <= limit - area.minX {
                target.origin.x = min(f.minX, limit - f.width)
            } else {
                target.origin.x = max(f.minX, area.minX)
                target.size.width = limit - target.minX
            }
        }
        if target.origin != f.origin, !window.canMove { return nil }
        if target.size != f.size, !window.canResize { return nil }
        // A sliver is no use to anyone; leave it for the owner to place.
        guard target.width >= 120, target.height >= 80 else { return nil }
        return target == f ? nil : target
    }
}
