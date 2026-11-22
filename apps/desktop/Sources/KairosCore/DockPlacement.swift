import CoreGraphics
import Foundation

/// Where Kairos Bar's panel sits on its screen, and the fewest window calls that put it there.
/// The panel is on every Space, so a Space change needs no call at all: re-setting its frame or
/// ordering it front again as a swipe lands can make it blink, which the Dock never does.
public enum DockPlacement {
    /// The panel's frame for a panel of `size` whose bar, at rest, is centred `barMid` along it.
    /// `screen` is the whole display and `visible` the part macOS leaves free of its Dock and
    /// menu bar. When `replacesDock` is true, Kairos is hiding the system Dock so the bar sits
    /// against `screen`'s edge even if macOS still reserves a Dock strip in `visible`. The top
    /// of `visible` is replaced by `menuBar` below the screen's top, because the menu bar hides
    /// in a full-screen app's Space and a side bar must not move with it.
    public static func frame(edge: DockEdge, size: CGSize, barMid: Double,
                             screen: CGRect, visible: CGRect, menuBar: Double,
                             replacesDock: Bool = false) -> CGRect {
        let area = replacesDock ? screen : visible
        let top = screen.maxY - menuBar
        let midY = (area.minY + top) / 2
        var origin: CGPoint
        switch edge {
        case .left: origin = CGPoint(x: area.minX, y: midY + barMid - size.height)
        case .right: origin = CGPoint(x: area.maxX - size.width, y: midY + barMid - size.height)
        case .bottom: origin = CGPoint(x: area.midX - barMid, y: area.minY)
        }
        if edge == .bottom {
            origin.x = min(max(origin.x, screen.minX), screen.maxX - size.width)
        } else {
            origin.y = min(max(origin.y, screen.minY), screen.maxY - size.height)
        }
        return CGRect(origin: origin, size: size)
    }

    public enum Step: Equatable, Sendable {
        case setFrame(CGRect), orderFront, orderOut
    }

    /// The calls that take the panel from `current` (its frame while on screen, nil while not)
    /// to `target` (nil to take it off screen). Nothing when it is already there.
    public static func steps(from current: CGRect?, to target: CGRect?) -> [Step] {
        switch (current, target) {
        case (nil, nil): return []
        case (.some, nil): return [.orderOut]
        case let (nil, .some(t)): return [.setFrame(t), .orderFront]
        case let (.some(c), .some(t)): return same(c, t) ? [] : [.setFrame(t)]
        }
    }

    /// Within half a point: the window server keeps a panel's frame on whole points.
    private static func same(_ a: CGRect, _ b: CGRect) -> Bool {
        abs(a.minX - b.minX) < 0.5 && abs(a.minY - b.minY) < 0.5 && abs(a.width - b.width) < 0.5 && abs(a.height - b.height) < 0.5
    }
}
