import XCTest
@testable import KairosCore

/// Plays `defaults` and `killall` against an in-memory com.apple.dock domain.
final class FakeDock: CommandRunner {
    var domain: [String: (type: String, text: String)] = [:]
    var restarts = 0
    var log: [[String]] = []
    /// Stands in for the restore script, which the fake never executes.
    var onScript: (() -> Void)?
    var scriptRuns = 0

    func run(_ tool: String, _ arguments: [String]) -> (status: Int32, output: String) {
        log.append([tool] + arguments)
        if tool.hasSuffix("/killall") { restarts += 1; return (0, "") }
        if tool == "/bin/sh" { scriptRuns += 1; onScript?(); return (0, "") }
        XCTAssertTrue(tool.hasSuffix("/defaults"), tool)
        XCTAssertEqual(arguments[1], SystemDock.domain, "only the Dock's domain is touched")
        let key = arguments[2]
        switch arguments[0] {
        case "read-type": return domain[key].map { (0, "Type is \($0.type)\n") } ?? (1, "")
        case "read": return domain[key].map { (0, "\($0.text)\n") } ?? (1, "")
        case "delete": domain[key] = nil; return (0, "")
        case "write":
            let types = ["-bool": "boolean", "-float": "float", "-int": "integer", "-string": "string"]
            let type = types[arguments[3]]!
            let text = type == "boolean" ? (arguments[4] == "true" ? "1" : "0") : arguments[4]
            domain[key] = (type, text)
            return (0, "")
        default: XCTFail("unexpected \(arguments)"); return (1, "")
        }
    }

    func value(_ key: String) -> String? { domain[key].map { "\($0.type):\($0.text)" } }
}

final class SystemDockTests: XCTestCase {
    var memory: UserDefaults!
    var suite = ""

    override func setUp() {
        suite = "kairos.tests.systemdock.\(UUID().uuidString)"
        memory = UserDefaults(suiteName: suite)
    }

    override func tearDown() { memory.removePersistentDomain(forName: suite) }

    func testReplaceHidesTheDockAndARelaunchedKairosRestoresTheUsersValues() {
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "0"), "autohide-delay": ("float", "0.2"), "tilesize": ("integer", "48")]
        SystemDock(commands: dock, memory: memory).replace()
        XCTAssertEqual(dock.value("autohide"), "boolean:1")
        XCTAssertEqual(dock.value("autohide-delay"), "float:1000")
        XCTAssertEqual(dock.restarts, 1)

        // A fresh instance, as after quitting and relaunching Kairos, reads the saved values back.
        let relaunched = SystemDock(commands: dock, memory: UserDefaults(suiteName: suite)!)
        XCTAssertTrue(relaunched.isReplaced)
        relaunched.restore()
        XCTAssertEqual(dock.value("autohide"), "boolean:0")
        XCTAssertEqual(dock.value("autohide-delay"), "float:0.2")
        XCTAssertEqual(dock.value("tilesize"), "integer:48", "other Dock settings are untouched")
        XCTAssertFalse(relaunched.isReplaced)
        XCTAssertEqual(dock.restarts, 2)
    }

    func testRestoreDeletesWhatTheUserHadNeverSet() {
        let dock = FakeDock()
        let system = SystemDock(commands: dock, memory: memory)
        system.replace()
        XCTAssertEqual(dock.value("autohide"), "boolean:1")
        system.restore()
        XCTAssertNil(dock.value("autohide"))
        XCTAssertNil(dock.value("autohide-delay"))
        XCTAssertTrue(dock.domain.isEmpty)
    }

    func testReplacingTwiceKeepsTheFirstSavedValues() {
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "1"), "autohide-delay": ("integer", "0")]
        let system = SystemDock(commands: dock, memory: memory)
        system.replace()
        system.replace()
        XCTAssertEqual(dock.restarts, 1, "the second call does nothing")
        system.restore()
        XCTAssertEqual(dock.value("autohide"), "boolean:1")
        XCTAssertEqual(dock.value("autohide-delay"), "integer:0", "the user's type is kept")
    }

    func testRestoreWithNothingSavedLeavesTheDockAlone() {
        let dock = FakeDock()
        SystemDock(commands: dock, memory: memory).restore()
        XCTAssertTrue(dock.log.isEmpty)
    }

    func testTheDockIsHiddenOnlyWhileTheBarIsOnScreen() {
        var s = Settings()
        XCTAssertFalse(s.hidesSystemDock)
        s.replacesSystemDock = true
        XCTAssertTrue(s.hidesSystemDock)
        s.dockVisible = false
        XCTAssertFalse(s.hidesSystemDock, "hiding Kairos Bar brings the Dock back")
    }

    // Found by verification/system-dock: Crash, PrefsLost, Launch used to record Kairos's own
    // hidden setting as the owner's.
    func testLostMemoryWithAScriptLeftRunsItInsteadOfRecordingTheHiddenDock() throws {
        let script = FileManager.default.temporaryDirectory.appendingPathComponent("kairos-dock-\(UUID().uuidString).sh")
        defer { try? FileManager.default.removeItem(at: script) }
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "0")]
        SystemDock(commands: dock, memory: memory, restoreScript: script).replace()
        memory.removeObject(forKey: "systemDockSaved")
        XCTAssertTrue(FileManager.default.fileExists(atPath: script.path))
        dock.onScript = { dock.domain = ["autohide": ("boolean", "0")] }

        let relaunched = SystemDock(commands: dock, memory: memory, restoreScript: script)
        relaunched.replace()
        XCTAssertEqual(dock.scriptRuns, 1)
        XCTAssertEqual(dock.value("autohide-delay"), "float:1000")
        relaunched.restore()
        XCTAssertEqual(dock.value("autohide"), "boolean:0")
        XCTAssertNil(dock.value("autohide-delay"))
    }

    func testRestoreWithLostMemoryRunsTheScriptLeftOnDisk() {
        let script = FileManager.default.temporaryDirectory.appendingPathComponent("kairos-dock-\(UUID().uuidString).sh")
        defer { try? FileManager.default.removeItem(at: script) }
        let dock = FakeDock()
        SystemDock(commands: dock, memory: memory, restoreScript: script).replace()
        memory.removeObject(forKey: "systemDockSaved")
        SystemDock(commands: dock, memory: memory, restoreScript: script).restore()
        XCTAssertEqual(dock.scriptRuns, 1)
        XCTAssertFalse(FileManager.default.fileExists(atPath: script.path))
    }

    func testKairossOwnHiddenSettingIsNeverRecordedAsTheOwners() {
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "1"), "autohide-delay": ("float", "1000")]
        let system = SystemDock(commands: dock, memory: memory)
        system.replace()
        system.restore()
        XCTAssertNil(dock.value("autohide"), "back to the macOS default, a Dock that stays on screen")
        XCTAssertNil(dock.value("autohide-delay"))
    }

    func testReplaceShowsOnlyRunningAppsAndRestorePutsTheOwnersSettingBack() {
        let dock = FakeDock()
        dock.domain = ["static-only": ("boolean", "0")]
        let system = SystemDock(commands: dock, memory: memory)
        system.replace()
        XCTAssertEqual(dock.value("static-only"), "boolean:1", "Mission Control shows only a short strip")
        system.restore()
        XCTAssertEqual(dock.value("static-only"), "boolean:0")

        dock.domain = [:]
        system.replace()
        XCTAssertEqual(dock.value("static-only"), "boolean:1")
        system.restore()
        XCTAssertNil(dock.value("static-only"), "a setting the owner never had is deleted")
    }

    // The owner's Mac was hidden by a build that knew only the two auto-hide keys.
    func testADockHiddenByAnOlderBuildGetsTheNewSettingAndKeepsTheOwnersValues() {
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "1"), "autohide-delay": ("float", "1000")]
        memory.set(["autohide": ["boolean", "0"]], forKey: "systemDockSaved")
        let system = SystemDock(commands: dock, memory: memory)
        system.replace()
        XCTAssertEqual(dock.value("static-only"), "boolean:1")
        XCTAssertEqual(dock.restarts, 1)
        system.replace()
        XCTAssertEqual(dock.restarts, 1, "recorded once; the next launch does nothing")
        system.restore()
        XCTAssertEqual(dock.value("autohide"), "boolean:0")
        XCTAssertNil(dock.value("autohide-delay"))
        XCTAssertNil(dock.value("static-only"))
    }

    func testTheRestoreScriptPutsBackTheRunningAppsSetting() {
        let dock = FakeDock()
        let text = SystemDock(commands: dock, memory: memory).restoreScriptText(["static-only": ["boolean", "1"]])
        XCTAssertTrue(text.contains("'write' 'com.apple.dock' 'static-only' '-bool' 'true'"), text)
        XCTAssertTrue(text.contains("'delete' 'com.apple.dock' 'autohide'"), text)
    }
}
