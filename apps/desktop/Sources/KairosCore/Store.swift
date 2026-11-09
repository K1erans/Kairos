import Foundation

/// Loads and saves `AppData` as one JSON file. Saves are atomic.
public final class DataStore {
    public let url: URL

    public init(url: URL) { self.url = url }

    /// `KAIROS_HOME` points the app and the native host at another folder (used for testing).
    public static var defaultDirectory: URL {
        if let home = ProcessInfo.processInfo.environment["KAIROS_HOME"], !home.isEmpty {
            return URL(fileURLWithPath: home, isDirectory: true)
        }
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Kairos", isDirectory: true)
    }

    public static func standard() -> DataStore {
        DataStore(url: defaultDirectory.appendingPathComponent("kairos.json"))
    }

    public static var encoder: JSONEncoder {
        let e = JSONEncoder()
        e.outputFormatting = [.prettyPrinted, .sortedKeys]
        e.dateEncodingStrategy = .iso8601
        return e
    }

    public static var decoder: JSONDecoder {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .iso8601
        return d
    }

    /// Returns starter data when no file exists. A file that fails to decode is kept
    /// beside the store as `kairos.corrupt-<time>.json` rather than overwritten.
    public func load() -> AppData {
        guard let raw = try? Data(contentsOf: url) else { return .starter() }
        do {
            return try Self.decoder.decode(AppData.self, from: raw)
        } catch {
            let stamp = Int(Date().timeIntervalSince1970)
            let backup = url.deletingLastPathComponent().appendingPathComponent("kairos.corrupt-\(stamp).json")
            try? raw.write(to: backup)
            return .starter()
        }
    }

    public func save(_ data: AppData) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Self.encoder.encode(data).write(to: url, options: .atomic)
    }
}
