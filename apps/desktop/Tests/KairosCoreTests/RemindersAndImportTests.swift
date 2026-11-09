import XCTest
@testable import KairosCore

final class DueReminderTests: XCTestCase {
    let now = Date(timeIntervalSince1970: 2_000_000_000)

    private func data(_ tasks: [TaskItem]) -> AppData {
        var d = AppData.starter()
        d.tasks = tasks
        return d
    }
    private func task(_ title: String, in minutes: Double?, done: Bool = false, group: UUID? = nil) -> TaskItem {
        var t = TaskItem(title: title, groupId: group)
        t.due = minutes.map { now.addingTimeInterval($0 * 60) }
        if done { t.status = .done }
        return t
    }

    func testOnlyUnfinishedTasksDueLaterAreReminded() {
        let d = data([task("later", in: 30), task("past", in: -5), task("done", in: 30, done: true), task("no date", in: nil)])
        XCTAssertEqual(DueReminders.plan(.init(d), now: now).map(\.title), ["later"])
    }

    func testTheSoonestComeFirstAndThereAreNeverMoreThan64() {
        let d = data((1...100).reversed().map { task("t\($0)", in: Double($0)) })
        let plan = DueReminders.plan(.init(d), now: now)
        XCTAssertEqual(plan.count, DueReminders.limit)
        XCTAssertEqual(plan.first?.title, "t1")
        XCTAssertEqual(plan.last?.title, "t64")
    }

    func testAReminderSaysWhenAndWhere() {
        var d = AppData.starter()
        let work = d.groups[0]
        let t = task("Send the report", in: 10, group: work.id)
        d.tasks = [t]
        let r = DueReminders.plan(.init(d), now: now)[0]
        XCTAssertEqual(r.fireAt, t.due)
        XCTAssertEqual(r.body, "Due now, in \(work.name).")
        XCTAssertEqual(r.id, DueReminder.idPrefix + t.id.uuidString)
    }

    func testTurningRemindersOffPlansNone() {
        var d = data([task("later", in: 30)])
        d.settings.notifiesWhenDue = false
        XCTAssertTrue(DueReminders.plan(.init(d), now: now).isEmpty)
    }
}

final class ImportTests: XCTestCase {
    let now = Date(timeIntervalSince1970: 2_000_000_000)

    func testSomethingThatIsNotAnExportIsRefused() {
        XCTAssertThrowsError(try AppData.fromExport(Data("{}".utf8)))
        XCTAssertThrowsError(try AppData.fromExport(Data("not json".utf8)))
        XCTAssertNoThrow(try AppData.fromExport(DataStore.encoder.encode(AppData.starter())))
    }

    func testImportTakesTheFilesContentButKeepsWhatIsLiveHere() {
        var current = AppData.starter()
        current.focus.mode = .focus
        current.seenClients = ["shield"]
        current.onboarded = true
        var file = AppData.starter()
        file.groups = [AppGroup(name: "Imported", icon: "pencil", colorHex: "#6B7280")]
        file.tasks = [TaskItem(title: "From the file")]
        file.focus.mode = .off
        file.focus.activeGroupId = UUID() // a group the file does not have
        file.focus.currentTaskId = UUID()
        file.onboarded = false
        let next = current.importing(file, now: now)
        XCTAssertEqual(next.groups.map(\.name), ["Imported"])
        XCTAssertEqual(next.tasks.map(\.title), ["From the file"])
        XCTAssertEqual(next.focus.mode, .focus, "the mode is not the file's to change")
        XCTAssertEqual(next.focus.activeGroupId, file.groups[0].id)
        XCTAssertNil(next.focus.currentTaskId)
        XCTAssertEqual(next.seenClients, ["shield"])
        XCTAssertTrue(next.onboarded)
    }

    func testTheCurrentFileIsBackedUpBesideItFirst() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("kairos-import-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = DataStore(url: dir.appendingPathComponent("kairos.json"))
        XCTAssertNil(try store.backup(now: now), "nothing saved yet, nothing to back up")
        var data = AppData.starter()
        data.tasks = [TaskItem(title: "Keep me")]
        try store.save(data)
        let first = try XCTUnwrap(store.backup(now: now))
        let second = try XCTUnwrap(store.backup(now: now))
        XCTAssertNotEqual(first, second, "a second backup in the same second does not overwrite the first")
        XCTAssertEqual(first.deletingLastPathComponent().standardizedFileURL.path, dir.standardizedFileURL.path)
        XCTAssertTrue(first.lastPathComponent.hasPrefix("kairos-backup-"))
        let restored = try AppData.fromExport(Data(contentsOf: first))
        XCTAssertEqual(restored.tasks.map(\.title), ["Keep me"])
    }
}
