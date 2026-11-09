import XCTest
@testable import KairosCore

final class HistoryCapTests: XCTestCase {
    let now = Date(timeIntervalSince1970: 2_000_000_000)
    private func daysAgo(_ d: Double) -> Date { now.addingTimeInterval(-d * 86400) }
    private func session(_ end: Date) -> Session {
        Session(taskId: nil, groupId: nil, start: end.addingTimeInterval(-1500), end: end, kind: .focus)
    }

    func testLaunchTrimsHistoryEvenWhenNothingNewIsLogged() {
        var data = AppData.starter()
        data.sessions = [session(daysAgo(500)), session(daysAgo(1))]
        data.blockEvents = [BlockEvent(groupId: nil, target: "old.com", layer: "rule", time: daysAgo(500)),
                            BlockEvent(groupId: nil, target: "new.com", layer: "rule", time: daysAgo(1))]
        data.recoverAfterLaunch(now: now)
        XCTAssertEqual(data.sessions.count, 1)
        XCTAssertEqual(data.blockEvents.map(\.target), ["new.com"], "block events age out too")
    }

    func testAnOldSessionOutOfOrderIsStillTrimmed() {
        var data = AppData.starter()
        data.sessions = [session(daysAgo(1)), session(daysAgo(500))]
        data.trimHistory(now: now)
        XCTAssertEqual(data.sessions.map(\.end), [daysAgo(1)])
    }

    func testSessionsAreCappedByCountAsWellAsAge() {
        var data = AppData.starter()
        data.sessions = (0..<(AppData.maxSessions + 10)).map { session(now.addingTimeInterval(-Double($0))) }.reversed()
        data.trimHistory(now: now)
        XCTAssertEqual(data.sessions.count, AppData.maxSessions)
        XCTAssertEqual(data.sessions.last?.end, now, "the newest are kept")
    }
}
