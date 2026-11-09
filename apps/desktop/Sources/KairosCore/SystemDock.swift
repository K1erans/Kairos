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
public struct SystemDock {
    public static let domain = "com.apple.dock"
    static let keys = ["autohide", "autohide-delay"]
    static let savedKey = "systemDockSaved"
    static let hiddenDelay = "1000"

    private let commands: CommandRunner
    private let memory: UserDefaults

    public init(commands: CommandRunner = ProcessRunner(), memory: UserDefaults = .standard) {
        self.commands = commands
        self.memory = memory
    }

    /// True while Kairos has the Dock hidden and holds the user's values to put back.
    public var isReplaced: Bool { memory.dictionary(forKey: Self.savedKey) != nil }

    public func replace() {
        guard !isReplaced else { return }
        // key -> [type, value] as `defaults` reports them; a key the user never set is left out.
        var saved: [String: [String]] = [:]
        for key in Self.keys {
            if let value = read(key) { saved[key] = [value.type, value.text] }
        }
        memory.set(saved, forKey: Self.savedKey)
        defaults(["write", Self.domain, "autohide", "-bool", "true"])
        defaults(["write", Self.domain, "autohide-delay", "-float", Self.hiddenDelay])
        restartDock()
    }

    public func restore() {
        guard let saved = memory.dictionary(forKey: Self.savedKey) as? [String: [String]] else { return }
        for key in Self.keys {
            if let value = saved[key], value.count == 2, let flag = Self.flag(type: value[0]) {
                defaults(["write", Self.domain, key, flag, Self.argument(type: value[0], text: value[1])])
            } else {
                defaults(["delete", Self.domain, key])
            }
        }
        memory.removeObject(forKey: Self.savedKey)
        restartDock()
    }

    private func read(_ key: String) -> (type: String, text: String)? {
        let type = commands.run("/usr/bin/defaults", ["read-type", Self.domain, key])
        let value = commands.run("/usr/bin/defaults", ["read", Self.domain, key])
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

    private func defaults(_ arguments: [String]) { commands.run("/usr/bin/defaults", arguments) }

    /// The Dock only reads its settings at launch; macOS relaunches it at once.
    private func restartDock() { commands.run("/usr/bin/killall", ["Dock"]) }
}

extension Settings {
    /// The macOS Dock stays hidden only while Kairos Bar is on screen to take its place.
    public var hidesSystemDock: Bool { replacesSystemDock && dockVisible }
}
