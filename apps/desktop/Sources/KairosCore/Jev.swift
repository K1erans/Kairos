import Foundation

public struct JevVerdict: Equatable, Sendable {
    public var choice: String
    public var confidence: Double
    public var probabilities: [String: Double]
    public var cached = false

    public init(choice: String, confidence: Double, probabilities: [String: Double], cached: Bool = false) {
        self.choice = choice; self.confidence = confidence; self.probabilities = probabilities; self.cached = cached
    }

    public static let unsure = JevVerdict(choice: "unsure", confidence: 0, probabilities: [:])

    /// Block when the probability of "distracting" reaches the group's threshold.
    public func shouldBlock(threshold: Double) -> Bool {
        (probabilities["distracting"] ?? (choice == "distracting" ? confidence : 0)) >= threshold
    }
}

public enum JevError: LocalizedError {
    case noKey, http(Int, String), badResponse
    public var errorDescription: String? {
        switch self {
        case .noKey: return "No TypeSafe API key is set."
        case let .http(code, body): return "Jev returned HTTP \(code): \(body.prefix(200))"
        case .badResponse: return "Jev returned an answer Kairos could not read."
        }
    }
}

/// Client for TypeSafe AI's Jev (POST /v1/systemone). One `choice` question per check.
public final class JevClient: @unchecked Sendable {
    public static let endpoint = URL(string: "https://api.typesafe.ai/v1/systemone")!
    private let session: URLSession
    private let lock = NSLock()
    private var cache: [String: (verdict: JevVerdict, at: Date)] = [:]
    public var cacheTTL: TimeInterval = 24 * 3600
    public var timeout: TimeInterval = 2

    public init(session: URLSession = .shared) { self.session = session }

    /// What is being judged: a web page or an app window.
    public struct Subject: Sendable {
        public var kind: String // "page" or "app"
        public var url: String = ""
        public var title: String = ""
        public var description: String = ""
        public var excerpt: String = ""
        public var app: String = ""
        public init(kind: String) { self.kind = kind }
        var cacheKey: String { kind == "page" ? url : "\(app)|\(title)" }
    }

    public static func requestBody(task: String, group: String, subject: Subject) -> [String: Any] {
        var state: [String: Any] = ["task": task, "group": group]
        if subject.kind == "page" {
            state["url"] = subject.url
            state["title"] = subject.title
            if !subject.description.isEmpty { state["description"] = subject.description }
            state["excerpt"] = subject.excerpt
        } else {
            state["app"] = subject.app
            if !subject.title.isEmpty { state["window_title"] = subject.title }
        }
        let noun = subject.kind == "page" ? "page" : "app window"
        return [
            "model": "jev-latest",
            "state": state,
            "questions": [
                "focus": [
                    "type": "choice",
                    "instructions": "Is this \(noun) useful for the user's current task?",
                    "criteria": [
                        "relevant": "Directly helps with the task, or is a tool the task needs",
                        "distracting": "Unrelated entertainment, social media, shopping or news",
                        "unsure": "Could be either",
                    ],
                ],
            ],
        ]
    }

    public static func parse(_ data: Data) throws -> JevVerdict {
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let answers = root["answers"] as? [String: Any],
              let focus = answers["focus"] as? [String: Any],
              let choice = focus["choice"] as? String else { throw JevError.badResponse }
        var probs: [String: Double] = [:]
        for (k, v) in (focus["probabilities"] as? [String: Any]) ?? [:] {
            if let n = v as? NSNumber { probs[k] = n.doubleValue }
        }
        let confidence = (focus["confidence"] as? NSNumber)?.doubleValue ?? probs[choice] ?? 0
        return JevVerdict(choice: choice, confidence: confidence, probabilities: probs)
    }

    public func check(apiKey: String?, task: String, group: String, groupId: String, subject: Subject, now: Date = Date()) async throws -> JevVerdict {
        guard let apiKey, !apiKey.isEmpty else { throw JevError.noKey }
        let key = "\(groupId)|\(task)|\(subject.cacheKey)"
        if var hit = cached(key, now: now) { hit.cached = true; return hit }

        var req = URLRequest(url: Self.endpoint)
        req.httpMethod = "POST"
        req.timeoutInterval = timeout
        req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: Self.requestBody(task: task, group: group, subject: subject))
        let (data, response) = try await session.data(for: req)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw JevError.http(status, String(decoding: data, as: UTF8.self)) }
        let verdict = try Self.parse(data)
        remember(key, verdict, now: now)
        return verdict
    }

    private func cached(_ key: String, now: Date) -> JevVerdict? {
        lock.lock(); defer { lock.unlock() }
        guard let hit = cache[key], now.timeIntervalSince(hit.at) < cacheTTL else { return nil }
        return hit.verdict
    }

    private func remember(_ key: String, _ verdict: JevVerdict, now: Date) {
        lock.lock(); defer { lock.unlock() }
        cache[key] = (verdict, now)
    }
}

/// Whether Jev can judge anything for the active group, and if not, why.
public enum JevReadiness: Equatable, Sendable {
    case off, ready, noKey, noTask
    case failing(String)

    public init(enabled: Bool, hasKey: Bool, hasTask: Bool, lastError: String?) {
        if !enabled { self = .off }
        else if !hasKey { self = .noKey }
        else if !hasTask { self = .noTask }
        else if let lastError { self = .failing(lastError) }
        else { self = .ready }
    }

    /// What to tell the owner, or nil when Jev is running or switched off.
    public var problem: String? {
        switch self {
        case .off, .ready: return nil
        case .noKey: return "Jev is on but has no TypeSafe API key, so only your block lists apply. Add the key in Settings."
        case .noTask: return "Jev is on but there is no current task to compare with, so it is not judging anything. Start a task."
        case let .failing(error): return "Jev could not answer the last check: \(error)"
        }
    }
}
