import KairosCore
import XCTest

/// Stands in for NSEvent's monitors: records which exist and lets a test click.
private final class FakeMonitors {
    private(set) var live: [Int: (Bool) -> Void] = [:]
    private var next = 0

    func install(_ report: @escaping (Bool) -> Void) -> [Int] {
        // One global monitor (other apps, the desktop) and one local (Kairos's own windows).
        (0..<2).map { _ in next += 1; live[next] = report; return next }
    }

    func remove(_ id: Int) { live[id] = nil }
    func click(inside: Bool) { live.values.first?(inside) }
}

final class ClickAwayTests: XCTestCase {
    private func make() -> (ClickAway<Int>, FakeMonitors) {
        let fake = FakeMonitors()
        return (ClickAway(install: fake.install, remove: fake.remove), fake)
    }

    func testAClickOutsideClosesTheBoxAndRemovesTheMonitors() {
        let (watch, fake) = make()
        var closed = 0
        watch.start { closed += 1 }
        XCTAssertEqual(fake.live.count, 2)
        fake.click(inside: false)
        XCTAssertEqual(closed, 1)
        XCTAssertTrue(fake.live.isEmpty, "no monitors once the box is closed")
        XCTAssertFalse(watch.isWatching)
    }

    func testAClickInsideKeepsTheBoxOpen() {
        let (watch, fake) = make()
        var closed = 0
        watch.start { closed += 1 }
        fake.click(inside: true)
        XCTAssertEqual(closed, 0)
        XCTAssertEqual(fake.live.count, 2)
    }

    func testClosingByHotkeyOrEscapeRemovesTheMonitors() {
        let (watch, fake) = make()
        var closed = 0
        watch.start { closed += 1 }
        watch.stop()
        XCTAssertTrue(fake.live.isEmpty)
        XCTAssertEqual(closed, 0, "closing another way does not report a click away")
        watch.stop()
        XCTAssertTrue(fake.live.isEmpty, "stopping twice is harmless")
    }

    func testOpeningAgainDoesNotStackMonitors() {
        let (watch, fake) = make()
        var closed = 0
        watch.start { closed += 1 }
        watch.start { closed += 10 }
        XCTAssertEqual(fake.live.count, 2)
        fake.click(inside: false)
        XCTAssertEqual(closed, 10, "only the latest opening is closed")
    }
}
