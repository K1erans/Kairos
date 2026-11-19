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
/// It also turns on "static-only", so the Dock shows only running apps. Mission Control (F3)
/// always shows the Dock, auto-hide or not, and this keeps it to a short strip there.
///
/// While the Dock is hidden a shell script that does the same as `restore()` sits at
/// `restoreScript`, so `DockWatchdog` can bring the Dock back if Kairos stops without doing it.
public struct SystemDock {
    public enum State: Equatable, Sendable {
        case restored
        case replaced
        case restoring
    }

    public static let domain = "com.apple.dock"
    static let keys = ["autohide", "autohide-delay", "static-only"]
    /// Keys added after the first release. Older saved values have no entry for them.
    static let laterKeys = ["static-only"]
    static let savedKey = "systemDockSaved"
    static let restoredKey = "systemDockRestored"
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

    /// True while Kairos has the Dock hidden (or partially restored) and holds the user's values to put back.
    public var isReplaced: Bool { memory.dictionary(forKey: Self.savedKey) != nil }

    /// Explicit Dock replacement lifecycle state.
    public var state: State {
        guard saved != nil else { return .restored }
        return restoredKeys.isEmpty ? .replaced : .restoring
    }

    /// True while recovery preferences or a restore script still exist and must remain covered by the watchdog.
    public var hasRecoveryData: Bool {
        isReplaced || (restoreScript.map { FileManager.default.fileExists(atPath: $0.path) } ?? false)
    }

    private var saved: [String: [String]]? { memory.dictionary(forKey: Self.savedKey) as? [String: [String]] }
    private var restoredKeys: Set<String> { Set(memory.stringArray(forKey: Self.restoredKey) ?? []) }

    public func replace() {
        if var saved {
            // Hidden by an older build that did not know a later key: record the owner's value
            // for it (that build never changed it) and apply the hidden setting. Only those keys:
            // older builds left out a key the owner had never set, so a missing original key
            // means "not set", and reading it now would record Kairos's own value.
            let wasRestoring = !restoredKeys.isEmpty
            let missing = Self.laterKeys.filter { saved[$0] == nil }
            for key in missing { saved[key] = read(key).map { [$0.type, $0.text] } ?? [] }
            if !missing.isEmpty { memory.set(saved, forKey: Self.savedKey) }
            if wasRestoring { memory.removeObject(forKey: Self.restoredKey) }
            // Hidden by a build that wrote no script, recorded fewer keys, or was mid-restore: write it now.
            writeRestoreScript(saved)
            let toHide = wasRestoring ? Self.keys : missing
            guard !toHide.isEmpty else { return }
            for key in toHide { _ = defaults(Self.hiddenArguments(key)) }
            restartDock()
            return
        }
        // Nothing remembered, yet a restore script is still there: the Dock is hidden from a
        // run whose memory was lost. Put it back first, so what is read below is the owner's.
        // If recovery fails, keep the existing script intact instead of overwriting it.
        guard recover() else { return }
        // key -> [type, value] as `defaults` reports them; a key the user never set is [].
        var values: [String: [String]] = [:]
        for key in Self.keys { values[key] = read(key).map { [$0.type, $0.text] } ?? [] }
        // Kairos's own hidden setting is never recorded as the owner's: restoring it would leave
        // the Mac with no Dock. With nothing better known, restore will clear both keys, which
        // is the macOS default of a Dock that stays on screen.
        if let delay = values["autohide-delay"], delay.count == 2, Double(delay[1]) == Double(Self.hiddenDelay) {
            values = Dictionary(uniqueKeysWithValues: Self.keys.map { ($0, [String]()) })
        }
        memory.set(values, forKey: Self.savedKey)
        memory.removeObject(forKey: Self.restoredKey)
        // Before the Dock goes, so there is never a hidden Dock without a way back.
        writeRestoreScript(values)
        for key in Self.keys { _ = defaults(Self.hiddenArguments(key)) }
        restartDock()
    }

    /// The `defaults` call that sets `key` the way Kairos wants it while the Dock is hidden.
    static func hiddenArguments(_ key: String) -> [String] {
        switch key {
        case "autohide-delay": return ["write", domain, key, "-float", hiddenDelay]
        default: return ["write", domain, key, "-bool", "true"]   // autohide, static-only
        }
    }

    public func restore() {
        guard let saved else { _ = recover(); return }
        var restored = restoredKeys
        let pending = Self.keys.filter { !restored.contains($0) }
        var changed = false
        for key in pending {
            if defaults(Self.restoreArguments(for: key, saved: saved)).status == 0 {
                restored.insert(key)
                changed = true
            }
        }
        let remaining = Self.keys.filter { !restored.contains($0) }
        if remaining.isEmpty {
            memory.removeObject(forKey: Self.savedKey)
            memory.removeObject(forKey: Self.restoredKey)
            if let restoreScript { try? FileManager.default.removeItem(at: restoreScript) }
            restartDock()
        } else {
            if !restored.isEmpty {
                memory.set(Self.keys.filter { restored.contains($0) }, forKey: Self.restoredKey)
            }
            if changed {
                writeRestoreScript(saved, keys: remaining)
                restartDock()
            }
        }
    }

    /// With nothing remembered but a restore script left on disk, runs it: it holds the
    /// owner's values, puts them back, clears the memory and removes itself.
    /// Returns false when a leftover script fails to run, leaving it on disk to retry.
    @discardableResult
    private func recover() -> Bool {
        guard saved == nil, let restoreScript, FileManager.default.fileExists(atPath: restoreScript.path) else { return true }
        let result = commands.run("/bin/sh", [restoreScript.path])
        guard result.status == 0 else { return false }
        memory.removeObject(forKey: Self.restoredKey)
        try? FileManager.default.removeItem(at: restoreScript)
        return true
    }

    /// The `defaults` calls that put back what `saved` holds, deleting what the user never set.
    static func restoreArguments(_ saved: [String: [String]]) -> [[String]] {
        keys.map { restoreArguments(for: $0, saved: saved) }
    }

    static func restoreArguments(for key: String, saved: [String: [String]]) -> [String] {
        if let value = saved[key], value.count == 2, let flag = flag(type: value[0]) {
            return ["write", domain, key, flag, argument(type: value[0], text: value[1])]
        }
        return ["delete", domain, key]
    }

    /// `restore()` as a shell script that removes itself only once every preference has been restored.
    func restoreScriptText(_ saved: [String: [String]], keys: [String] = SystemDock.keys) -> String {
        var lines = [
            "#!/bin/sh",
            "# Written by Kairos while it hides the macOS Dock, and removed when Kairos puts the Dock back.",
            "# If Kairos stops without doing that, its Dock watchdog runs this.",
            "ok=0",
        ]
        for key in keys {
            let arguments = Self.restoreArguments(for: key, saved: saved)
            let cmd = ([tools.defaults] + arguments).map(Self.quoted).joined(separator: " ")
            if arguments.first == "delete" {
                lines.append("\(cmd) 2>/dev/null || { err=$(\(cmd) 2>&1); case \"$err\" in *\"Could not find key\"*|*\"not found\"*|*\"does not exist\"*) ;; *) ok=1 ;; esac; }")
            } else {
                lines.append("\(cmd) 2>/dev/null || ok=1")
            }
        }
        let clearSaved = ([tools.defaults, "delete", memoryDomain, Self.savedKey]).map(Self.quoted).joined(separator: " ")
        lines.append("if [ \"$ok\" -eq 0 ]; then")
        lines.append("  \(clearSaved) 2>/dev/null")
        lines.append("  /bin/rm -f \"$0\"")
        lines.append("fi")
        lines.append("\(Self.quoted(tools.killall)) Dock")
        lines.append("exit \"$ok\"")
        return lines.joined(separator: "\n") + "\n"
    }

    private func writeRestoreScript(_ saved: [String: [String]], keys: [String] = SystemDock.keys) {
        guard let restoreScript else { return }
        try? FileManager.default.createDirectory(at: restoreScript.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? Data(restoreScriptText(saved, keys: keys).utf8).write(to: restoreScript, options: .atomic)
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

    @discardableResult
    private func defaults(_ arguments: [String]) -> (status: Int32, output: String) {
        commands.run(tools.defaults, arguments)
    }

    /// The Dock only reads its settings at launch; macOS relaunches it at once.
    private func restartDock() { _ = commands.run(tools.killall, ["Dock"]) }
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
