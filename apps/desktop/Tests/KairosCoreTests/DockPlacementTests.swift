import CoreGraphics
import KairosCore
import XCTest

/// The owner's display: 1470 x 956 with a 34 pt menu bar, the real Dock hidden.
final class DockPlacementTests: XCTestCase {
    let screen = CGRect(x: 0, y: 0, width: 1470, height: 956)
    let desktop = CGRect(x: 0, y: 0, width: 1470, height: 922)
    /// A full-screen app's Space: the menu bar is hidden, so the visible frame reaches the top.
    let fullScreenSpace = CGRect(x: 0, y: 0, width: 1470, height: 956)
    let size = CGSize(width: 594, height: 94.4)

    private func frame(_ edge: DockEdge, visible: CGRect, size: CGSize? = nil) -> CGRect {
        let s = size ?? (edge == .bottom ? self.size : CGSize(width: self.size.height, height: self.size.width))
        return DockPlacement.frame(edge: edge, size: s, barMid: 280, screen: screen, visible: visible, menuBar: 34)
    }

    func testASpaceChangeNeedsNoWindowCalls() {
        for edge in DockEdge.allCases {
            let shown = frame(edge, visible: desktop)
            XCTAssertEqual(DockPlacement.steps(from: shown, to: frame(edge, visible: desktop)), [],
                           "\(edge): swiping to another desktop neither moves nor re-orders the panel")
        }
    }

    func testTheMenuBarHidingInAFullScreenSpaceMovesNothing() {
        for edge in DockEdge.allCases {
            XCTAssertEqual(frame(edge, visible: fullScreenSpace), frame(edge, visible: desktop),
                           "\(edge): the bar stays put when a Space without the menu bar comes forward")
            XCTAssertEqual(DockPlacement.steps(from: frame(edge, visible: desktop), to: frame(edge, visible: fullScreenSpace)), [])
        }
    }

    func testTheWindowServerRoundingTheFrameIsNotAChange() {
        let target = frame(.bottom, visible: desktop)
        let rounded = CGRect(x: target.minX.rounded(), y: target.minY.rounded(), width: target.width.rounded(), height: target.height.rounded())
        XCTAssertEqual(DockPlacement.steps(from: rounded, to: target), [])
    }

    func testANewSizeMovesThePanelWithoutReorderingIt() {
        let before = frame(.bottom, visible: desktop)
        let after = frame(.bottom, visible: desktop, size: CGSize(width: 640, height: 104))
        XCTAssertEqual(DockPlacement.steps(from: before, to: after), [.setFrame(after)])
    }

    func testHidingAndShowingTheBar() {
        let shown = frame(.bottom, visible: desktop)
        XCTAssertEqual(DockPlacement.steps(from: shown, to: nil), [.orderOut])
        XCTAssertEqual(DockPlacement.steps(from: nil, to: nil), [])
        XCTAssertEqual(DockPlacement.steps(from: nil, to: shown), [.setFrame(shown), .orderFront])
    }

    func testTheBarAtRestIsCentredOnItsEdge() {
        let bottom = frame(.bottom, visible: desktop)
        XCTAssertEqual(bottom.minX + 280, desktop.midX, "the bar's middle, not the panel's, is centred")
        XCTAssertEqual(bottom.minY, 0)
        let left = frame(.left, visible: desktop)
        XCTAssertEqual(left.minX, 0)
        XCTAssertEqual(left.maxY - 280, (0 + 956 - 34) / 2, "centred between the screen's foot and the menu bar")
        let right = frame(.right, visible: desktop)
        XCTAssertEqual(right.maxX, 1470)
    }

    func testAWideBarStaysOnScreen() {
        let wide = DockPlacement.frame(edge: .bottom, size: CGSize(width: 1500, height: 94), barMid: 100,
                                       screen: screen, visible: desktop, menuBar: 34)
        XCTAssertEqual(wide.maxX, screen.maxX)
    }

    func testTheRealDocksReservedStripIsRespected() {
        // A Dock that is not hidden keeps its strip; the bar sits beside it on the same edge.
        let withDock = CGRect(x: 0, y: 70, width: 1470, height: 852)
        XCTAssertEqual(frame(.bottom, visible: withDock).minY, 70)
    }
}
