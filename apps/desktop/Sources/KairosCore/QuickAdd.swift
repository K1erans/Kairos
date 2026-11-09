import Foundation

/// Result of parsing a quick-add line such as
/// `Draft blog post tomorrow 3pm @Work ~45m !high`.
public struct ParsedTask: Equatable {
    public var title: String
    public var due: Date?
    public var groupName: String?
    public var estimateMin: Int?
    public var priority: Int = 0
}

public enum QuickAdd {
    static let weekdays = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"]

    /// One word, or "next week", read as what it most likely means.
    private enum Piece {
        case title(String)
        case group(String)
        case estimate(Int)
        case priority(Int)
        case day(Date)
        /// "sun", "sat": also plain words, so a date only where nothing but tokens follows it,
        /// or a time comes straight after.
        case shortDay(Date, word: String)
        case time(hour: Int, minute: Int)

        var isTitle: Bool { if case .title = self { return true } else { return false } }
        var isTime: Bool { if case .time = self { return true } else { return false } }
    }

    /// `groups` are the existing group names. An @word naming none of them stays in the title.
    public static func parse(_ input: String, groups: [String], now: Date = Date(), calendar: Calendar = .current) -> ParsedTask {
        var pieces = read(input, groups: groups, now: now, calendar: calendar)
        var onlyTokensAfter = true
        for i in pieces.indices.reversed() {
            if case let .shortDay(_, word) = pieces[i], !onlyTokensAfter, !(i + 1 < pieces.count && pieces[i + 1].isTime) {
                pieces[i] = .title(word)
            }
            onlyTokensAfter = onlyTokensAfter && !pieces[i].isTitle
        }

        var result = ParsedTask(title: "")
        var title: [String] = []
        var day: Date?
        var time: (hour: Int, minute: Int)?
        for piece in pieces {
            switch piece {
            case let .title(w): title.append(w)
            case let .group(g): result.groupName = g
            case let .estimate(m): result.estimateMin = m
            case let .priority(p): result.priority = p
            case let .day(d), let .shortDay(d, _): day = d
            case let .time(h, m): time = (h, m)
            }
        }
        result.title = title.joined(separator: " ")
        if day != nil || time != nil {
            let base = day ?? now
            var comps = calendar.dateComponents([.year, .month, .day], from: base)
            comps.hour = time?.hour ?? 17
            comps.minute = time?.minute ?? 0
            var due = calendar.date(from: comps)
            // A bare time already past today means tomorrow.
            if day == nil, let d = due, d <= now {
                due = calendar.date(byAdding: .day, value: 1, to: d)
            }
            result.due = due
        }
        return result
    }

    private static func read(_ input: String, groups: [String], now: Date, calendar: Calendar) -> [Piece] {
        let words = input.split(separator: " ").map(String.init)
        var pieces: [Piece] = []
        var i = 0
        while i < words.count {
            let word = words[i], lower = word.lowercased()
            i += 1
            if (word.hasPrefix("@") || word.hasPrefix("#")), word.count > 1 {
                let name = word.dropFirst()
                if let known = groups.first(where: { $0.caseInsensitiveCompare(name) == .orderedSame }) {
                    pieces.append(.group(known))
                } else {
                    pieces.append(.title(word))
                }
            } else if word.hasPrefix("~"), let m = minutes(String(lower.dropFirst())) {
                pieces.append(.estimate(m))
            } else if word.hasPrefix("!"), let p = priority(String(lower.dropFirst())) {
                pieces.append(.priority(p))
            } else if lower == "today" {
                pieces.append(.day(now))
            } else if lower == "tomorrow" || lower == "tmrw", let d = calendar.date(byAdding: .day, value: 1, to: now) {
                pieces.append(.day(d))
            } else if lower == "next", i < words.count, words[i].lowercased() == "week",
                      let d = calendar.date(byAdding: .day, value: 7, to: now) {
                pieces.append(.day(d))
                i += 1
            } else if let wd = weekdays.firstIndex(of: lower), let d = next(weekday: wd, now: now, calendar: calendar) {
                pieces.append(.day(d))
            } else if lower.count == 3, let wd = weekdays.firstIndex(where: { $0.hasPrefix(lower) }),
                      let d = next(weekday: wd, now: now, calendar: calendar) {
                pieces.append(.shortDay(d, word: word))
            } else if let t = clock(lower) {
                pieces.append(.time(hour: t.hour, minute: t.minute))
            } else {
                pieces.append(.title(word))
            }
        }
        return pieces
    }

    /// The next such weekday after today; the same weekday means next week.
    private static func next(weekday: Int, now: Date, calendar: Calendar) -> Date? {
        let today = calendar.component(.weekday, from: now) - 1
        var delta = (weekday - today + 7) % 7
        if delta == 0 { delta = 7 }
        return calendar.date(byAdding: .day, value: delta, to: now)
    }

    private static let duration = try! NSRegularExpression( // swiftlint:disable:this force_try
        pattern: "^(?:(\\d+)(?:h|hr|hrs|hour|hours))?(?:(\\d+)(?:m|min|mins|minute|minutes)?)?$")

    /// "45m", "45min", "2h", "2hrs", "1h30m", "1h30", "90" -> minutes
    static func minutes(_ s: String) -> Int? {
        if let n = Int(s) { return n > 0 ? n : nil }
        let range = NSRange(s.startIndex..., in: s)
        guard let m = duration.firstMatch(in: s, range: range) else { return nil }
        func number(_ group: Int) -> Int {
            guard let r = Range(m.range(at: group), in: s) else { return 0 }
            return Int(s[r]) ?? 0
        }
        let total = number(1) * 60 + number(2)
        return total > 0 ? total : nil
    }

    static func priority(_ s: String) -> Int? {
        switch s {
        case "high", "h", "3", "urgent": return 3
        case "med", "medium", "m", "2": return 2
        case "low", "l", "1": return 1
        default: return nil
        }
    }

    /// "3pm", "3:30pm", "15:00", "9am"
    static func clock(_ s: String) -> (hour: Int, minute: Int)? {
        var body = s
        var meridiem: String?
        if body.hasSuffix("am") || body.hasSuffix("pm") {
            meridiem = String(body.suffix(2)); body = String(body.dropLast(2))
        }
        let parts = body.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count <= 2, let first = parts.first, var hour = Int(first) else { return nil }
        var minute = 0
        if parts.count == 2 {
            guard parts[1].count == 2, let m = Int(parts[1]) else { return nil }
            minute = m
        } else if meridiem == nil {
            return nil // a bare number is not a time
        }
        if let meridiem {
            guard (1...12).contains(hour) else { return nil }
            if meridiem == "pm", hour < 12 { hour += 12 }
            if meridiem == "am", hour == 12 { hour = 0 }
        }
        guard (0...23).contains(hour), (0...59).contains(minute) else { return nil }
        return (hour, minute)
    }
}

public enum SmartNext {
    /// The task to suggest next: unfinished tasks in the group, earliest due first, then by
    /// priority. A task whose estimate fits `availableMin` beats one that does not.
    public static func suggest(tasks: [TaskItem], groupId: UUID?, excluding: UUID? = nil, availableMin: Int? = nil) -> TaskItem? {
        let pool = tasks.filter {
            $0.status == .todo && $0.id != excluding && $0.parentId == nil && (groupId == nil || $0.groupId == groupId)
        }
        func fits(_ t: TaskItem) -> Bool {
            guard let availableMin, let e = t.estimateMin else { return true }
            return e <= availableMin
        }
        return pool.sorted { a, b in
            if fits(a) != fits(b) { return fits(a) }
            switch (a.due, b.due) {
            case let (x?, y?) where x != y: return x < y
            case (_?, nil): return true
            case (nil, _?): return false
            default: break
            }
            if a.priority != b.priority { return a.priority > b.priority }
            return a.createdAt < b.createdAt
        }.first
    }
}
