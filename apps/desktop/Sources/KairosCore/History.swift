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

    public mutating func trimHistory(now: Date) {
        let cutoff = now.addingTimeInterval(-Double(Self.sessionDays) * 86400)
        if let first = sessions.first, first.end < cutoff { sessions.removeAll { $0.end < cutoff } }
        if blockEvents.count > Self.maxBlockEvents { blockEvents.removeFirst(blockEvents.count - Self.maxBlockEvents) }
    }

    /// Run once at launch. The timer does not survive a quit, so nothing is left blocking, and
    /// the phase that was running is logged as cut short at the last save.
    public mutating func recoverAfterLaunch(now: Date = Date()) {
        if let open = openSession, open.end.timeIntervalSince(open.start) >= Session.minimumLength {
            var cut = open
            cut.completed = false
            sessions.append(cut)
            trimHistory(now: now)
        }
        openSession = nil
        focus.mode = .off
        focus.endsAt = nil
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
    public mutating func updateActiveGroup(_ change: (inout AppGroup) -> Void) {
        guard let i = groups.firstIndex(where: { $0.id == activeGroup?.id }) else { return }
        change(&groups[i])
    }
}

/// What to hide and unhide when "hide apps outside the group" switches groups.
public enum GroupSwitch {
    /// `visible` are the regular apps on screen now. Apps hidden when the new group was last
    /// left come back first, and stay: they belong to that group's workspace. Then everything
    /// else outside `keep` is hidden.
    public static func plan(visible: [String], keep: Set<String>, restore: [String]) -> (unhide: [String], hide: [String]) {
        let back = Set(restore)
        return (restore, visible.filter { !keep.contains($0) && !back.contains($0) })
    }
}
