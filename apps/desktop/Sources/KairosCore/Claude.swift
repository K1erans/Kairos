import Foundation

public enum ClaudeError: LocalizedError, Equatable {
    case noKey, http(Int, String), refused(String), truncated, empty
    case noNode, noHelper, notLoggedIn, helper(String), cancelled

    public var errorDescription: String? {
        switch self {
        case .noKey: return "No Anthropic API key is set. Add one in Kairos Desktop > Settings, or choose \u{201C}Claude subscription\u{201D} under 3D scenes."
        case let .http(code, body): return "Claude returned HTTP \(code): \(body.prefix(300))"
        case let .refused(why): return "Claude declined this request\(why.isEmpty ? "" : " (\(why))")."
        case .truncated: return "The scene was too long and got cut off. Try a simpler subject."
        case .empty: return "Claude returned nothing usable."
        case .noNode:
            return "Kairos couldn\u{2019}t find Node.js, which it needs to write scenes with your Claude subscription. Install it (for example with \u{201C}brew install node\u{201D}), or choose \u{201C}Anthropic API key\u{201D} in Kairos Desktop > Settings > 3D scenes."
        case .noHelper:
            return "Kairos.app is missing its scene helper. Rebuild Kairos with scripts/build-app.sh, or choose \u{201C}Anthropic API key\u{201D} in Kairos Desktop > Settings > 3D scenes."
        case .notLoggedIn:
            return "Claude Code isn\u{2019}t logged in on this Mac. Open Terminal, run \u{201C}claude\u{201D}, type /login, then try again."
        case let .helper(line): return "Claude couldn\u{2019}t write the scene: \(line)"
        case .cancelled: return "The scene build was cancelled."
        }
    }
}

/// One request to Claude, the same whichever way it is sent.
public struct ClaudePrompt: Equatable, Sendable {
    public var system: String
    public var user: String
    /// "low", "medium" or "high".
    public var effort: String
    /// The API's output limit. The subscription helper leaves this to the Agent SDK.
    public var maxTokens: Int

    public init(system: String, user: String, effort: String, maxTokens: Int) {
        self.system = system
        self.user = user
        self.effort = effort
        self.maxTokens = maxTokens
    }
}

/// Sends a prompt to Claude and returns its answer's text. Cancelling the calling task stops the
/// request (the helper process is terminated, the HTTP request dropped) and throws `.cancelled`.
public protocol ClaudeBackend: Sendable {
    func complete(_ prompt: ClaudePrompt) async throws -> String
}

public enum Claude {
    public static let model = "claude-opus-5-5"

    /// Whatever a cancelled request threw becomes `.cancelled`, so the reply says so.
    static func cancelling<T>(_ work: () async throws -> T) async throws -> T {
        do {
            let value = try await work()
            try Task.checkCancellation()
            return value
        } catch let error as URLError where error.code == .cancelled {
            throw ClaudeError.cancelled
        } catch is CancellationError {
            throw ClaudeError.cancelled
        } catch {
            if Task.isCancelled { throw ClaudeError.cancelled }
            throw error
        }
    }
}

// MARK: - Claude subscription

/// Runs the Claude Agent SDK helper (`node scene-agent.mjs`), so requests are billed to the
/// Claude subscription this Mac's Claude Code is logged in to. The prompt goes to the helper's
/// stdin as {"system","user","effort","model"}; the answer comes back on stdout; a failure is one
/// line on stderr and a non-zero exit.
public struct ClaudeSubscription: ClaudeBackend {
    public let node: URL?
    public let helper: URL?
    public var model = Claude.model
    /// The environment the helper starts with; `node`'s folder is put first on its PATH.
    public var environment: [String: String]

    public init(node: URL?, helper: URL?, environment: [String: String] = ProcessInfo.processInfo.environment) {
        self.node = node
        self.helper = helper
        self.environment = environment
    }

    public static func input(_ prompt: ClaudePrompt, model: String) throws -> Data {
        try JSONSerialization.data(withJSONObject: ["system": prompt.system, "user": prompt.user, "effort": prompt.effort, "model": model],
                                   options: .sortedKeys)
    }

    /// What a failed helper run means, from its stderr. The helper prints one line; Node itself
    /// prints a stack ending in its version when it cannot start the helper at all.
    public static func failure(stderr: String) -> ClaudeError {
        if stderr.contains("ERR_MODULE_NOT_FOUND") || stderr.contains("Cannot find package") { return .noHelper }
        let line = stderr.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
            .last { !$0.isEmpty && !$0.hasPrefix("at ") && !$0.hasPrefix("Node.js v") } ?? ""
        let lower = line.lowercased()
        let loginHints = ["/login", "not logged in", "log in", "authentication", "invalid api key", "oauth", "401"]
        if loginHints.contains(where: lower.contains) { return .notLoggedIn }
        return .helper(line.isEmpty ? "the scene helper stopped without saying why." : String(line.prefix(300)))
    }

    public func complete(_ prompt: ClaudePrompt) async throws -> String {
        guard let node else { throw ClaudeError.noNode }
        guard let helper, FileManager.default.fileExists(atPath: helper.path) else { throw ClaudeError.noHelper }
        var env = environment
        // An API key here would be billed instead of the subscription; the helper drops it too.
        env["ANTHROPIC_API_KEY"] = nil
        env["ANTHROPIC_AUTH_TOKEN"] = nil
        env["PATH"] = ([node.deletingLastPathComponent().path] + (env["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin").split(separator: ":").map(String.init))
            .joined(separator: ":")
        let input = try Self.input(prompt, model: model)
        let output = try await Claude.cancelling {
            try await Subprocess.run(node, [helper.path], directory: helper.deletingLastPathComponent(), environment: env, input: input)
        }
        guard output.status == 0 else { throw Self.failure(stderr: String(decoding: output.stderr, as: UTF8.self)) }
        let text = String(decoding: output.stdout, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { throw ClaudeError.empty }
        return text
    }
}

/// Runs a program to the end with `input` on its stdin, collecting stdout and stderr. Cancelling
/// the calling task terminates the program (SIGTERM).
public enum Subprocess {
    public struct Output: Sendable {
        public let status: Int32
        public let stdout: Data
        public let stderr: Data
    }

    private final class Run: @unchecked Sendable {
        let lock = NSLock()
        let process = Process()
        var stdout = Data(), stderr = Data()
        var cancelled = false

        func cancel() {
            lock.lock(); defer { lock.unlock() }
            cancelled = true
            if process.isRunning { process.terminate() }
        }
    }

    public static func run(_ program: URL, _ arguments: [String], directory: URL? = nil, environment: [String: String]? = nil,
                           input: Data) async throws -> Output {
        let run = Run()
        let process = run.process
        process.executableURL = program
        process.arguments = arguments
        if let directory { process.currentDirectoryURL = directory }
        if let environment { process.environment = environment }
        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr
        // A program that exits before reading its input must not take Kairos down with SIGPIPE.
        _ = fcntl(stdin.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1)

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Output, Error>) in
                let group = DispatchGroup()
                group.enter()
                process.terminationHandler = { _ in group.leave() }
                run.lock.lock()
                if run.cancelled {
                    run.lock.unlock()
                    done.resume(throwing: CancellationError())
                    return
                }
                do {
                    try process.run()
                } catch {
                    run.lock.unlock()
                    done.resume(throwing: error)
                    return
                }
                run.lock.unlock()
                // Only the child writes to these now; closing ours lets the reads below end.
                try? stdout.fileHandleForWriting.close()
                try? stderr.fileHandleForWriting.close()
                for (pipe, isOut) in [(stdout, true), (stderr, false)] {
                    group.enter()
                    DispatchQueue.global().async {
                        let data = pipe.fileHandleForReading.readDataToEndOfFile()
                        run.lock.lock()
                        if isOut { run.stdout = data } else { run.stderr = data }
                        run.lock.unlock()
                        group.leave()
                    }
                }
                DispatchQueue.global().async {
                    try? stdin.fileHandleForWriting.write(contentsOf: input)
                    try? stdin.fileHandleForWriting.close()
                }
                group.notify(queue: .global()) {
                    run.lock.lock(); defer { run.lock.unlock() }
                    if run.cancelled { return done.resume(throwing: CancellationError()) }
                    done.resume(returning: Output(status: process.terminationStatus, stdout: run.stdout, stderr: run.stderr))
                }
            }
        } onCancel: {
            run.cancel()
        }
    }
}

// MARK: - Anthropic API key

/// The Claude Messages API over raw HTTP (there is no official Swift SDK), billed to an Anthropic
/// API key. Requests stream so long scenes cannot time out.
public struct ClaudeAPI: ClaudeBackend {
    public static let endpoint = URL(string: "https://api.anthropic.com/v1/messages")!

    public let apiKey: String?
    private let session: URLSession

    public init(apiKey: String?, session: URLSession = .shared) {
        self.apiKey = apiKey
        self.session = session
    }

    public static func requestBody(_ prompt: ClaudePrompt) -> [String: Any] {
        [
            "model": Claude.model,
            "max_tokens": prompt.maxTokens,
            "stream": true,
            "thinking": ["type": "adaptive"],
            "output_config": ["effort": prompt.effort],
            // If a safety classifier declines, the API retries on Anthropic's recommended model.
            "fallbacks": "default",
            "system": prompt.system,
            "messages": [["role": "user", "content": prompt.user]],
        ]
    }

    public func complete(_ prompt: ClaudePrompt) async throws -> String {
        guard let apiKey, !apiKey.isEmpty else { throw ClaudeError.noKey }
        var req = URLRequest(url: Self.endpoint)
        req.httpMethod = "POST"
        req.timeoutInterval = 600
        req.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        req.setValue("2026-10-21", forHTTPHeaderField: "anthropic-version")
        req.setValue("server-side-fallback-2026-07-01", forHTTPHeaderField: "anthropic-beta")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: Self.requestBody(prompt))

        return try await Claude.cancelling {
            let (bytes, response) = try await session.bytes(for: req)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            guard status == 200 else {
                var text = ""
                for try await line in bytes.lines { text += line; if text.count > 2000 { break } }
                throw ClaudeError.http(status, text)
            }
            var parser = StreamParser()
            for try await line in bytes.lines { try parser.consume(line: line) }
            return try parser.result()
        }
    }

    /// Accumulates text from the Messages API's server-sent events.
    public struct StreamParser {
        public private(set) var text = ""
        public private(set) var stopReason: String?
        private var refusalCategory = ""

        public init() {}

        public mutating func consume(line: String) throws {
            guard line.hasPrefix("data:") else { return }
            let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
            guard let obj = try? JSONSerialization.jsonObject(with: Data(payload.utf8)) as? [String: Any],
                  let type = obj["type"] as? String else { return }
            switch type {
            case "content_block_delta":
                if let delta = obj["delta"] as? [String: Any], delta["type"] as? String == "text_delta",
                   let t = delta["text"] as? String { text += t }
            case "content_block_start":
                // A `fallback` block marks where another model took over. On a stream the partial
                // before it is kept and the fallback model continues it, so nothing is dropped here.
                // Only a final `refusal` stop (every model declined) discards the text, in result().
                if let block = obj["content_block"] as? [String: Any], block["type"] as? String == "text",
                   let t = block["text"] as? String { text += t }
            case "message_delta":
                if let delta = obj["delta"] as? [String: Any] {
                    if let r = delta["stop_reason"] as? String { stopReason = r }
                    if let d = delta["stop_details"] as? [String: Any], let c = d["category"] as? String { refusalCategory = c }
                }
            case "error":
                let message = (obj["error"] as? [String: Any])?["message"] as? String ?? "stream error"
                throw ClaudeError.http(0, message)
            default: break
            }
        }

        public func result() throws -> String {
            if stopReason == "refusal" { throw ClaudeError.refused(refusalCategory) }
            if stopReason == "max_tokens" { throw ClaudeError.truncated }
            let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if t.isEmpty { throw ClaudeError.empty }
            return t
        }
    }
}
