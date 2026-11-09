import Foundation

public struct CloudVoice: Identifiable, Equatable, Sendable {
    public var id: String
    public var name: String
    public init(id: String, name: String) { self.id = id; self.name = name }
}

public enum VoiceError: LocalizedError {
    case noKey, http(Int, String), empty
    public var errorDescription: String? {
        switch self {
        case .noKey: return "No API key is set for the chosen voice provider."
        case let .http(code, body): return "The voice service returned HTTP \(code): \(body.prefix(200))"
        case .empty: return "The voice service returned no audio."
        }
    }
}

/// Text to speech through ElevenLabs or OpenAI. Returns MP3 data; Kairos Desktop plays it.
public final class VoiceClient: @unchecked Sendable {
    public static let elevenLabsDefaultVoice = "21m00Tcm4TlvDq8ikWAM" // "Rachel"
    public static let openAIVoices = ["alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer"]

    private let session: URLSession
    private let lock = NSLock()
    private var cache: [String: Data] = [:]
    private var order: [String] = []

    public init(session: URLSession = .shared) { self.session = session }

    public static func request(provider: VoiceProvider, apiKey: String, voiceId: String, text: String) -> URLRequest? {
        var req: URLRequest
        var body: [String: Any]
        switch provider {
        case .system:
            return nil
        case .elevenlabs:
            let voice = voiceId.isEmpty ? elevenLabsDefaultVoice : voiceId
            guard let safe = voice.addingPercentEncoding(withAllowedCharacters: .alphanumerics),
                  let url = URL(string: "https://api.elevenlabs.io/v1/text-to-speech/\(safe)?output_format=mp3_44100_128") else { return nil }
            req = URLRequest(url: url)
            req.setValue(apiKey, forHTTPHeaderField: "xi-api-key")
            body = ["text": text, "model_id": "eleven_flash_v2_5"]
        case .openai:
            req = URLRequest(url: URL(string: "https://api.openai.com/v1/audio/speech")!)
            req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
            body = ["model": "gpt-4o-mini-tts", "voice": voiceId.isEmpty ? "alloy" : voiceId, "input": text, "response_format": "mp3"]
        }
        req.httpMethod = "POST"
        req.timeoutInterval = 30
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try? JSONSerialization.data(withJSONObject: body)
        return req
    }

    /// Synthesises `text`, reusing audio already fetched for the same provider, voice and text
    /// (so a prefetched next sentence plays without a gap).
    public func synthesize(provider: VoiceProvider, apiKey: String?, voiceId: String, text: String) async throws -> Data {
        guard let apiKey, !apiKey.isEmpty else { throw VoiceError.noKey }
        let key = "\(provider.rawValue)|\(voiceId)|\(text)"
        if let hit = cached(key) { return hit }
        guard let req = Self.request(provider: provider, apiKey: apiKey, voiceId: voiceId, text: text) else { throw VoiceError.noKey }
        let (data, response) = try await session.data(for: req)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw VoiceError.http(status, String(decoding: data, as: UTF8.self)) }
        guard !data.isEmpty else { throw VoiceError.empty }
        remember(key, data)
        return data
    }

    public func elevenLabsVoices(apiKey: String) async throws -> [CloudVoice] {
        var req = URLRequest(url: URL(string: "https://api.elevenlabs.io/v1/voices")!)
        req.setValue(apiKey, forHTTPHeaderField: "xi-api-key")
        let (data, response) = try await session.data(for: req)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw VoiceError.http(status, String(decoding: data, as: UTF8.self)) }
        return Self.parseElevenLabsVoices(data)
    }

    public static func parseElevenLabsVoices(_ data: Data) -> [CloudVoice] {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let voices = root["voices"] as? [[String: Any]] else { return [] }
        return voices.compactMap { v in
            guard let id = v["voice_id"] as? String, let name = v["name"] as? String else { return nil }
            return CloudVoice(id: id, name: name)
        }
    }

    private func cached(_ key: String) -> Data? {
        lock.lock(); defer { lock.unlock() }
        return cache[key]
    }

    private func remember(_ key: String, _ data: Data) {
        lock.lock(); defer { lock.unlock() }
        if cache[key] == nil { order.append(key) }
        cache[key] = data
        while order.count > 12 { cache[order.removeFirst()] = nil }
    }
}
