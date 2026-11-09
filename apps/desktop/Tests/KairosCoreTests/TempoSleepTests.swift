import XCTest
@testable import KairosCore

final class TempoSleepTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    let classic = TempoPreset(name: "C", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 2)

    private func finished(_ events: [TempoEvent]) -> [TempoPhase] {
        events.compactMap { if case let .finished(p, _, _, _, _) = $0 { return p } else { return nil } }
    }

    func testALongSleepLogsOnlyThePhaseThatRanAndThenGoesIdle() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        let wake = t0.addingTimeInterval(3 * 3600)
        var logged = finished(e.tick(now: wake))
        for s in 1...120 { logged += finished(e.tick(now: wake.addingTimeInterval(Double(s)))) }
        XCTAssertEqual(logged, [.focus], "no phantom sessions after waking")
        XCTAssertEqual(e.phase, .idle)
    }

    func testWakingInsideTheNextPhaseLandsInIt() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        let wake = t0.addingTimeInterval(25 * 60 + 120) // two minutes into the short break
        let events = e.tick(now: wake)
        XCTAssertEqual(finished(events), [.focus])
        XCTAssertEqual(e.phase, .shortBreak)
        XCTAssertEqual(e.remaining(now: wake), 180)
        XCTAssertTrue(e.tick(now: wake.addingTimeInterval(1)).isEmpty)
    }

    func testStopForgetsTheCycleCount() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        e.tick(now: t0.addingTimeInterval(1500)) // focus 1 done, short break
        e.stop(now: t0.addingTimeInterval(1600))
        e.start(now: t0.addingTimeInterval(2000))
        e.tick(now: t0.addingTimeInterval(3500))
        XCTAssertEqual(e.phase, .shortBreak, "a new timer starts a new cycle")
    }

    func testPausingAtTheEndStillCompletes() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        let end = t0.addingTimeInterval(1500)
        e.pause(now: end)
        let events = e.tick(now: end.addingTimeInterval(1))
        XCTAssertEqual(finished(events), [.focus])
        XCTAssertEqual(e.phase, .shortBreak)
    }
}

final class TempoModeTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    let relaxed = TempoPreset(name: "R", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 4)
    let deep = TempoPreset(name: "D", focusMin: 50, shortBreakMin: 10, longBreakMin: 30, cyclesBeforeLong: 4, strict: true)

    func testStrictChosenByHandHoldsForTheWholeTimer() {
        var e = TempoEngine(preset: relaxed)
        e.start(strict: true, now: t0)
        XCTAssertEqual(e.mode, .strict)
        e.tick(now: t0 + 1500)
        XCTAssertEqual(e.mode, .onBreak)
        e.tick(now: t0 + 1800)
        XCTAssertEqual(e.phase, .focus)
        XCTAssertEqual(e.mode, .strict, "the next focus phase is strict too")
        e.stop(now: t0 + 1900)
        XCTAssertEqual(e.mode, .off)
        e.start(now: t0 + 2000)
        XCTAssertEqual(e.mode, .focus, "a new timer does not inherit it")
    }

    func testAStrictPresetMakesFocusStrict() {
        var e = TempoEngine(preset: deep)
        e.start(now: t0)
        XCTAssertEqual(e.mode, .strict)
    }

    func testANewPresetMidPhaseAppliesFromTheNextPhase() {
        var e = TempoEngine(preset: relaxed)
        e.start(now: t0)
        e.use(deep)
        XCTAssertEqual(e.endsAt, t0 + 1500, "the running phase keeps its length")
        XCTAssertEqual(e.mode, .focus, "and its mode")
        e.tick(now: t0 + 1500)
        XCTAssertEqual(e.remaining(now: t0 + 1500), 600, "the break is the new preset's")
        e.tick(now: t0 + 2100)
        XCTAssertEqual(e.mode, .strict)
        XCTAssertEqual(e.remaining(now: t0 + 2100), 3000)
    }

    func testAPresetChosenWhileIdleAppliesAtOnce() {
        var e = TempoEngine(preset: relaxed)
        e.use(deep)
        e.start(now: t0)
        XCTAssertEqual(e.remaining(now: t0), 3000)
    }
}

final class ScreensTests: XCTestCase {
    func testTheOverlayGoesToTheDisplayShowingMostOfTheWindow() {
        let main = (id: UInt32(1), bounds: CGRect(x: 0, y: 0, width: 1440, height: 900))
        let right = (id: UInt32(2), bounds: CGRect(x: 1440, y: -200, width: 2560, height: 1440))
        XCTAssertEqual(Screens.display(showing: CGRect(x: 1300, y: 100, width: 800, height: 600), among: [main, right]), 2)
        XCTAssertEqual(Screens.display(showing: CGRect(x: 100, y: 100, width: 800, height: 600), among: [main, right]), 1)
        XCTAssertNil(Screens.display(showing: CGRect(x: -5000, y: 0, width: 10, height: 10), among: [main, right]))
    }
}
