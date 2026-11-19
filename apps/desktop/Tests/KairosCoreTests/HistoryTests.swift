import XCTest
@testable import KairosCore

final class HistoryTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    func testOverrideMarksTheEventForTheAllowedTarget() {
        var d = AppData.starter()
        d.logBlock(BlockEvent(groupId: nil, target: "youtube.com", layer: "rule"))
        d.logBlock(BlockEvent(groupId: nil, target: "com.hnc.Discord", label: "Discord", layer: "app"))
        d.logBlock(BlockEvent(groupId: nil, target: "reddit.com", layer: "rule"))
        d.markOverridden(target: "youtube.com")
        d.markOverridden(target: "com.hnc.Discord")
        XCTAssertEqual(d.blockEvents.map(\.overridden), [true, true, false])
        XCTAssertEqual(d.blockEvents[1].displayName, "Discord")
    }

    func testAGrantCoversSubdomainEventsAndSkipsMarkedOnes() {
        var d = AppData()
        d.logBlock(BlockEvent(groupId: nil, target: "m.youtube.com", layer: "rule"))
        d.logBlock(BlockEvent(groupId: nil, target: "youtube.com", layer: "rule", overridden: true))
        d.markOverridden(target: "youtube.com")
        XCTAssertEqual(d.blockEvents.map(\.overridden), [true, true])
        d.markOverridden(target: "x.com")
        XCTAssertEqual(d.blockEvents.map(\.overridden), [true, true])
    }

    func testListeningGoesToTheCurrentTaskOrToUnassigned() {
        var d = AppData.starter()
        d.logListening(seconds: 30)
        XCTAssertEqual(d.unassignedListenedSec, 30)
        let task = TaskItem(title: "Read", groupId: d.groups[0].id)
        d.tasks.append(task)
        d.focus.currentTaskId = task.id
        d.logListening(seconds: 20)
        d.logListening(seconds: -5)
        XCTAssertEqual(d.tasks[0].listenedSec, 20)
        XCTAssertEqual(d.unassignedListenedSec, 30)
    }

    func testRelaunchLogsTheInterruptedPhaseAndStopsBlocking() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("relaunch-\(UUID())")
        let store = DataStore(url: dir.appendingPathComponent("kairos.json"))
        var e = TempoEngine()
        e.start(now: t0)
        var d = AppData.starter()
        d.focus.mode = .focus
        d.focus.endsAt = e.endsAt
        d.openSession = Session(try XCTUnwrap(e.current(now: t0 + 600)), taskId: nil, groupId: d.groups[0].id)
        try store.save(d)

        var back = store.load()
        back.recoverAfterLaunch(now: t0 + 3600)
        XCTAssertEqual(back.focus.mode, .off)
        XCTAssertNil(back.focus.endsAt)
        XCTAssertNil(back.openSession)
        XCTAssertEqual(back.sessions.count, 1)
        XCTAssertEqual(back.sessions[0].end, t0 + 600, "recorded up to the last save")
        XCTAssertFalse(back.sessions[0].completed)
    }

    func testRelaunchWhilePausedComesBackOff() {
        var e = TempoEngine()
        e.start(now: t0)
        e.pause(now: t0 + 300)
        var d = AppData.starter()
        d.focus.mode = .focus // paused: no end time, which used to keep focus on
        d.openSession = Session(e.current(now: t0 + 900)!, taskId: nil, groupId: nil)
        d.recoverAfterLaunch(now: t0 + 1000)
        XCTAssertEqual(d.focus.mode, .off)
        XCTAssertEqual(d.sessions.first?.end, t0 + 300, "a paused phase stopped counting when paused")
    }

    func testFalseStartsAreNotLogged() {
        var d = AppData()
        d.log(.finished(phase: .focus, start: t0, end: t0 + 10, duration: 10, interruptions: 0, completed: false))
        d.log(.started(phase: .focus))
        XCTAssertTrue(d.sessions.isEmpty)
        d.openSession = Session(taskId: nil, groupId: nil, start: t0, end: t0 + 5, kind: .focus)
        d.recoverAfterLaunch(now: t0)
        XCTAssertTrue(d.sessions.isEmpty)
    }

    func testHistoryIsCapped() {
        var d = AppData()
        let day = 86400.0
        d.sessions = [Session(taskId: nil, groupId: nil, start: t0 - 500 * day, end: t0 - 500 * day + 600, kind: .focus),
                      Session(taskId: nil, groupId: nil, start: t0 - 10 * day, end: t0 - 10 * day + 600, kind: .focus)]
        d.log(.finished(phase: .focus, start: t0, end: t0 + 600, duration: 600, interruptions: 0, completed: true), now: t0 + 600)
        XCTAssertEqual(d.sessions.count, 2, "sessions older than 400 days go")
        for i in 0..<(AppData.maxBlockEvents + 10) {
            d.logBlock(BlockEvent(groupId: nil, target: "s\(i).com", layer: "rule"), now: t0)
        }
        XCTAssertEqual(d.blockEvents.count, AppData.maxBlockEvents)
        XCTAssertEqual(d.blockEvents.last?.target, "s\(AppData.maxBlockEvents + 9).com", "the newest are kept")
    }

    func testHiddenElementsAddAndRemove() {
        var d = AppData.starter()
        func hide(_ sel: String, _ host: String, _ on: Bool) -> Bool { d.setElementHidden(sel, on: host, on, strictLocked: false) }
        XCTAssertTrue(hide("#a", "https://www.News.com/x", true))
        XCTAssertFalse(hide("#a", "news.com", true), "already hidden")
        XCTAssertTrue(hide("#b", "news.com", true))
        XCTAssertEqual(d.activeGroup?.hiddenElements["news.com"], ["#a", "#b"])
        XCTAssertTrue(hide("#a", "news.com", false))
        XCTAssertEqual(d.activeGroup?.hiddenElements["news.com"], ["#b"])
        XCTAssertTrue(hide("#b", "news.com", false))
        XCTAssertNil(d.activeGroup?.hiddenElements["news.com"], "no empty entry is left behind")
        XCTAssertFalse(hide("#zz", "other.com", false))
        XCTAssertFalse(hide("", "news.com", true))
        XCTAssertEqual(d.activeGroup?.hiddenElements, [:])
    }

    func testGroupSwitchRestoresOnlyAppsInTheDestinationGroup() {
        var work = AppGroup(name: "Work")
        work.dockApps = ["mail", "browser", "notes"]
        let policy = GroupSwitch(to: work, behavior: .hide)

        XCTAssertEqual(policy.action(for: "mail", isHidden: true, hiddenBySwitch: true), .unhide)
        XCTAssertNil(policy.action(for: "browser", isHidden: false, hiddenBySwitch: false))
        XCTAssertNil(policy.action(for: "notes", isHidden: true, hiddenBySwitch: false))
        XCTAssertNil(policy.action(for: "chat", isHidden: true, hiddenBySwitch: true))
        XCTAssertEqual(policy.action(for: "music", isHidden: false, hiddenBySwitch: false), .hide)
        XCTAssertNil(policy.action(for: "com.apple.finder", isHidden: false, hiddenBySwitch: false))
    }

    func testHidingOnlyTheBlockedAppsLeavesEveryOtherAppAlone() {
        var group = AppGroup(name: "Study")
        group.dockApps = ["chat"]
        group.blockedApps = ["chat", "mail"]
        let policy = GroupSwitch(to: group, behavior: .hideBlocked)
        XCTAssertEqual(policy.action(for: "chat", isHidden: false, hiddenBySwitch: false), .hide)
        XCTAssertNil(policy.action(for: "mail", isHidden: true, hiddenBySwitch: true))
        XCTAssertNil(policy.action(for: "music", isHidden: false, hiddenBySwitch: false))
        XCTAssertEqual(policy.action(for: "browser", isHidden: true, hiddenBySwitch: true), .unhide)
        XCTAssertEqual(SwitchBehavior(rawValue: "hideBlocked"), .hideBlocked)
    }

    func testPausedTimeIsExcludedFromLoggedFocusMinutes() {
        var e = TempoEngine(preset: .init(name: "Classic", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 4))
        var d = AppData()
        e.start(.focus, now: t0)
        // Pause 1: after 10 min active, paused for 10 min.
        e.pause(now: t0 + 600)
        e.resume(now: t0 + 1200)
        // Pause 2: after 5 more min active (15 min total), paused for 5 min.
        e.pause(now: t0 + 1500)
        e.resume(now: t0 + 1800)
        // Finish the remaining 10 min (wall clock is 40 min, active is 25 min).
        let events = e.tick(now: t0 + 2400)
        for event in events { d.log(event, now: t0 + 2400) }
        XCTAssertEqual(d.sessions.count, 1)
        XCTAssertEqual(d.sessions[0].duration, 1500, accuracy: 0.001)
        XCTAssertEqual(d.sessions[0].minutes, 25, accuracy: 0.001)
        XCTAssertEqual(d.sessions[0].end, t0 + 2400)
    }

    func testFalseStartWithLongPauseGapIsNotLogged() {
        var e = TempoEngine()
        var d = AppData()
        e.start(.focus, now: t0)
        e.pause(now: t0 + 10)
        e.resume(now: t0 + 120)
        // Active time is 20s (< 30s minimum), even though wall-clock span is 130s.
        for event in e.stop(now: t0 + 130) { d.log(event, now: t0 + 130) }
        XCTAssertTrue(d.sessions.isEmpty)

        // Same rule applies to crash recovery of an openSession.
        d.openSession = Session(taskId: nil, groupId: nil, start: t0, end: t0 + 130, activeSeconds: 20, kind: .focus)
        d.recoverAfterLaunch(now: t0 + 200)
        XCTAssertTrue(d.sessions.isEmpty)
    }

    func testStrictRelaunchPreservesPausedDuration() {
        var e = TempoEngine(preset: .init(name: "Classic", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 4))
        e.start(.focus, strict: true, now: t0)
        e.pause(now: t0 + 600)
        e.resume(now: t0 + 1200) // endsAt = t0 + 2100 (25m active, 10m paused)

        var d = AppData()
        d.focus.mode = .strict
        d.focus.strictUntil = e.endsAt
        d.openSession = e.current(now: t0 + 1500).flatMap { Session($0, taskId: nil, groupId: nil) }

        guard let resumed = d.recoverAfterLaunch(now: t0 + 1600) else { return XCTFail("strict phase must resume") }
        XCTAssertEqual(resumed.pausedDuration, 600, accuracy: 0.001)

        var relaunched = TempoEngine()
        relaunched.resumeStrict(resumed)
        let events = relaunched.tick(now: t0 + 2100)
        for event in events { d.log(event, now: t0 + 2100) }
        XCTAssertEqual(d.sessions.count, 1)
        XCTAssertEqual(d.sessions[0].minutes, 25, accuracy: 0.001)
    }

    func testLegacySessionWithoutActiveSecondsFallsBackToWallClock() throws {
        let json = """
        {"start":"2026-01-01T10:00:00Z","end":"2026-01-01T10:25:00Z","kind":"focus","interruptions":0,"completed":true}
        """.data(using: .utf8)!
        let decoded = try DataStore.decoder.decode(Session.self, from: json)
        XCTAssertNil(decoded.activeSeconds)
        XCTAssertEqual(decoded.duration, 1500, accuracy: 0.001)
        XCTAssertEqual(decoded.minutes, 25, accuracy: 0.001)
    }
}
