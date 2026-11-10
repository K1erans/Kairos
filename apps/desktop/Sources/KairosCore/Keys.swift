import Foundation

/// Where API keys (TypeSafe, Anthropic, ElevenLabs, OpenAI) are kept. The real profile keeps
/// them in the login Keychain. A scratch copy (`KAIROS_HOME`, used by the checks and the snapshot
/// tool) keeps them in a plain file in its scratch folder instead, so it never reads the owner's
/// Keychain and never makes macOS ask for it.
public enum KeyStore: Equatable, Sendable {
    case keychain
    case file(URL)

    public static func at(environment: [String: String]) -> KeyStore {
        guard let home = environment["KAIROS_HOME"], !home.isEmpty else { return .keychain }
        return .file(URL(fileURLWithPath: home, isDirectory: true).appendingPathComponent("keys.json"))
    }

    public static var current: KeyStore { at(environment: ProcessInfo.processInfo.environment) }
}

/// A scratch copy's keys: account -> key, as JSON.
public struct ScratchKeys: Sendable {
    public let url: URL

    public init(url: URL) { self.url = url }

    private func all() -> [String: String] {
        guard let data = try? Data(contentsOf: url),
              let keys = try? JSONDecoder().decode([String: String].self, from: data) else { return [:] }
        return keys
    }

    public func get(_ account: String) -> String? { all()[account] }

    /// An empty value removes the key.
    @discardableResult
    public func set(_ value: String, for account: String) -> Bool {
        var keys = all()
        keys[account] = value.isEmpty ? nil : value
        guard let data = try? JSONEncoder().encode(keys) else { return false }
        return (try? data.write(to: url, options: .atomic)) != nil
    }
}
