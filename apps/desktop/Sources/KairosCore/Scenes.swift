import Foundation

public struct SceneCandidate: Codable, Equatable, Sendable {
    public var name: String
    public var kind: String
    public var why: String
}

/// Writes 3D scenes for Kairos Read. The prompts are the same whichever backend sends them:
/// the Claude subscription (the Agent SDK helper) or an Anthropic API key (the Messages API).
public struct SceneWriter: Sendable {
    public static let threeVersion = "0.170.0"

    public let backend: ClaudeBackend

    public init(backend: ClaudeBackend) { self.backend = backend }

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

    public func candidates(title: String, text: String) async throws -> [SceneCandidate] {
        let user = "Article title: \(title)\n\n<article>\n\(Self.clip(text))\n</article>"
        let raw = try await backend.complete(ClaudePrompt(system: Self.candidateSystem, user: user, effort: "low", maxTokens: 4000))
        return Self.parseCandidates(raw)
    }

    public static func generatePrompt(title: String, text: String, subject: String) -> ClaudePrompt {
        let user = """
        Build a scene of: \(subject)

        It should show what this article says about it, so someone listening to the article can \
        follow along.

        Article title: \(title)

        <article>
        \(clip(text))
        </article>
        """
        return ClaudePrompt(system: sceneSystem, user: user, effort: "high", maxTokens: 64000)
    }

    public func generate(title: String, text: String, subject: String) async throws -> String {
        try Self.extractHTML(try await backend.complete(Self.generatePrompt(title: title, text: text, subject: subject)))
    }

    public func edit(html: String, instruction: String) async throws -> String {
        let user = """
        Here is a scene you wrote earlier. Change it as asked and return the complete updated \
        HTML document, keeping everything that was not asked to change.

        Requested change: \(instruction)

        <scene>
        \(html)
        </scene>
        """
        let raw = try await backend.complete(ClaudePrompt(system: Self.sceneSystem, user: user, effort: "high", maxTokens: 64000))
        return try Self.extractHTML(raw)
    }
}
