import Foundation

public enum ImportProblem: LocalizedError, Equatable {
    /// Not JSON, or JSON that is not a Kairos export.
    case notAnExport
    /// Import waits for the timer to stop, so it cannot end or change a focus phase.
    case timerRunning

    public var errorDescription: String? {
        switch self {
        case .notAnExport: return "The file is not a Kairos export."
        case .timerRunning: return "Stop the timer first."
        }
    }
}

extension AppData {
    /// Reads a file made by "Export everything as JSON". Decoding is lenient, so any JSON object
    /// would read as an empty profile; a file without groups is refused instead.
    public static func fromExport(_ raw: Data) throws -> AppData {
        guard let file = try? DataStore.decoder.decode(AppData.self, from: raw), !file.groups.isEmpty else {
            throw ImportProblem.notAnExport
        }
        return file
    }

    /// What Import puts in place of this data: everything from `file`, except what is live on
    /// this Mac. The mode and the timer's saved state, the open session, the extensions seen
    /// here and onboarding stay as they are. The file's active group and current task are used
    /// when the file has them.
    public func importing(_ file: AppData, now: Date) -> AppData {
        var next = file
        next.focus.mode = focus.mode
        next.focus.endsAt = focus.endsAt
        next.focus.strictUntil = focus.strictUntil
        next.openSession = openSession
        next.seenClients = seenClients
        next.onboarded = onboarded
        if next.group(next.focus.activeGroupId) == nil { next.focus.activeGroupId = next.groups.first?.id }
        if let id = next.focus.currentTaskId, !next.tasks.contains(where: { $0.id == id }) { next.focus.currentTaskId = nil }
        next.trimHistory(now: now)
        return next
    }
}

extension DataStore {
    /// Copies the saved file to `kairos-backup-<date>-<time>.json` beside it, before an import
    /// replaces it. Nil when there is no saved file yet.
    public func backup(now: Date) throws -> URL? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyyMMdd-HHmmss"
        let stamp = format.string(from: now)
        var target = url.deletingLastPathComponent().appendingPathComponent("kairos-backup-\(stamp).json")
        var n = 2
        while FileManager.default.fileExists(atPath: target.path) {
            target = url.deletingLastPathComponent().appendingPathComponent("kairos-backup-\(stamp)-\(n).json")
            n += 1
        }
        try FileManager.default.copyItem(at: url, to: target)
        return target
    }
}
