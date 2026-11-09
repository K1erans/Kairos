import XCTest
@testable import KairosCore

final class AutoHideTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    let away = DockAutoHide.Inputs()
    let edge = DockAutoHide.Inputs(atEdge: true)
    let onBar = DockAutoHide.Inputs(onBar: true)

    /// Runs the machine as the window does: an update at every decision time.
    private func settle(_ a: inout DockAutoHide, _ inputs: DockAutoHide.Inputs, from start: Date, for seconds: TimeInterval) {
        var now = start
        a.update(inputs, now: now)
        while let next = a.nextDecision(), next <= start + seconds, next > now {
            now = next
            a.update(inputs, now: now)
        }
    }

    private func hidden(_ start: Date = Date(timeIntervalSince1970: 1_000_000)) -> DockAutoHide {
        var a = DockAutoHide(now: start)
        settle(&a, away, from: start, for: 5)
        return a
    }

    func testItHidesShortlyAfterThePointerLeaves() {
        var a = DockAutoHide(now: t0)
        a.update(away, now: t0 + 0.4)
        XCTAssertEqual(a.state, .shown(leftAt: t0), "still out inside the hide delay")
        a.update(away, now: t0 + DockAutoHide.hideDelay)
        XCTAssertEqual(a.state, .hiding(from: 1, at: t0 + DockAutoHide.hideDelay))
        XCTAssertFalse(a.isOut, "it stops taking clicks as it leaves")
        a.update(away, now: t0 + DockAutoHide.hideDelay + DockAutoHide.slide)
        XCTAssertEqual(a.state, .hidden(armedAt: nil))
        XCTAssertEqual(a.visible(at: t0 + 9), 0)
    }

    func testItStaysOutWhileThePointerIsOnItOrSomethingHoldsIt() {
        var a = DockAutoHide(now: t0)
        for s in stride(from: 0.0, through: 10, by: 0.25) { a.update(onBar, now: t0 + s) }
        XCTAssertEqual(a.state, .shown(leftAt: nil))
        // A context menu stays open while the pointer is off the bar.
        for s in stride(from: 10.0, through: 20, by: 0.25) { a.update(.init(held: true), now: t0 + s) }
        XCTAssertEqual(a.state, .shown(leftAt: nil))
        a.update(away, now: t0 + 20.25)
        XCTAssertEqual(a.state, .shown(leftAt: t0 + 20.25), "the hide delay starts when the menu closes")
    }

    func testTouchingTheEdgeRevealsAfterABriefDelay() {
        var a = hidden()
        let t = t0 + 10
        a.update(edge, now: t)
        XCTAssertEqual(a.state, .hidden(armedAt: t))
        XCTAssertEqual(a.nextDecision(), t + DockAutoHide.revealDelay, "the window wakes even if the pointer rests")
        a.update(edge, now: t + DockAutoHide.revealDelay / 2)
        XCTAssertEqual(a.state, .hidden(armedAt: t))
        a.update(edge, now: t + DockAutoHide.revealDelay)
        XCTAssertEqual(a.state, .revealing(from: 0, at: t + DockAutoHide.revealDelay))
        XCTAssertTrue(a.isOut)
        a.update(onBar, now: t + DockAutoHide.revealDelay + DockAutoHide.slide)
        XCTAssertEqual(a.state, .shown(leftAt: nil))
    }

    func testBrushingPastTheEdgeDoesNotReveal() {
        var a = hidden()
        a.update(edge, now: t0 + 10)
        a.update(away, now: t0 + 10.1)
        a.update(edge, now: t0 + 10.15)
        a.update(edge, now: t0 + 10.25)
        XCTAssertEqual(a.state, .hidden(armedAt: t0 + 10.15), "the delay restarts after leaving the strip")
        XCTAssertEqual(a.visible(at: t0 + 10.25), 0)
    }

    func testHeldAloneNeverCallsAHiddenBarBack() {
        var a = hidden()
        a.update(.init(held: true), now: t0 + 10)
        a.update(.init(held: true), now: t0 + 11)
        XCTAssertEqual(a.state, .hidden(armedAt: nil))
    }

    func testReturningMidSlideReversesWithoutAJump() {
        var a = DockAutoHide(now: t0)
        let hideAt = t0 + DockAutoHide.hideDelay
        a.update(away, now: hideAt)
        let mid = hideAt + DockAutoHide.slide * 0.4
        let before = a.visible(at: mid)
        a.update(onBar, now: mid)
        guard case let .revealing(from, at) = a.state else { return XCTFail("expected revealing, got \(a.state)") }
        XCTAssertEqual(at, mid)
        XCTAssertEqual(from, 0.6, accuracy: 1e-6)
        XCTAssertEqual(a.visible(at: mid), before, accuracy: 1e-6, "no jump")
        XCTAssertEqual(a.nextDecision()!.timeIntervalSince(mid), DockAutoHide.slide * 0.4, accuracy: 1e-6, "only the rest of the way")
    }

    func testTheSlideIsEasedAndMonotonic() {
        var a = hidden()
        a.update(edge, now: t0 + 10)
        a.update(edge, now: t0 + 10.2)
        let start = t0 + 10.2
        let samples = stride(from: 0.0, through: DockAutoHide.slide, by: 0.01).map { a.visible(at: start + $0) }
        XCTAssertEqual(samples, samples.sorted())
        XCTAssertLessThan(samples[3], 0.1, "eases in: slow at the start")
        XCTAssertGreaterThan(samples[samples.count - 4], 0.9, "eases out: slow at the end")
        XCTAssertEqual(samples.last!, 1, accuracy: 1e-9)
    }

    func testTheEdgeStripSpansTheBar() {
        let g = DockGeometry(slots: [.newGroup, .divider, .addApp], icon: 36)
        XCTAssertTrue(g.touchesEdge(axis: g.restLength / 2, fromEdge: 0))
        XCTAssertTrue(g.touchesEdge(axis: 0, fromEdge: DockAutoHide.edgeStrip))
        XCTAssertFalse(g.touchesEdge(axis: g.restLength / 2, fromEdge: 6), "a pointer near the edge but not on it")
        XCTAssertFalse(g.touchesEdge(axis: g.restLength + 30, fromEdge: 0), "the edge beyond the bar's ends")
    }
}
