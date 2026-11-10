import Foundation

/// Finds `node` for the scene helper. An app opened from Finder gets a bare PATH
/// (/usr/bin:/bin:/usr/sbin:/sbin), so the usual places are tried by name first, then the login
/// shell is asked, the way Terminal would find it.
public enum NodeLocator {
    /// The places tried before asking the login shell, in order: Homebrew on Apple silicon, Homebrew
    /// on Intel and the nodejs.org installer, Volta, then nvm's installed versions, newest first.
    public static func candidates(home: String, nvmVersions: [String]) -> [String] {
        ["/opt/homebrew/bin/node", "/usr/local/bin/node", "\(home)/.volta/bin/node"]
            + nvmVersions.sorted { version($0).lexicographicallyPrecedes(version($1)) }.reversed()
                .map { "\(home)/.nvm/versions/node/\($0)/bin/node" }
    }

    /// "v22.11.0" -> [22, 11, 0]
    static func version(_ name: String) -> [Int] {
        name.drop { !$0.isNumber }.split(separator: ".").map { Int($0.prefix { $0.isNumber }) ?? 0 }
    }

    /// The first candidate that is executable, else the login shell's answer when it is.
    public static func find(home: String, nvmVersions: [String], isExecutable: (String) -> Bool,
                            loginShell: () -> String?) -> String? {
        if let found = candidates(home: home, nvmVersions: nvmVersions).first(where: isExecutable) { return found }
        guard let answer = loginShell() else { return nil }
        // The last absolute path printed: a shell's startup files may print other things first.
        let path = answer.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
            .last { $0.hasPrefix("/") }
        return path.flatMap { isExecutable($0) ? $0 : nil }
    }

    private static let cache = Cache()

    /// `find` on this Mac. A path found is kept for the rest of the run; not finding one is not,
    /// so installing Node while Kairos is open works without a relaunch.
    public static func locate() -> URL? {
        if let known = cache.value { return known }
        let fm = FileManager.default
        let home = fm.homeDirectoryForCurrentUser.path
        let nvm = (try? fm.contentsOfDirectory(atPath: "\(home)/.nvm/versions/node")) ?? []
        let found = find(home: home, nvmVersions: nvm, isExecutable: fm.isExecutableFile(atPath:), loginShell: askLoginShell)
            .map { URL(fileURLWithPath: $0) }
        cache.value = found
        return found
    }

    /// `command -v node` in the owner's login shell (zsh, bash and fish all understand it), given
    /// three seconds.
    static func askLoginShell() -> String? {
        guard let entry = getpwuid(getuid()), let shellPath = entry.pointee.pw_shell else { return nil }
        let shell = String(cString: shellPath)
        guard !shell.isEmpty else { return nil }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: shell)
        process.arguments = ["-l", "-c", "command -v node"]
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        process.standardInput = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return nil }
        let deadline = Date().addingTimeInterval(3)
        while process.isRunning && Date() < deadline { usleep(20_000) }
        if process.isRunning { process.terminate(); return nil }
        return String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    }

    private final class Cache: @unchecked Sendable {
        private let lock = NSLock()
        private var url: URL?
        var value: URL? {
            get { lock.lock(); defer { lock.unlock() }; return url }
            set { lock.lock(); defer { lock.unlock() }; if let newValue { url = newValue } }
        }
    }
}

/// The scene helper, `scene-agent.mjs` with its node_modules, which scripts/build-app.sh puts in
/// Kairos.app/Contents/Resources/scene-agent. `KAIROS_SCENE_AGENT` points elsewhere (a
/// development checkout, after `npm ci` in apps/desktop/scene-agent, or a test's fake helper).
public enum SceneHelper {
    public static func url(resources: URL?, environment: [String: String]) -> URL? {
        if let path = environment["KAIROS_SCENE_AGENT"], !path.isEmpty { return URL(fileURLWithPath: path) }
        return resources?.appendingPathComponent("scene-agent/scene-agent.mjs")
    }

    /// The helper is there with its dependencies installed.
    public static func isInstalled(_ url: URL?) -> Bool {
        guard let url else { return false }
        let fm = FileManager.default
        return fm.fileExists(atPath: url.path)
            && fm.fileExists(atPath: url.deletingLastPathComponent().appendingPathComponent("node_modules/@anthropic-ai/claude-agent-sdk").path)
    }

    public static var current: URL? {
        url(resources: Bundle.main.resourceURL, environment: ProcessInfo.processInfo.environment)
    }
}

/// What the Claude subscription backend needs on this Mac, and the line Settings shows for it.
public struct SceneSetup: Equatable, Sendable {
    public var node: URL?
    public var helper: URL?
    public var helperInstalled: Bool

    public init(node: URL? = nil, helper: URL? = nil) {
        self.node = node
        self.helper = helper
        helperInstalled = SceneHelper.isInstalled(helper)
    }

    /// Settings > 3D scenes: whether scenes can be built with the chosen backend, and why not.
    public func status(_ backend: SceneBackend, hasClaudeKey: Bool) -> (ready: Bool, text: String) {
        switch backend {
        case .apiKey:
            return hasClaudeKey
                ? (true, "Scenes use the Anthropic key under API keys, billed to that API account.")
                : (false, "Add an Anthropic key under API keys, or choose \u{201C}Claude subscription\u{201D}.")
        case .subscription:
            guard let node else {
                return (false, "Node.js wasn\u{2019}t found. Install it (brew install node), then come back to this page.")
            }
            guard helperInstalled else {
                return (false, "This copy of Kairos has no scene helper. Rebuild it with scripts/build-app.sh.")
            }
            return (true, "Ready: scenes go through Claude Code on your Claude subscription, with Node.js at \(node.path). If Claude Code isn\u{2019}t logged in yet, run claude in Terminal, then /login.")
        }
    }
}
