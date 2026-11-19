import XCTest
@testable import KairosCore

/// Strict survives quitting Kairos: the strict focus phase's end time is saved, and a relaunch
/// before that time comes back in the same strict phase instead of off.
final class StrictRelaunchTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    private func savedMidStrict(until: Date, savedAt: Date) throws -> AppData {
        var e = TempoEngine(preset: TempoPreset.defaults[1]) // Deep work: strict, 50 min
        e.start(now: t0)
        var d = AppData.starter()
        d.focus.mode = .strict
        d.focus.endsAt = e.endsAt
        d.focus.strictUntil = until
        d.openSession = Session(try XCTUnwrap(e.current(now: savedAt)), taskId: nil, groupId: d.groups[0].id)
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("strict-\(UUID())")
        let store = DataStore(url: dir.appendingPathComponent("kairos.json"))
        try store.save(d)
        return store.load()
    }

    func testARelaunchBeforeTheEndResumesTheStrictPhaseWithTheTimeLeft() throws {
        var d = try savedMidStrict(until: t0 + 3000, savedAt: t0 + 600)
        let resume = try XCTUnwrap(d.recoverAfterLaunch(now: t0 + 900))
        XCTAssertEqual(d.focus.mode, .strict)
        XCTAssertEqual(d.focus.strictUntil, t0 + 3000)
        XCTAssertEqual(resume.endsAt, t0 + 3000)
        XCTAssertEqual(resume.startedAt, t0, "the phase keeps its start, so it is logged once, whole")
        XCTAssertTrue(d.sessions.isEmpty, "the phase is still running, so nothing is logged yet")

        var tempo = TempoEngine(preset: TempoPreset.defaults[0])
        tempo.resumeStrict(resume)
        XCTAssertEqual(tempo.phase, .focus)
        XCTAssertEqual(tempo.mode, .strict)
        XCTAssertEqual(tempo.remaining(now: t0 + 900), 2100)
        let events = tempo.tick(now: t0 + 3000)
        XCTAssertEqual(events.first, .finished(phase: .focus, start: t0, end: t0 + 3000, duration: 3000, interruptions: 0, completed: true))
        XCTAssertEqual(tempo.phase, .shortBreak)
    }

    func testARelaunchAfterTheEndComesBackOff() throws {
        var d = try savedMidStrict(until: t0 + 3000, savedAt: t0 + 600)
        XCTAssertNil(d.recoverAfterLaunch(now: t0 + 3001))
        XCTAssertEqual(d.focus.mode, .off)
        XCTAssertNil(d.focus.strictUntil)
        XCTAssertEqual(d.sessions.map(\.end), [t0 + 600], "logged up to the last save, as for any other phase")
    }

    func testTheStatePushedToChromeCarriesStrictUntilAndTheOverrideLength() throws {
        var d = AppData.starter()
        d.settings.overrideMinutes = 7
        var payload = BridgeState.payload(data: d, ledger: OverrideLedger())
        XCTAssertEqual(payload["overrideMinutes"] as? Int, 7)
        XCTAssertTrue(payload["strictUntil"] is NSNull)
        d.focus.mode = .strict
        d.focus.strictUntil = Date(timeIntervalSince1970: 1_794_252_486.430)
        payload = BridgeState.payload(data: d, ledger: OverrideLedger())
        XCTAssertEqual(payload["strictUntil"] as? Int, 1_794_252_486_430)
        XCTAssertTrue(JSONSerialization.isValidJSONObject(payload))
    }

    func testTheConfirmationNamesTheEndAndSaysThereIsNoWayOut() {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "UTC")!
        let ends = cal.date(from: DateComponents(year: 2026, month: 11, day: 9, hour: 15, minute: 25))!
        let c = StrictConfirmation(minutes: 50, endsAt: ends, locale: Locale(identifier: "en_GB"), timeZone: cal.timeZone)
        XCTAssertTrue(c.title.contains("15:25"), c.title)
        XCTAssertTrue(c.message.contains("stopped, skipped or quit"), c.message)
        XCTAssertTrue(c.message.contains("50 minutes"), c.message)
    }
}
