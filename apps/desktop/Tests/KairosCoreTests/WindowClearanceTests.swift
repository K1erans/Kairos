import CoreGraphics
import KairosCore
import XCTest

/// The owner's display in Accessibility coordinates (top-left origin): 1470 x 956, a 34 pt menu
/// bar, Kairos Bar pinned at the bottom with 24 pt icons (36 pt deep with its gap to the edge).
final class WindowClearanceTests: XCTestCase {
    let area = CGRect(x: 0, y: 34, width: 1470, height: 922)
    let panel = CGRect(x: 438, y: 861, width: 594, height: 95)
    lazy var bottom = WindowClearance.strip(edge: .bottom, panel: panel, lead: 6, length: 560, depth: 36)

    func testTheStripRunsAlongTheBarFromTheScreenEdge() {
        XCTAssertEqual(bottom, CGRect(x: 444, y: 920, width: 560, height: 36))
        let side = CGRect(x: 0, y: 200, width: 280, height: 600)
        XCTAssertEqual(WindowClearance.strip(edge: .left, panel: side, lead: 10, length: 500, depth: 36),
                       CGRect(x: 0, y: 210, width: 36, height: 500))
        XCTAssertEqual(WindowClearance.strip(edge: .right, panel: side.offsetBy(dx: 1190, dy: 0), lead: 10, length: 500, depth: 36),
                       CGRect(x: 1434, y: 210, width: 36, height: 500))
    }

    func testAZoomedWindowIsShortenedToEndAboveTheBar() {
        let zoomed = WindowClearance.Window(frame: CGRect(x: 0, y: 34, width: 1470, height: 922))
        XCTAssertEqual(WindowClearance.clear(zoomed, of: bottom, edge: .bottom, area: area),
                       CGRect(x: 0, y: 34, width: 1470, height: 886))
    }

    func testAWindowThatFitsMovesUpKeepingItsSize() {
        let low = WindowClearance.Window(frame: CGRect(x: 300, y: 500, width: 800, height: 450))
        XCTAssertEqual(WindowClearance.clear(low, of: bottom, edge: .bottom, area: area),
                       CGRect(x: 300, y: 470, width: 800, height: 450))
    }

    func testAWindowBesideTheBarIsLeftAlone() {
        let corner = WindowClearance.Window(frame: CGRect(x: 0, y: 600, width: 400, height: 356))
        XCTAssertNil(WindowClearance.clear(corner, of: bottom, edge: .bottom, area: area), "it does not reach over the bar")
        let above = WindowClearance.Window(frame: CGRect(x: 300, y: 100, width: 800, height: 820))
        XCTAssertNil(WindowClearance.clear(above, of: bottom, edge: .bottom, area: area), "ending at the bar's edge is clear")
    }

    func testFullScreenMinimisedAndNonStandardWindowsAreLeftAlone() {
        let frame = CGRect(x: 0, y: 34, width: 1470, height: 922)
        for window in [WindowClearance.Window(frame: frame, isFullScreen: true),
                       WindowClearance.Window(frame: frame, isMinimized: true),
                       WindowClearance.Window(frame: frame, isStandard: false)] {
            XCTAssertNil(WindowClearance.clear(window, of: bottom, edge: .bottom, area: area))
        }
    }

    func testAFixedSizeWindowMovesIfItFitsAndIsLeftIfNot() {
        let fits = WindowClearance.Window(frame: CGRect(x: 300, y: 600, width: 500, height: 340), canResize: false)
        XCTAssertEqual(WindowClearance.clear(fits, of: bottom, edge: .bottom, area: area)?.size, fits.frame.size)
        let tooTall = WindowClearance.Window(frame: CGRect(x: 300, y: 34, width: 500, height: 922), canResize: false)
        XCTAssertNil(WindowClearance.clear(tooTall, of: bottom, edge: .bottom, area: area))
    }

    func testAWindowThatCannotMoveIsOnlyShrunk() {
        let pinned = WindowClearance.Window(frame: CGRect(x: 300, y: 500, width: 800, height: 450), canMove: false)
        XCTAssertNil(WindowClearance.clear(pinned, of: bottom, edge: .bottom, area: area),
                     "it would fit by moving, which it can't, and shrinking from the top would move it too")
        let tall = WindowClearance.Window(frame: CGRect(x: 0, y: 34, width: 1470, height: 922), canMove: false)
        XCTAssertEqual(WindowClearance.clear(tall, of: bottom, edge: .bottom, area: area)?.height, 886, "shrinking its foot keeps its origin")
    }

    func testSideBarsPushWindowsSideways() {
        let left = WindowClearance.strip(edge: .left, panel: CGRect(x: 0, y: 200, width: 280, height: 600), lead: 10, length: 500, depth: 36)
        let wide = WindowClearance.Window(frame: CGRect(x: 0, y: 34, width: 1470, height: 922))
        XCTAssertEqual(WindowClearance.clear(wide, of: left, edge: .left, area: area), CGRect(x: 36, y: 34, width: 1434, height: 922))
        let small = WindowClearance.Window(frame: CGRect(x: 10, y: 300, width: 600, height: 400))
        XCTAssertEqual(WindowClearance.clear(small, of: left, edge: .left, area: area), CGRect(x: 36, y: 300, width: 600, height: 400))

        let right = WindowClearance.strip(edge: .right, panel: CGRect(x: 1190, y: 200, width: 280, height: 600), lead: 10, length: 500, depth: 36)
        XCTAssertEqual(WindowClearance.clear(wide, of: right, edge: .right, area: area), CGRect(x: 0, y: 34, width: 1434, height: 922))
        let nearRight = WindowClearance.Window(frame: CGRect(x: 1000, y: 300, width: 460, height: 400))
        XCTAssertEqual(WindowClearance.clear(nearRight, of: right, edge: .right, area: area), CGRect(x: 974, y: 300, width: 460, height: 400))
    }

    func testAClearedWindowIsLeftAloneNextTime() {
        let zoomed = WindowClearance.Window(frame: CGRect(x: 0, y: 34, width: 1470, height: 922))
        let once = WindowClearance.clear(zoomed, of: bottom, edge: .bottom, area: area)!
        XCTAssertNil(WindowClearance.clear(WindowClearance.Window(frame: once), of: bottom, edge: .bottom, area: area),
                     "the move it causes does not set off another")
    }

    func testASliverIsNotWorthMaking() {
        let short = WindowClearance.Window(frame: CGRect(x: 300, y: 860, width: 60, height: 96), canMove: false)
        XCTAssertNil(WindowClearance.clear(short, of: bottom, edge: .bottom, area: area))
    }

    func testKeepingWindowsClearIsOnForNewAndExistingProfiles() throws {
        XCTAssertTrue(Settings().keepsWindowsClear)
        let saved = try DataStore.decoder.decode(Settings.self, from: Data(#"{"autoHideBar": false, "dockEdge": "bottom"}"#.utf8))
        XCTAssertTrue(saved.keepsWindowsClear, "a profile saved before the setting existed gets it on")
    }

    func testTheChecklistSaysAccessibilityKeepsWindowsClear() {
        let item = SetupChecklist(SetupFacts()).items.first { $0.step == .accessibility }
        XCTAssertTrue(item?.detail.contains("Kairos Bar") == true)
    }
}
