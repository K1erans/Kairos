import Foundation

/// Runs a command-line tool. Injected so tests never touch the real Dock.
public protocol CommandRunner {
    @discardableResult
    func run(_ tool: String, _ arguments: [String]) -> (status: Int32, output: String)
}

public struct ProcessRunner: CommandRunner {
    public init() {}

    @discardableResult
    public func run(_ tool: String, _ arguments: [String]) -> (status: Int32, output: String) {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: tool)
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do { try process.run() } catch { return (-1, "") }
        let out = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: out, as: UTF8.self))
    }
}

/// Gets the macOS Dock out of the way so Kairos Bar can stand in for it.
///
/// macOS has no way to remove or quit the Dock for good (it also runs Mission Control and
/// Launchpad). The supported workaround is to turn on its auto-hide and give the reveal a very
/// long delay, so it never slides out. `restore()` puts back exactly what was there before,
/// including "not set", and works after a relaunch because the old values live in `memory`.
///
/// While the Dock is hidden a shell script that does the same as `restore()` sits at
/// `restoreScript`, so `DockWatchdog` can bring the Dock back if Kairos stops without doing it.
public struct SystemDock {
    public static let domain = "com.apple.dock"
    static let keys = ["autohide", "autohide-delay"]
    static let savedKey = "systemDockSaved"
    static let hiddenDelay = "1000"

    /// The command-line tools, as paths. Tests point them at fakes.
    public struct Tools: Sendable {
        public var defaults: String
        public var killall: String
        public init(defaults: String, killall: String) { self.defaults = defaults; self.killall = killall }
        public static let system = Tools(defaults: "/usr/bin/defaults", killall: "/usr/bin/killall")
    }

    private let commands: CommandRunner
    private let memory: UserDefaults
    private let tools: Tools
    private let restoreScript: URL?
    /// The defaults domain `memory` writes to, which the script clears.
    private let memoryDomain: String

    public init(commands: CommandRunner = ProcessRunner(), memory: UserDefaults = .standard, tools: Tools = .system,
                restoreScript: URL? = nil, memoryDomain: String = Bundle.main.bundleIdentifier ?? "com.kairos.desktop") {
        self.commands = commands
        self.memory = memory
        self.tools = tools
        self.restoreScript = restoreScript
        self.memoryDomain = memoryDomain
    }

    /// True while Kairos has the Dock hidden and holds the user's values to put back.
    public var isReplaced: Bool { memory.dictionary(forKey: Self.savedKey) != nil }

    private var saved: [String: [String]]? { memory.dictionary(forKey: Self.savedKey) as? [String: [String]] }

    public func replace() {
        if let saved {
            // Hidden by a build that wrote no script: write it now.
            writeRestoreScript(saved)
            return
        }
        // key -> [type, value] as `defaults` reports them; a key the user never set is left out.
        var values: [String: [String]] = [:]
        for key in Self.keys {
            if let value = read(key) { values[key] = [value.type, value.text] }
        }
        memory.set(values, forKey: Self.savedKey)
        // Before the Dock goes, so there is never a hidden Dock without a way back.
        writeRestoreScript(values)
        defaults(["write", Self.domain, "autohide", "-bool", "true"])
        defaults(["write", Self.domain, "autohide-delay", "-float", Self.hiddenDelay])
        restartDock()
    }

    public func restore() {
        guard let saved else { return }
        for arguments in Self.restoreArguments(saved) { defaults(arguments) }
        memory.removeObject(forKey: Self.savedKey)
        if let restoreScript { try? FileManager.default.removeItem(at: restoreScript) }
        restartDock()
    }

    /// The `defaults` calls that put back what `saved` holds, deleting what the user never set.
    static func restoreArguments(_ saved: [String: [String]]) -> [[String]] {
        keys.map { key in
            if let value = saved[key], value.count == 2, let flag = flag(type: value[0]) {
                return ["write", domain, key, flag, argument(type: value[0], text: value[1])]
            }
            return ["delete", domain, key]
        }
    }

    /// `restore()` as a shell script that removes itself when done.
    func restoreScriptText(_ saved: [String: [String]]) -> String {
        var lines = [
            "#!/bin/sh",
            "# Written by Kairos while it hides the macOS Dock, and removed when Kairos puts the Dock back.",
            "# If Kairos stops without doing that, its Dock watchdog runs this.",
        ]
        for arguments in Self.restoreArguments(saved) + [["delete", memoryDomain, Self.savedKey]] {
            lines.append(([tools.defaults] + arguments).map(Self.quoted).joined(separator: " ") + " 2>/dev/null")
        }
        lines.append("/bin/rm -f \"$0\"")
        lines.append("\(Self.quoted(tools.killall)) Dock")
        return lines.joined(separator: "\n") + "\n"
    }

    private func writeRestoreScript(_ saved: [String: [String]]) {
        guard let restoreScript else { return }
        try? FileManager.default.createDirectory(at: restoreScript.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? Data(restoreScriptText(saved).utf8).write(to: restoreScript, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: restoreScript.path)
    }

    static func quoted(_ text: String) -> String {
        "'" + text.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    private func read(_ key: String) -> (type: String, text: String)? {
        let type = commands.run(tools.defaults, ["read-type", Self.domain, key])
        let value = commands.run(tools.defaults, ["read", Self.domain, key])
        guard type.status == 0, value.status == 0 else { return nil }
        // "Type is boolean"
        let name = type.output.trimmingCharacters(in: .whitespacesAndNewlines).components(separatedBy: " ").last ?? ""
        return (name, value.output.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private static func flag(type: String) -> String? {
        ["boolean": "-bool", "float": "-float", "integer": "-int", "string": "-string"][type]
    }

    /// `defaults read` prints booleans as 1 and 0.
    private static func argument(type: String, text: String) -> String {
        guard type == "boolean" else { return text }
        return text == "1" || text.lowercased() == "true" ? "true" : "false"
    }

    private func defaults(_ arguments: [String]) { commands.run(tools.defaults, arguments) }

    /// The Dock only reads its settings at launch; macOS relaunches it at once.
    private func restartDock() { commands.run(tools.killall, ["Dock"]) }
}

/// A per-user LaunchAgent that brings the macOS Dock back when Kairos has stopped (a crash, a
/// force quit) while hiding it. Every 15 seconds it runs `SystemDock`'s restore script if the
/// script is there and no process called `processName` is running for this user.
public struct DockWatchdog {
    public static let label = "com.kairos.dock-watchdog"
    public static let interval = 15

    private let agents: URL
    private let restoreScript: URL
    private let processName: String
    private let uid: uid_t
    private let commands: CommandRunner

    /// `agents` is ~/Library/LaunchAgents for real.
    public init(agents: URL, restoreScript: URL, processName: String = "Kairos", uid: uid_t = getuid(),
                commands: CommandRunner = ProcessRunner()) {
        self.agents = agents
        self.restoreScript = restoreScript
        self.processName = processName
        self.uid = uid
        self.commands = commands
    }

    public var plistURL: URL { agents.appendingPathComponent("\(Self.label).plist") }
    public var isInstalled: Bool { FileManager.default.fileExists(atPath: plistURL.path) }

    /// The check the agent runs. `$1` is the restore script.
    public static func check(processName: String) -> String {
        "[ -f \"$1\" ] || exit 0; /usr/bin/pgrep -xq -U \"$(/usr/bin/id -u)\" \(SystemDock.quoted(processName)) && exit 0; exec /bin/sh \"$1\""
    }

    /// The command line the agent runs, also used by the tests.
    public var arguments: [String] { ["/bin/sh", "-c", Self.check(processName: processName), "kairos-dock-watchdog", restoreScript.path] }

    public var plist: [String: Any] {
        ["Label": Self.label, "ProgramArguments": arguments, "StartInterval": Self.interval, "RunAtLoad": false,
         "ProcessType": "Background"]
    }

    /// Writes the agent and (re)loads it for this login session.
    public func install() {
        try? FileManager.default.createDirectory(at: agents, withIntermediateDirectories: true)
        guard let data = try? PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0),
              (try? data.write(to: plistURL, options: .atomic)) != nil else { return }
        commands.run("/bin/launchctl", ["bootout", "gui/\(uid)/\(Self.label)"])
        commands.run("/bin/launchctl", ["bootstrap", "gui/\(uid)", plistURL.path])
    }

    public func remove() {
        guard isInstalled else { return }
        commands.run("/bin/launchctl", ["bootout", "gui/\(uid)/\(Self.label)"])
        try? FileManager.default.removeItem(at: plistURL)
    }
}

extension Settings {
    /// The macOS Dock stays hidden only while Kairos Bar is on screen to take its place.
    public var hidesSystemDock: Bool { replacesSystemDock && dockVisible }

    /// "Use Kairos Bar instead of the macOS Dock". Turning it on also turns on auto-hide, so the
    /// bar keeps out of the way the way the Dock did; the owner can turn that back off.
    public mutating func setReplacesSystemDock(_ on: Bool) {
        if on && !replacesSystemDock { autoHideBar = true }
        replacesSystemDock = on
    }
}
