import CoreGraphics
import Foundation

/// Whether a full-screen app has the Space on a display, from public signals that need no
/// Accessibility permission: the window list's bounds and whether macOS shows its menu bar.
public enum FullScreenSpace {
    /// One entry of the on-screen window list, front to back, in the window list's coordinates
    /// (top-left origin, y down).
    public struct Window: Equatable, Sendable {
        public let layer: Int
        public let bounds: CGRect
        public let alpha: Double
        public init(layer: Int, bounds: CGRect, alpha: Double = 1) {
            self.layer = layer; self.bounds = bounds; self.alpha = alpha
        }
    }

    /// The menu bar's own window, which a full-screen app's Space hides.
    public static let menuBarLayer = 24

    /// `display` is the display's bounds in the same coordinates. `topInset` is the strip a
    /// camera housing takes at its top (zero without one): full-screen apps stop below it. A
    /// window there only counts while the menu bar is hidden, because a zoomed window with the
    /// Dock hidden reaches the same spot on a desktop. The menu bar counts as hidden when its
    /// window is not on screen or `visibleReachesTop` (NSScreen's visible frame) says so.
    public static func detect(windows: [Window], display: CGRect, topInset: Double, visibleReachesTop: Bool) -> Bool {
        // The frontmost normal window on this display. Tiny helper windows don't count.
        guard let front = windows.first(where: {
            $0.layer == 0 && $0.alpha > 0 && $0.bounds.width >= 100 && $0.bounds.height >= 100
                && $0.bounds.intersection(display).width >= 100
        })?.bounds else { return false }
        let reachesBottom = front.maxY >= display.maxY - 1
        let reachesTop = front.minY <= display.minY + 1
        let belowHousing = topInset > 0 && front.minY <= display.minY + topInset + 1
        return reachesBottom && (reachesTop || (belowHousing && (visibleReachesTop || !menuBarShown(windows, on: display))))
    }

    private static func menuBarShown(_ windows: [Window], on display: CGRect) -> Bool {
        windows.contains {
            $0.layer == menuBarLayer && $0.bounds.minY <= display.minY + 1 && $0.bounds.maxY > display.minY + 1
                && $0.bounds.intersection(display).width >= display.width / 2
        }
    }
}
