import XCTest
@testable import KairosCore

final class LenientTests: XCTestCase {
    let t = Date(timeIntervalSince1970: 1_800_000_000)

    /// Every field set away from its default, so a field that silently falls back shows up.
    var samples: [(String, any Lenient)] {
        var group = AppGroup(name: "Work", icon: "book.fill", colorHex: "#123456")
        group.dockApps = ["a"]; group.blockedApps = ["b"]; group.allowSites = ["c.com"]; group.blockSites = ["d.com"]
        group.hiddenElements = ["e.com": ["#x"]]; group.jevEnabled = false; group.jevThreshold = 0.4
        group.tempoPreset = "Sprint"; group.desktop = 2
        var task = TaskItem(title: "Write", groupId: UUID())
        task.notes = "n"; task.priority = 2; task.due = t; task.estimateMin = 30; task.status = .done
        task.parentId = UUID(); task.createdAt = t; task.completedAt = t; task.listenedSec = 9
        let session = Session(taskId: UUID(), groupId: UUID(), start: t, end: t + 600, activeSeconds: 540, kind: .rest, interruptions: 2, completed: false)
        let event = BlockEvent(groupId: UUID(), target: "com.hnc.Discord", label: "Discord", layer: "app", overridden: true, time: t)
        let preset = TempoPreset(name: "P", focusMin: 40, shortBreakMin: 7, longBreakMin: 20, cyclesBeforeLong: 3, strict: true)
        var settings = Settings()
        settings.dockEdge = .bottom; settings.dockVisible = false; settings.iconSize = 50; settings.switchBehavior = .hide
        settings.presets = [preset]; settings.neverAnalyse = ["x.com"]; settings.jevFailClosed = true
        settings.maxOverridesPerSession = 1; settings.overrideMinutes = 9; settings.breakOverlay = false
        settings.groupHotkeys = false; settings.voiceProvider = .openai; settings.voiceId = "nova"
        settings.autoHideBar = true; settings.replacesSystemDock = true
        var focus = FocusState()
        focus.activeGroupId = UUID(); focus.currentTaskId = UUID(); focus.mode = .strict; focus.endsAt = t
        var data = AppData()
        data.groups = [group]; data.tasks = [task]; data.sessions = [session]; data.blockEvents = [event]
        data.settings = settings; data.focus = focus; data.onboarded = true; data.openSession = session
        data.unassignedListenedSec = 5
        return [("AppGroup", group), ("TaskItem", task), ("Session", session), ("BlockEvent", event),
                ("TempoPreset", preset), ("Settings", settings), ("FocusState", focus), ("AppData", data)]
    }

    func testFieldTablesNameEveryStoredProperty() {
        for (name, sample) in samples {
            let stored = Set(Mirror(reflecting: sample).children.compactMap(\.label))
            XCTAssertEqual(Set(keys(of: sample)), stored, "\(name).fields must list every stored property")
        }
    }

    func testEachTypeDecodesWithEachKeyRemoved() throws {
        for (name, sample) in samples {
            try checkEachKeyRemoved(name, sample)
        }
    }

    private func keys<T: Lenient>(of _: T) -> [String] { T.fields.map(\.key) }

    private func checkEachKeyRemoved<T: Lenient>(_ name: String, _ sample: T) throws {
        let full = try object(sample)
        let fallback = try object(T.fallback), fallbackAgain = try object(T.fallback)
        for field in T.fields {
            var cut = full
            cut.removeValue(forKey: field.key)
            let data = try JSONSerialization.data(withJSONObject: cut)
            if field.required {
                XCTAssertThrowsError(try DataStore.decoder.decode(T.self, from: data), "\(name) without \(field.key)")
                let list = try JSONSerialization.data(withJSONObject: ["items": [full, cut, full]])
                let kept = try DataStore.decoder.decode(ListOf<T>.self, from: list)
                XCTAssertEqual(kept.items.count, 2, "\(name): an element without \(field.key) drops alone")
                continue
            }
            let decoded = try object(DataStore.decoder.decode(T.self, from: data))
            for (key, value) in full where key != field.key {
                XCTAssertEqual(decoded[key] as? NSObject, value as? NSObject, "\(name) without \(field.key) kept \(key)")
            }
            // Fresh ids and dates differ on every fallback; the rest must match the fallback.
            if (fallback[field.key] as? NSObject) == (fallbackAgain[field.key] as? NSObject) {
                XCTAssertEqual(decoded[field.key] as? NSObject, fallback[field.key] as? NSObject, "\(name).\(field.key) falls back")
            }
        }
        // A wrong type falls back too, rather than failing the value.
        for field in T.fields where !field.required {
            var bad = full
            bad[field.key] = ["unexpected": true]
            XCTAssertNoThrow(try DataStore.decoder.decode(T.self, from: JSONSerialization.data(withJSONObject: bad)), "\(name).\(field.key) of the wrong type")
        }
    }

    private func object<T: Encodable>(_ value: T) throws -> [String: Any] {
        try XCTUnwrap(JSONSerialization.jsonObject(with: DataStore.encoder.encode(value)) as? [String: Any])
    }

    func testOneBadFieldNoLongerDiscardsTheFile() throws {
        let json = #"{"groups":[{"name":"Work"}],"tasks":[{"id":"8C0C2D2E-5B0B-4C55-9A4A-2B8E0C1F6A11","title":"t","priority":0,"status":"todo","createdAt":"2026-11-09T10:00:00Z","listenedSec":0}],"sessions":[],"blockEvents":[],"settings":{"presets":[{"name":"A","focusMin":20,"shortBreakMin":5,"longBreakMin":15,"cyclesBeforeLong":4},{"name":"B","focusMin":30,"shortBreakMin":5,"longBreakMin":15,"cyclesBeforeLong":4,"strict":true},{"focusMin":1}]},"focus":{"mode":"off"},"onboarded":true}"#
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("lenient-\(UUID())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let url = dir.appendingPathComponent("kairos.json")
        try Data(json.utf8).write(to: url)
        let d = DataStore(url: url).load()
        XCTAssertEqual(d.tasks.map(\.title), ["t"], "a task without notes loads")
        XCTAssertEqual(d.groups.map(\.name), ["Work"], "the file is not swapped for starter data")
        XCTAssertEqual(d.settings.presets.map(\.name), ["A", "B"], "a preset without strict loads; a nameless one drops alone")
        XCTAssertEqual(d.settings.presets.map(\.strict), [false, true])
    }
}

private struct ListOf<T: Lenient>: Lenient {
    var items: [T] = []
    static var fallback: ListOf { ListOf() }
    static var fields: [LenientField<ListOf>] { [.field("items", \.items)] }
}
