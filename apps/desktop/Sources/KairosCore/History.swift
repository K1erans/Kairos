import Foundation

extension Session {
    /// Shorter phases are false starts and are not logged.
    public static let minimumLength: TimeInterval = 30

    /// The session a finished timer phase leaves in the log, or nil for anything else.
    public init?(_ event: TempoEvent, taskId: UUID?, groupId: UUID?) {
        guard case let .finished(phase, start, end, interruptions, completed) = event,
              phase != .idle, end.timeIntervalSince(start) >= Self.minimumLength else { return nil }
        self.init(taskId: phase == .focus ? taskId : nil, groupId: groupId, start: start, end: end,
                  kind: phase == .focus ? .focus : .rest, interruptions: interruptions, completed: completed)
    }
}

extension AppData {
    /// How much history is kept.
    public static let sessionDays = 400
    public static let maxBlockEvents = 5000
    /// Far more than a year of focus and break phases, so it bounds only a runaway.
    public static let maxSessions = 20_000

    /// Logs a finished timer phase against the current task and group.
    public mutating func log(_ event: TempoEvent, now: Date = Date()) {
        guard let session = Session(event, taskId: focus.currentTaskId, groupId: focus.activeGroupId) else { return }
        sessions.append(session)
        trimHistory(now: now)
    }

    public mutating func logBlock(_ event: BlockEvent, now: Date = Date()) {
        blockEvents.append(event)
        trimHistory(now: now)
    }

    /// Keeps the saved file from growing without end: sessions and block events older than
    /// `sessionDays` go, then the oldest past each count limit.
    public mutating func trimHistory(now: Date) {
        let cutoff = now.addingTimeInterval(-Double(Self.sessionDays) * 86400)
        if sessions.contains(where: { $0.end < cutoff }) { sessions.removeAll { $0.end < cutoff } }
        if sessions.count > Self.maxSessions { sessions.removeFirst(sessions.count - Self.maxSessions) }
        if blockEvents.contains(where: { $0.time < cutoff }) { blockEvents.removeAll { $0.time < cutoff } }
        if blockEvents.count > Self.maxBlockEvents { blockEvents.removeFirst(blockEvents.count - Self.maxBlockEvents) }
    }

    /// Run once at launch. A strict focus phase that has not run out picks up where it was, so
    /// quitting or a crash cannot end it; the returned value is what the timer resumes. Any
    /// other timer does not survive a quit: nothing is left blocking, and the phase that was
    /// running is logged as cut short at the last save.
    @discardableResult
    public mutating func recoverAfterLaunch(now: Date = Date()) -> StrictResume? {
        trimHistory(now: now)
        if let until = focus.strictUntil, until > now {
            let start = openSession.flatMap { $0.kind == .focus ? $0.start : nil } ?? now
            openSession = nil
            focus.mode = .strict
            focus.endsAt = until
            return StrictResume(startedAt: min(start, now), endsAt: until)
        }
        if let open = openSession, open.end.timeIntervalSince(open.start) >= Session.minimumLength {
            var cut = open
            cut.completed = false
            sessions.append(cut)
            trimHistory(now: now)
        }
        openSession = nil
        focus.mode = .off
        focus.endsAt = nil
        focus.strictUntil = nil
        return nil
    }

    /// Marks the newest unmarked block of `target` as overridden. Sites match the grant's
    /// subdomains too, as the grant itself does.
    public mutating func markOverridden(target: String) {
        guard let i = blockEvents.lastIndex(where: { !$0.overridden && ($0.target == target || Rules.hostMatches($0.target, entry: target)) })
        else { return }
        blockEvents[i].overridden = true
    }

    /// Kairos Read time goes to the current task, or to an unassigned total when there is none.
    public mutating func logListening(seconds: Int) {
        guard seconds > 0 else { return }
        if let i = tasks.firstIndex(where: { $0.id == focus.currentTaskId }) {
            tasks[i].listenedSec += seconds
        } else {
            unassignedListenedSec += seconds
        }
    }

    /// Applies `change` to the active group, if there is one.
    /// The focus state after switching to group `id`. The group being left remembers its current
    /// task, and the task `id` remembered comes back if it is still open in that group.
    public func focusAfterSwitch(to id: UUID) -> FocusState {
        var focus = self.focus
        if let old = focus.activeGroupId { focus.taskByGroup[old.uuidString] = focus.currentTaskId }
        focus.activeGroupId = id
        if currentTask?.groupId != id {
            let remembered = focus.taskByGroup[id.uuidString].flatMap { tid in tasks.first { $0.id == tid } }
            focus.currentTaskId = remembered.flatMap { $0.status == .todo && $0.groupId == id ? $0.id : nil }
        }
        return focus
    }
}

/// What to hide and unhide when "hide apps outside the group" switches groups.
public enum GroupSwitch {
    /// `visible` are the regular apps on screen now. Apps hidden when the new group was last
    /// left come back first, and stay: they belong to that group's workspace. Then everything
    /// else outside `keep` is hidden.
    /// With `only` set, nothing outside it is hidden: that is "hide the apps the group blocks".
    public static func plan(visible: [String], keep: Set<String>, restore: [String], only: Set<String>? = nil) -> (unhide: [String], hide: [String]) {
        let back = Set(restore)
        return (restore, visible.filter { !keep.contains($0) && !back.contains($0) && (only?.contains($0) ?? true) })
    }
}
