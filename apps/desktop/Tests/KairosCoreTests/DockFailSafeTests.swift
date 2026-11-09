import XCTest
@testable import KairosCore

/// Records every `launchctl` call; nothing is loaded.
final class FakeLaunchctl: CommandRunner {
    var log: [[String]] = []
    func run(_ tool: String, _ arguments: [String]) -> (status: Int32, output: String) {
        XCTAssertEqual(tool, "/bin/launchctl")
        log.append(arguments)
        return (0, "")
    }
}

/// If Kairos stops while it hides the macOS Dock, the Dock must still come back. Everything here
/// runs against fake `defaults`, `killall` and `launchctl` in a temporary folder.
final class DockFailSafeTests: XCTestCase {
    var dir: URL!
    var memory: UserDefaults!
    var suite = ""

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("kairos-dock-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        suite = "kairos.tests.dockfailsafe.\(UUID().uuidString)"
        memory = UserDefaults(suiteName: suite)
    }

    override func tearDownWithError() throws {
        memory.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: dir)
    }

    private var script: URL { dir.appendingPathComponent("dock-restore.sh") }
    private var toolLog: URL { dir.appendingPathComponent("tools.log") }

    /// Shell stand-ins for `defaults` and `killall` that log their arguments, one call per line.
    private func fakeTools() throws -> SystemDock.Tools {
        for name in ["defaults", "killall"] {
            let url = dir.appendingPathComponent(name)
            try "#!/bin/sh\nprintf '%s' \(name) >> \(SystemDock.quoted(toolLog.path)); printf '|%s' \"$@\" >> \(SystemDock.quoted(toolLog.path)); echo >> \(SystemDock.quoted(toolLog.path))\n"
                .write(to: url, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        }
        return SystemDock.Tools(defaults: dir.appendingPathComponent("defaults").path, killall: dir.appendingPathComponent("killall").path)
    }

    private func loggedCalls() -> [[String]] {
        let text = (try? String(contentsOf: toolLog, encoding: .utf8)) ?? ""
        return text.split(separator: "\n").map { $0.split(separator: "|", omittingEmptySubsequences: false).map(String.init) }
    }

    /// Runs `/bin/sh` on a script, refusing any that could reach the real Dock or launchd.
    @discardableResult
    private func shell(_ arguments: [String]) throws -> Int32 {
        for path in arguments where path.hasPrefix(dir.path) {
            let text = (try? String(contentsOfFile: path, encoding: .utf8)) ?? ""
            for real in ["/usr/bin/defaults", "/usr/bin/killall", "launchctl"] {
                guard !text.contains(real) else { XCTFail("\(path) would run the real \(real)"); return -1 }
            }
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: arguments[0])
        p.arguments = Array(arguments.dropFirst())
        try p.run()
        p.waitUntilExit()
        return p.terminationStatus
    }

    func testTheRestoreScriptPutsBackWhatRestoreWouldAndRemovesItself() throws {
        let dock = FakeDock()
        dock.domain = ["autohide": ("boolean", "0"), "autohide-delay": ("string", "it's 0.2")]
        let system = SystemDock(commands: dock, memory: memory, tools: try fakeTools(), restoreScript: script, memoryDomain: "com.kairos.test")
        system.replace()
        XCTAssertTrue(FileManager.default.fileExists(atPath: script.path), "written while the Dock is hidden")

        try shell(["/bin/sh", script.path])
        let calls = loggedCalls()
        XCTAssertEqual(calls, [
            ["defaults", "write", "com.apple.dock", "autohide", "-bool", "false"],
            ["defaults", "write", "com.apple.dock", "autohide-delay", "-string", "it's 0.2"],
            ["defaults", "delete", "com.kairos.test", "systemDockSaved"],
            ["killall", "Dock"],
        ])
        XCTAssertFalse(FileManager.default.fileExists(atPath: script.path))
    }

    func testRestoringFromKairosRemovesTheScript() throws {
        let system = SystemDock(commands: FakeDock(), memory: memory, tools: try fakeTools(), restoreScript: script)
        system.replace()
        XCTAssertTrue(FileManager.default.fileExists(atPath: script.path))
        system.restore()
        XCTAssertFalse(FileManager.default.fileExists(atPath: script.path))
    }

    func testADockHiddenByAnOlderBuildGetsItsScriptAtTheNextLaunch() throws {
        let dock = FakeDock(), tools = try fakeTools()
        SystemDock(commands: dock, memory: memory, tools: tools).replace() // no script, as before
        XCTAssertFalse(FileManager.default.fileExists(atPath: script.path))
        SystemDock(commands: dock, memory: memory, tools: tools, restoreScript: script).replace()
        XCTAssertTrue(FileManager.default.fileExists(atPath: script.path))
        XCTAssertEqual(dock.restarts, 1, "the Dock is not restarted again")
    }

    func testTheWatchdogRunsTheScriptOnlyWhenKairosIsNotRunning() throws {
        let marker = dir.appendingPathComponent("ran")
        let write = { try "#!/bin/sh\ntouch \(SystemDock.quoted(marker.path))\nrm -f \"$0\"\n".write(to: self.script, atomically: true, encoding: .utf8) }

        // pgrep leaves out its own ancestors, so the running stand-in for Kairos is a sibling.
        let sleeper = Process()
        sleeper.executableURL = URL(fileURLWithPath: "/bin/sleep")
        sleeper.arguments = ["30"]
        try sleeper.run()
        defer { sleeper.terminate() }
        let running = "sleep"
        try write()
        try shell(DockWatchdog(agents: dir, restoreScript: script, processName: running, commands: FakeLaunchctl()).arguments)
        XCTAssertFalse(FileManager.default.fileExists(atPath: marker.path), "Kairos is up: it will put the Dock back itself")

        let watchdog = DockWatchdog(agents: dir, restoreScript: script, processName: "kairos-gone-\(Int.random(in: 1000...9999))", commands: FakeLaunchctl())
        try shell(watchdog.arguments)
        XCTAssertTrue(FileManager.default.fileExists(atPath: marker.path), "Kairos is gone: the Dock comes back")
        XCTAssertFalse(FileManager.default.fileExists(atPath: script.path))

        try FileManager.default.removeItem(at: marker)
        XCTAssertEqual(try shell(watchdog.arguments), 0, "no script, nothing to do")
        XCTAssertFalse(FileManager.default.fileExists(atPath: marker.path))
    }

    func testTheWatchdogIsALaunchAgentThatComesAndGoesWithTheSwitch() throws {
        let launchctl = FakeLaunchctl()
        let agents = dir.appendingPathComponent("LaunchAgents")
        let watchdog = DockWatchdog(agents: agents, restoreScript: script, uid: 501, commands: launchctl)
        watchdog.remove()
        XCTAssertTrue(launchctl.log.isEmpty, "nothing installed, nothing to remove")

        watchdog.install()
        let plist = try XCTUnwrap(NSDictionary(contentsOf: watchdog.plistURL) as? [String: Any])
        XCTAssertEqual(plist["Label"] as? String, "com.kairos.dock-watchdog")
        XCTAssertEqual(plist["StartInterval"] as? Int, 15)
        XCTAssertEqual(plist["ProgramArguments"] as? [String], watchdog.arguments)
        XCTAssertEqual(launchctl.log.last, ["bootstrap", "gui/501", watchdog.plistURL.path])

        watchdog.remove()
        XCTAssertEqual(launchctl.log.last, ["bootout", "gui/501/com.kairos.dock-watchdog"])
        XCTAssertFalse(watchdog.isInstalled)
    }

    func testATerminationSignalReachesTheHandlerInsteadOfEndingTheProcess() {
        let queue = DispatchQueue(label: "kairos.tests.signal")
        let got = expectation(description: "handler")
        let trap = SignalTrap([SIGUSR1], queue: queue) { number in
            XCTAssertEqual(number, SIGUSR1)
            got.fulfill()
        }
        kill(getpid(), SIGUSR1)
        wait(for: [got], timeout: 2)
        trap.cancel()
    }

    func testUsingKairosBarInsteadOfTheDockTurnsOnAutoHide() {
        var s = Settings()
        XCTAssertFalse(s.autoHideBar)
        s.setReplacesSystemDock(true)
        XCTAssertTrue(s.replacesSystemDock)
        XCTAssertTrue(s.autoHideBar)
        s.autoHideBar = false
        s.setReplacesSystemDock(true)
        XCTAssertFalse(s.autoHideBar, "only switching it on sets auto-hide; the owner's later choice stands")
        s.setReplacesSystemDock(false)
        XCTAssertFalse(s.replacesSystemDock)
    }
}
