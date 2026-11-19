import XCTest
@testable import KairosCore

/// "Start the next focus session automatically" off: a break that runs out waits for the owner.
final class BreakWaitTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)
    let classic = TempoPreset(name: "C", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 2)

    /// Focus runs out, then the short break runs out, with the setting off.
    private func engineWaitingAfterFirstBreak() -> (TempoEngine, [TempoEvent]) {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        e.tick(now: t0.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        let events = e.tick(now: t0.addingTimeInterval(30 * 60), startsFocusAfterBreak: false)
        return (e, events)
    }

    func testTheSettingIsOffByDefault() {
        XCTAssertFalse(Settings().startsFocusAfterBreak)
    }

    func testABreakThatRunsOutWaitsInsteadOfStartingFocus() {
        var (e, events) = engineWaitingAfterFirstBreak()
        XCTAssertEqual(e.phase, .idle)
        XCTAssertEqual(e.mode, .off)
        XCTAssertTrue(e.awaitingFocus)
        XCTAssertEqual(events.last, .awaitingFocus)
        guard case let .finished(phase, _, _, _, _, completed) = events.first else { return XCTFail("the break is logged") }
        XCTAssertEqual(phase, .shortBreak)
        XCTAssertTrue(completed)
        XCTAssertTrue(e.tick(now: t0.addingTimeInterval(60 * 60), startsFocusAfterBreak: false).isEmpty, "nothing runs while waiting")
    }

    func testFocusRunningOutStillGoesStraightToTheBreak() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        e.tick(now: t0.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        XCTAssertEqual(e.phase, .shortBreak)
        XCTAssertFalse(e.awaitingFocus)
    }

    func testStartingAfterTheWaitKeepsTheCountForTheLongBreak() {
        var (e, _) = engineWaitingAfterFirstBreak()
        let start = t0.addingTimeInterval(40 * 60)
        e.start(.focus, now: start)
        XCTAssertFalse(e.awaitingFocus)
        e.tick(now: start.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        XCTAssertEqual(e.phase, .longBreak, "the second focus of two before a long break")
    }

    func testStoppingTheWaitStartsANewCount() {
        var (e, _) = engineWaitingAfterFirstBreak()
        XCTAssertEqual(e.stop(now: t0.addingTimeInterval(35 * 60)), [.stopped])
        XCTAssertFalse(e.awaitingFocus)
        let start = t0.addingTimeInterval(40 * 60)
        e.start(.focus, now: start)
        e.tick(now: start.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        XCTAssertEqual(e.phase, .shortBreak)
    }

    func testNotNowEndsTheWaitAndTheCount() {
        var (e, _) = engineWaitingAfterFirstBreak()
        e.endWait()
        XCTAssertFalse(e.awaitingFocus)
        XCTAssertEqual(e.phase, .idle)
        let start = t0.addingTimeInterval(40 * 60)
        e.start(.focus, now: start)
        e.tick(now: start.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        XCTAssertEqual(e.phase, .shortBreak, "a new count, so a short break")
    }

    func testSkippingABreakStillStartsFocusAtOnce() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        e.tick(now: t0.addingTimeInterval(25 * 60), startsFocusAfterBreak: false)
        e.skip(now: t0.addingTimeInterval(26 * 60))
        XCTAssertEqual(e.phase, .focus)
        XCTAssertFalse(e.awaitingFocus)
    }

    func testWithTheSettingOnABreakRunsStraightIntoFocus() {
        var e = TempoEngine(preset: classic)
        e.start(now: t0)
        e.tick(now: t0.addingTimeInterval(25 * 60), startsFocusAfterBreak: true)
        let events = e.tick(now: t0.addingTimeInterval(30 * 60), startsFocusAfterBreak: true)
        XCTAssertEqual(e.phase, .focus)
        XCTAssertEqual(events.last, .started(phase: .focus))
    }

    func testASleepPastTheNextFocusStillJustStops() {
        var e = TempoEngine(preset: classic)
        e.start(.shortBreak, now: t0)
        let events = e.tick(now: t0.addingTimeInterval(5 * 3600), startsFocusAfterBreak: false)
        XCTAssertEqual(events.last, .stopped)
        XCTAssertFalse(e.awaitingFocus, "hours later there is no break to come back from")
    }

    func testAWaitThenAStrictStartHoldsStrict() {
        var (e, _) = engineWaitingAfterFirstBreak()
        e.start(.focus, strict: true, now: t0.addingTimeInterval(40 * 60))
        XCTAssertEqual(e.mode, .strict)
    }
}

final class MenuBarTitleTests: XCTestCase {
    func testAShortNameIsShownWhole() {
        XCTAssertEqual(MenuBarTitle.make(name: "Work", phase: .idle, remaining: 0, awaitingFocus: false), "Work")
    }

    func testALongNameIsCutWithAnEllipsis() {
        let name = "Write the quarterly report for the board"
        let idle = MenuBarTitle.make(name: name, phase: .idle, remaining: 0, awaitingFocus: false)
        XCTAssertEqual(idle, "Write the quarterly…")
        XCTAssertEqual(idle.count, MenuBarTitle.idleLimit)
        let running = MenuBarTitle.make(name: name, phase: .focus, remaining: 754, awaitingFocus: false)
        XCTAssertEqual(running, "Write the quart… · 12:34")
    }

    func testAnEllipsisNeverFollowsASpace() {
        XCTAssertEqual(MenuBarTitle.make(name: "Read the books on the shelf", phase: .focus, remaining: 60, awaitingFocus: false),
                       "Read the books… · 1:00")
    }

    func testABreakShowsOnlyTheClock() {
        XCTAssertEqual(MenuBarTitle.make(name: "Write the quarterly report", phase: .shortBreak, remaining: 299.5, awaitingFocus: false),
                       "Break · 5:00")
    }

    func testWaitingAfterABreakSaysSo() {
        XCTAssertEqual(MenuBarTitle.make(name: "Work", phase: .idle, remaining: 0, awaitingFocus: true), "Break over")
    }

    func testEmojiCountAsOneCharacter() {
        let name = String(repeating: "🍅", count: 30)
        XCTAssertEqual(MenuBarTitle.make(name: name, phase: .idle, remaining: 0, awaitingFocus: false).count, MenuBarTitle.idleLimit)
    }
}
