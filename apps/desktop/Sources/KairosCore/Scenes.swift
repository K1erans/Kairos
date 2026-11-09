import Foundation

public enum ClaudeError: LocalizedError {
    case noKey, http(Int, String), refused(String), truncated, empty
    public var errorDescription: String? {
        switch self {
        case .noKey: return "No Anthropic API key is set. Add one in Kairos Desktop > Settings."
        case let .http(code, body): return "Claude returned HTTP \(code): \(body.prefix(300))"
        case let .refused(why): return "Claude declined this request\(why.isEmpty ? "" : " (\(why))")."
        case .truncated: return "The scene was too long and got cut off. Try a simpler subject."
        case .empty: return "Claude returned nothing usable."
        }
    }
}

public struct SceneCandidate: Codable, Equatable, Sendable {
    public var name: String
    public var kind: String
    public var why: String
}

/// Writes 3D scenes for Kairos Read by calling the Claude Messages API over raw HTTP
/// (there is no official Swift SDK). Requests stream so long scenes cannot time out.
public final class SceneWriter: @unchecked Sendable {
    public static let endpoint = URL(string: "https://api.anthropic.com/v1/messages")!
    public static let model = "claude-opus-5-5"
    public static let threeVersion = "0.170.0"

    private let session: URLSession

    public init(session: URLSession = .shared) { self.session = session }

    public static let importMap = """
    <script type="importmap">
    {"imports":{"three":"https://unpkg.com/three@\(threeVersion)/build/three.module.js","three/addons/":"https://unpkg.com/three@\(threeVersion)/examples/jsm/"}}
    </script>
    """

    public static let sceneSystem = """
    You write interactive 3D scenes that help a reader understand what an article describes. The \
    reference for quality is a physically simulated record player in the browser: the behaviour \
    comes from simulating the real thing, not from playing back a canned animation.

    Return one complete, standalone HTML document and nothing else: no commentary and no Markdown \
    fences. It runs inside a narrow browser side panel (about 400px wide, any height) and is also \
    saved as a file people open directly, so it must work with no build step and no network \
    requests other than three.js.

    Technical contract:
    - Load three.js only through this import map, copied exactly, placed before your module script:
    \(importMap)
    - Import only `three` and, for camera control, `three/addons/controls/OrbitControls.js`. No \
    other libraries, no external textures, models, fonts or images. Build geometry in code.
    - Fill the window, handle resize, and cap devicePixelRatio at 2.
    - Give the important parts short visible labels (HTML overlays positioned from projected \
    coordinates work well) so the reader learns the names.
    - When the article explains how something works, simulate it: step the state each frame from \
    the quantities that matter and expose two to four of them as labelled sliders. When the \
    subject makes sound and it helps understanding, synthesise it with Web Audio, started only \
    after a click on a clearly labelled button.
    - Narration sync: listen for `window.addEventListener("message", ...)` events whose \
    `event.data` is `{type: "kairos:sentence", text}`. When the sentence mentions one of your \
    parts (match on a few keywords per part), highlight that part and ease the camera toward it; \
    clear the highlight when a later sentence mentions none.
    - Use a dark neutral background, readable 13px system-font UI, and keep controls in a compact \
    panel that does not cover the model.
    - Be accurate to the article. Where the article does not give a number, use a physically \
    plausible one rather than inventing a precise-looking figure in a label.
    """

    public static let candidateSystem = """
    You pick what in an article is worth showing as an interactive 3D scene. Choose up to three \
    things the article actually describes that a reader would understand better by seeing and \
    manipulating them: a physical object, a mechanism, or a process. Skip abstract ideas, people \
    and anything the article only mentions in passing. If nothing qualifies, return an empty array.

    Reply with only a JSON array, no prose and no code fences. Each item is an object with \
    "name" (short noun phrase), "kind" (one of "object", "mechanism", "process") and "why" (one \
    sentence on what the scene would show).
    """

    public static func requestBody(system: String, user: String, maxTokens: Int, effort: String) -> [String: Any] {
        [
            "model": model,
            "max_tokens": maxTokens,
            "stream": true,
            "thinking": ["type": "adaptive"],
            "output_config": ["effort": effort],
            // If a safety classifier declines, the API retries on Anthropic's recommended model.
            "fallbacks": "default",
            "system": system,
            "messages": [["role": "user", "content": user]],
        ]
    }

    /// Runs one streamed request and returns the concatenated text.
    func run(apiKey: String?, body: [String: Any]) async throws -> String {
        guard let apiKey, !apiKey.isEmpty else { throw ClaudeError.noKey }
        var req = URLRequest(url: Self.endpoint)
        req.httpMethod = "POST"
        req.timeoutInterval = 600
        req.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        req.setValue("2026-10-21", forHTTPHeaderField: "anthropic-version")
        req.setValue("server-side-fallback-2026-07-01", forHTTPHeaderField: "anthropic-beta")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)

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

    /// Keeps the article within a sensible prompt size without silently dropping the middle.
    static func clip(_ text: String, limit: Int = 60_000) -> String {
        guard text.count > limit else { return text }
        return String(text.prefix(limit)) + "\n\n[The article continues beyond this point; it was cut at \(limit) characters.]"
    }

    public static func parseCandidates(_ raw: String) -> [SceneCandidate] {
        guard let start = raw.firstIndex(of: "["), let end = raw.lastIndex(of: "]"), start < end,
              let list = try? JSONDecoder().decode([SceneCandidate].self, from: Data(raw[start...end].utf8)) else { return [] }
        return Array(list.prefix(3))
    }

    /// Strips Markdown fences or stray prose around the document, if the model added any.
    public static func extractHTML(_ raw: String) throws -> String {
        var s = raw
        if let fence = s.range(of: "```") {
            let after = s[fence.upperBound...]
            if let nl = after.firstIndex(of: "\n") { s = String(after[after.index(after: nl)...]) }
            if let close = s.range(of: "```", options: .backwards) { s = String(s[..<close.lowerBound]) }
        }
        let lower = s.lowercased()
        guard let start = lower.range(of: "<!doctype html") ?? lower.range(of: "<html"),
              let end = lower.range(of: "</html>", options: .backwards) else { throw ClaudeError.empty }
        return String(s[start.lowerBound..<end.upperBound])
    }

    public func candidates(apiKey: String?, title: String, text: String) async throws -> [SceneCandidate] {
        let user = "Article title: \(title)\n\n<article>\n\(Self.clip(text))\n</article>"
        let raw = try await run(apiKey: apiKey, body: Self.requestBody(system: Self.candidateSystem, user: user, maxTokens: 4000, effort: "low"))
        return Self.parseCandidates(raw)
    }

    public func generate(apiKey: String?, title: String, text: String, subject: String) async throws -> String {
        let user = """
        Build a scene of: \(subject)

        It should show what this article says about it, so someone listening to the article can \
        follow along.

        Article title: \(title)

        <article>
        \(Self.clip(text))
        </article>
        """
        let raw = try await run(apiKey: apiKey, body: Self.requestBody(system: Self.sceneSystem, user: user, maxTokens: 64000, effort: "high"))
        return try Self.extractHTML(raw)
    }

    public func edit(apiKey: String?, html: String, instruction: String) async throws -> String {
        let user = """
        Here is a scene you wrote earlier. Change it as asked and return the complete updated \
        HTML document, keeping everything that was not asked to change.

        Requested change: \(instruction)

        <scene>
        \(html)
        </scene>
        """
        let raw = try await run(apiKey: apiKey, body: Self.requestBody(system: Self.sceneSystem, user: user, maxTokens: 64000, effort: "high"))
        return try Self.extractHTML(raw)
    }
}
