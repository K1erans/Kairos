import Foundation

public enum Rules {
    /// Lower-cases and strips scheme, `www.`, path and port from something a user typed.
    public static func normalizeSite(_ raw: String) -> String {
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if let r = s.range(of: "://") { s = String(s[r.upperBound...]) }
        if let slash = s.firstIndex(of: "/") { s = String(s[..<slash]) }
        if let colon = s.firstIndex(of: ":") { s = String(s[..<colon]) }
        if s.hasPrefix("www.") { s = String(s.dropFirst(4)) }
        return s
    }

    /// A site matches when the hostname equals the entry or is a subdomain of it.
    public static func hostMatches(_ host: String, entry: String) -> Bool {
        let h = normalizeSite(host), e = normalizeSite(entry)
        guard !e.isEmpty else { return false }
        return h == e || h.hasSuffix("." + e)
    }

    public static func host(_ host: String, in list: [String]) -> Bool {
        list.contains { hostMatches(host, entry: $0) }
    }

    public static func hostOf(urlString: String) -> String {
        if let h = URL(string: urlString)?.host { return normalizeSite(h) }
        return normalizeSite(urlString)
    }
}

/// Temporary "Allow 5 min" grants, limited per focus session.
public struct OverrideLedger: Codable, Equatable, Sendable {
    public private(set) var used = 0
    /// target (hostname or bundle id) -> expiry
    public private(set) var allows: [String: Date] = [:]

    public init() {}

    public enum Refusal: String, Error {
        case strict = "Strict mode has no overrides until the session ends."
        case exhausted = "You have used all your overrides for this session."
        case noReason = "Say why you need it first."
        case notBlocking = "Nothing is being blocked right now."
    }

    public func remaining(max: Int) -> Int { Swift.max(0, max - used) }

    public mutating func request(target: String, reason: String, mode: Mode, max: Int, minutes: Int, now: Date) -> Result<Date, Refusal> {
        guard mode != .strict else { return .failure(.strict) }
        guard mode == .focus else { return .failure(.notBlocking) }
        guard !reason.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return .failure(.noReason) }
        guard used < max else { return .failure(.exhausted) }
        used += 1
        let until = now.addingTimeInterval(TimeInterval(minutes * 60))
        allows[target] = until
        return .success(until)
    }

    public func isAllowed(_ target: String, now: Date) -> Bool {
        if let until = allows[target], until > now { return true }
        // Site grants also cover subdomains.
        return allows.contains { $0.value > now && Rules.hostMatches(target, entry: $0.key) }
    }

    public func active(now: Date) -> [String: Date] { allows.filter { $0.value > now } }

    /// When the next live grant runs out, so the app can re-check and re-push state then.
    public func nextExpiry(after now: Date) -> Date? { allows.values.filter { $0 > now }.min() }

    /// Called when a new focus session starts.
    public mutating func reset() { used = 0; allows = [:] }
}

public enum BridgeState {
    static func ms(_ d: Date) -> Int { Int((d.timeIntervalSince1970 * 1000).rounded()) }

    /// The `state` object pushed to the extensions (see packages/shared/PROTOCOL.md).
    public static func payload(data: AppData, ledger: OverrideLedger, cloudVoice: Bool = false, now: Date = Date()) -> [String: Any] {
        let group = data.activeGroup
        let task = data.currentTask
        var rules: [String: Any] = [
            "blockSites": group?.blockSites ?? [],
            "allowSites": group?.allowSites ?? [],
            "hiddenElements": group?.hiddenElements ?? [:],
            "jevEnabled": group?.jevEnabled ?? false,
            "jevThreshold": group?.jevThreshold ?? 0.7,
            "neverAnalyse": data.settings.neverAnalyse,
        ]
        rules["temporaryAllows"] = ledger.active(now: now).mapValues { ms($0) }
        let overridesLeft = data.focus.mode == .focus ? ledger.remaining(max: data.settings.maxOverridesPerSession) : 0
        return [
            "mode": data.focus.mode.rawValue,
            "activeGroupId": group?.id.uuidString ?? "",
            "groupName": group?.name ?? "",
            "groupColor": group?.colorHex ?? "#4F7CFF",
            "currentTask": task.map { ["id": $0.id.uuidString, "title": $0.title] as [String: Any] } ?? NSNull(),
            "endsAt": data.focus.endsAt.map { ms($0) as Any } ?? NSNull(),
            "overridesLeft": overridesLeft,
            "overrideMinutes": data.settings.overrideMinutes,
            "strictUntil": data.focus.strictUntil.map { ms($0) as Any } ?? NSNull(),
            "cloudVoice": cloudVoice ? ["provider": data.settings.voiceProvider.rawValue, "name": data.settings.voiceProvider.label] as [String: Any] : NSNull(),
            "rules": rules,
        ]
    }
}

/// The two site lists of a group.
public enum SiteList: String, Sendable {
    case block, allow
}

extension AppData {
    /// `rule.addSite`: Kairos Shield's "Block this site" and the Jev overlay's "Always allow".
    /// The site goes on one list of the active group and off the other. An allow loosens
    /// blocking, so like an "Allow" override it is refused while a strict focus phase is locked.
    /// Returns false when nothing changed.
    @discardableResult
    public mutating func addSite(_ raw: String, to list: SiteList, strictLocked: Bool) -> Bool {
        let site = Rules.normalizeSite(raw)
        guard !site.isEmpty, !(list == .allow && strictLocked), let group = activeGroup else { return false }
        var g = group
        switch list {
        case .block:
            g.allowSites.removeAll { $0 == site }
            if !g.blockSites.contains(site) { g.blockSites.append(site) }
        case .allow:
            g.blockSites.removeAll { $0 == site }
            if !g.allowSites.contains(site) { g.allowSites.append(site) }
        }
        guard g != group else { return false }
        updateActiveGroup { $0 = g }
        return true
    }

    /// "Block in <group>" from Kairos Bar. Unblocking loosens blocking, so it waits while a strict
    /// focus phase is locked. Returns false when nothing changed.
    @discardableResult
    public mutating func setAppBlocked(_ bundleId: String, _ blocked: Bool, strictLocked: Bool) -> Bool {
        guard let group = activeGroup, group.blockedApps.contains(bundleId) != blocked, blocked || !strictLocked else { return false }
        updateActiveGroup { g in
            if blocked { g.blockedApps.append(bundleId) } else { g.blockedApps.removeAll { $0 == bundleId } }
        }
        return true
    }
}
