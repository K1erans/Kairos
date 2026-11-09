import XCTest
@testable import KairosCore

final class ReduceMotionTests: XCTestCase {
    let slots: [DockGeometry.Slot] = [.app("a"), .app("b"), .app("c"), .app("d")]

    func testWithReduceMotionIconsKeepTheirSizeUnderThePointer() {
        let motion = DockMotion(reduced: true)
        let g = DockGeometry(slots: slots, icon: 36, maxScale: motion.maxScale)
        let under = g.layout(pointer: g.restLength / 2).items
        XCTAssertEqual(under.map(\.size), [36, 36, 36, 36])
        XCTAssertEqual(g.headroom, 0)
    }

    func testWithoutItTheIconUnderThePointerGrows() {
        let g = DockGeometry(slots: slots, icon: 36, maxScale: DockMotion(reduced: false).maxScale)
        let first = g.layout(pointer: nil).items[0]
        XCTAssertGreaterThan(g.layout(pointer: first.center).items[0].size, 36)
    }

    func testAutoHideFadesInPlaceWithReduceMotionAndSlidesWithout() {
        XCTAssertEqual(DockMotion(reduced: true).appearance(reveal: 0.25).away, 0)
        XCTAssertEqual(DockMotion(reduced: true).appearance(reveal: 0.25).opacity, 0.25)
        XCTAssertEqual(DockMotion(reduced: false).appearance(reveal: 0.25).away, 0.75)
        XCTAssertEqual(DockMotion(reduced: false).appearance(reveal: 0.25).opacity, 1)
    }
}

final class DueTextTests: XCTestCase {
    var calendar: Calendar = {
        var c = Calendar(identifier: .gregorian)
        c.timeZone = TimeZone(identifier: "UTC")!
        return c
    }()
    let locale = Locale(identifier: "en_GB")
    /// Monday 9 November 2026, 10:00.
    lazy var now = at(2026, 11, 9, 10)

    private func at(_ y: Int, _ mo: Int, _ d: Int, _ h: Int, _ mi: Int = 0) -> Date {
        calendar.date(from: DateComponents(year: y, month: mo, day: d, hour: h, minute: mi))!
    }
    private func text(_ due: Date) -> String { let now = self.now; return DueText.short(due, now: now, calendar: calendar, locale: locale) }

    func testTheReferenceIsMondayMorning() {
        let now = self.now
        XCTAssertEqual(now, at(2026, 11, 9, 10))
        XCTAssertEqual(calendar.component(.weekday, from: now), 2)
    }

    func testNearDaysAreNamed() {
        XCTAssertEqual(text(at(2026, 11, 9, 15)), "Today 15:00")
        XCTAssertEqual(text(at(2026, 11, 10, 11)), "Tomorrow 11:00")
        XCTAssertEqual(text(at(2026, 11, 8, 15)), "Yesterday 15:00")
    }

    func testTheComingWeekUsesTheWeekday() {
        XCTAssertEqual(text(at(2026, 11, 11, 15)), "Wed 15:00")
        XCTAssertEqual(text(at(2026, 11, 15, 15)), "Sun 15:00")
    }

    func testLaterThisYearDropsTheYear() {
        XCTAssertEqual(text(at(2026, 11, 20, 15)), "20 Nov 15:00")
        XCTAssertEqual(text(at(2026, 11, 2, 15)), "2 Nov 15:00", "last week is a date, not a weekday that reads as next week")
    }

    func testAnotherYearDropsTheTime() {
        XCTAssertEqual(text(at(2027, 1, 5, 15)), "5 Jan 2027")
    }

    func testEveryFormStaysShort() {
        for due in [at(2026, 11, 9, 15), at(2026, 11, 11, 15), at(2026, 12, 24, 23, 59), at(2027, 9, 30, 11)] {
            XCTAssertLessThanOrEqual(text(due).count, 15, text(due))
        }
    }
}
