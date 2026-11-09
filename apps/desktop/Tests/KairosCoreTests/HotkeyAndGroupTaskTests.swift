import XCTest
@testable import KairosCore

final class HotkeyPlanTests: XCTestCase {
    func testANewProfileRegistersQuickAddAndTimerButNotTheDesktopKeys() {
        let plan = HotkeyPlan(Settings())
        XCTAssertEqual(plan.entries.map(\.action), [.quickAdd, .timer])
        XCTAssertEqual(plan.entries.map(\.label), ["⌥Space", "⌥⇧Space"])
    }

    func testEachShortcutCanBeChangedOrTurnedOff() {
        var s = Settings()
        s.quickAddHotkey = "control-option-n"
        s.timerHotkey = Hotkeys.off
        s.groupHotkeys = true
        let plan = HotkeyPlan(s)
        XCTAssertEqual(plan.entries.first?.label, "⌃⌥N")
        XCTAssertFalse(plan.entries.contains { $0.action == .timer })
        XCTAssertEqual(plan.entries.filter { if case .group = $0.action { return true }; return false }.count, 9)
    }

    func testAnUnknownStoredShortcutFallsBackToTheDefault() {
        var s = Settings()
        s.quickAddHotkey = "hyper-q"
        XCTAssertEqual(HotkeyPlan(s).entries.first?.label, "⌥Space")
    }

    func testTheTwoListsNeverOfferTheSameShortcut() {
        let a = Set(Hotkeys.quickAdd.map { [$0.keyCode, $0.modifiers] })
        let b = Set(Hotkeys.timer.map { [$0.keyCode, $0.modifiers] })
        XCTAssertTrue(a.isDisjoint(with: b))
    }

    func testFailuresAreNamed() {
        XCTAssertNil(HotkeyPlan.failureText([]))
        XCTAssertTrue(HotkeyPlan.failureText(["⌥Space"])!.hasPrefix("⌥Space is already taken"))
        XCTAssertTrue(HotkeyPlan.failureText(["⌃1", "⌃2"])!.contains("are already taken"))
    }

    func testAnExistingProfileKeepsItsGroupKeys() throws {
        let json = #"{"groupHotkeys": true}"#
        let s = try DataStore.decoder.decode(Settings.self, from: Data(json.utf8))
        XCTAssertTrue(s.groupHotkeys)
        XCTAssertEqual(s.quickAddHotkey, "option-space")
    }
}

final class GroupTaskTests: XCTestCase {
    func testSwitchingAwayAndBackBringsTheTaskBack() {
        var d = AppData()
        let work = AppGroup(name: "Work"), study = AppGroup(name: "Study")
        d.groups = [work, study]
        var report = TaskItem(title: "Write report"); report.groupId = work.id
        var essay = TaskItem(title: "Essay"); essay.groupId = study.id
        d.tasks = [report, essay]
        d.focus.activeGroupId = work.id
        d.focus.currentTaskId = report.id

        d.focus = d.focusAfterSwitch(to: study.id)
        XCTAssertNil(d.focus.currentTaskId, "Study had no task yet")
        d.focus.currentTaskId = essay.id
        d.focus = d.focusAfterSwitch(to: work.id)
        XCTAssertEqual(d.focus.currentTaskId, report.id)
        d.focus = d.focusAfterSwitch(to: study.id)
        XCTAssertEqual(d.focus.currentTaskId, essay.id)
    }

    func testADoneOrDeletedTaskDoesNotComeBack() {
        var d = AppData()
        let work = AppGroup(name: "Work"), study = AppGroup(name: "Study")
        d.groups = [work, study]
        var report = TaskItem(title: "Write report"); report.groupId = work.id
        d.tasks = [report]
        d.focus.activeGroupId = work.id
        d.focus.currentTaskId = report.id
        d.focus = d.focusAfterSwitch(to: study.id)
        d.tasks[0].status = .done
        d.focus = d.focusAfterSwitch(to: work.id)
        XCTAssertNil(d.focus.currentTaskId)
    }
}
