import KairosCore
import XCTest

/// Label widths are those of real labels at the label font (13 pt medium, 11 pt padding):
/// "Work ⌃1" 83, "Trash" 59, "Visual Studio Code - Insiders" 204, a 30-character group name
/// with ⌃1 241, and with "Site blocking off" 325.
final class DockLabelTests: XCTestCase {
    let work = 83.0, trash = 59.0, vsCode = 204.0, longGroup = 241.0, longGroupBlocking = 325.0

    /// The pill's extent along the bar.
    private func pill(_ center: Double, _ width: Double) -> ClosedRange<Double> { (center - width / 2)...(center + width / 2) }

    private func assertInside(_ p: ClosedRange<Double>, _ bounds: ClosedRange<Double>, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertGreaterThanOrEqual(p.lowerBound, bounds.lowerBound + DockLabel.margin - 1e-9, "clipped at the start", file: file, line: line)
        XCTAssertLessThanOrEqual(p.upperBound, bounds.upperBound - DockLabel.margin + 1e-9, "clipped at the end", file: file, line: line)
    }

    // MARK: Bottom edge (labels run along the bar)

    func testTheFirstSlotsLabelSlidesInsideThePanel() {
        let bounds = 0.0...594
        for width in [work, vsCode, longGroup, longGroupBlocking] {
            let c = DockLabel.center(item: 25, width: width, within: bounds)
            assertInside(pill(c, width), bounds)
            XCTAssertTrue(pill(c, width).contains(25), "still over the item")
        }
        XCTAssertEqual(DockLabel.center(item: 25, width: work, within: bounds), 4 + work / 2)
    }

    func testTheLastSlotsLabelSlidesInsideThePanel() {
        let bounds = 0.0...594
        for width in [trash, vsCode, longGroupBlocking] {
            let c = DockLabel.center(item: 575, width: width, within: bounds)
            assertInside(pill(c, width), bounds)
            XCTAssertTrue(pill(c, width).contains(575))
        }
    }

    func testAMiddleLabelIsCentredOverItsItem() {
        XCTAssertEqual(DockLabel.center(item: 300, width: work, within: 0...594), 300)
        XCTAssertEqual(DockLabel.center(item: 300, width: longGroup, within: 0...594), 300)
    }

    func testALabelStaysOnTheVisibleScreen() {
        // The panel starts 60 pt left of the visible screen: labels keep to the visible part.
        let visible = 60.0...594
        let c = DockLabel.center(item: 80, width: vsCode, within: visible)
        assertInside(pill(c, vsCode), visible)
    }

    func testAShortBarGrowsItsPanelForTheWidestLabel() {
        let grow = DockLabel.growth(length: 150, widest: longGroup)
        XCTAssertEqual(grow, 50)
        let bounds = 0.0...(150 + 2 * grow)
        let c = DockLabel.center(item: grow + 20, width: longGroup, within: bounds)
        assertInside(pill(c, longGroup), bounds)
        XCTAssertEqual(DockLabel.growth(length: 594, widest: longGroupBlocking), 0, "a long bar needs no more room")
    }

    func testTheOwnersBarHoveringItsFirstGroup() {
        // Two groups, +, divider, four apps, a separator, Downloads and Trash, 24 pt icons.
        let g = DockGeometry(slots: [.group(UUID()), .group(UUID()), .newGroup, .divider, .app("a"), .app("b"), .app("c"), .app("d"),
                                     .separator, .folder(.downloads), .folder(.trash)], icon: 24)
        let (lead0, trail0) = g.overflow
        let grow = DockLabel.growth(length: lead0 + g.restLength + trail0, widest: longGroupBlocking)
        let lead = lead0 + grow, length = lead0 + g.restLength + trail0 + 2 * grow
        let first = g.layout(pointer: nil).items[0].center
        let hovered = g.layout(pointer: first)
        let item = hovered.items[hovered.hovered!]
        XCTAssertGreaterThanOrEqual(lead + item.center - item.size / 2, 0, "the magnified first icon is inside the panel")
        XCTAssertGreaterThanOrEqual(lead + hovered.start, 0, "and so is the bar under it")
        for width in [work, longGroupBlocking] {
            let c = DockLabel.center(item: lead + item.center, width: width, within: 0...length)
            assertInside(pill(c, width), 0...length)
        }
        let last = g.layout(pointer: nil).items.last!.center
        let atEnd = g.layout(pointer: last)
        let trashItem = atEnd.items[atEnd.hovered!]
        XCTAssertLessThanOrEqual(lead + trashItem.center + trashItem.size / 2, length, "the magnified Trash is inside the panel")
    }

    // MARK: Side edges (labels sit beside the bar; only their height runs along it)

    func testSideLabelsAtEitherEndStayInsideThePanel() {
        let bounds = 0.0...640
        for item in [12.0, 320, 632] {
            let c = DockLabel.center(item: item, width: 26, within: bounds)
            assertInside(pill(c, 26), bounds)
        }
        XCTAssertEqual(DockLabel.center(item: 320, width: 26, within: bounds), 320)
    }

    func testTheSideLaneFitsTheWidestLabel() {
        XCTAssertEqual(DockLabel.sideLane(widest: longGroupBlocking), 339)
        XCTAssertGreaterThanOrEqual(DockLabel.sideLane(widest: vsCode), vsCode + DockLabel.gap)
        XCTAssertEqual(DockLabel.sideLane(widest: work), 97, "short names keep a narrow lane")
    }

    func testALabelWiderThanItsRoomIsCentredOnIt() {
        XCTAssertEqual(DockLabel.center(item: 10, width: 300, within: 0...200), 100)
    }
}
