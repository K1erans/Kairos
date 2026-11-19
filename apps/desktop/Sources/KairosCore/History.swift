import Foundation

extension Session {
    /// Shorter phases are false starts and are not logged.
    public static let minimumLength: TimeInterval = 30

    /// The session a finished timer phase leaves in the log, or nil for anything else.
    public init?(_ event: TempoEvent, taskId: UUID?, groupId: UUID?) {
        guard case let .finished(phase, start, end, duration, interruptions, completed) = event,
              phase != .idle, duration >= Self.minimumLength else { return nil }
        self.init(taskId: phase == .focus ? taskId : nil, groupId: groupId, start: start, end: end,
                  activeSeconds: duration, kind: phase == .focus ? .focus : .rest,
                  interruptions: interruptions, completed: completed)
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
            let focusOpen = openSession.flatMap { $0.kind == .focus ? $0 : nil }
            let start = focusOpen?.start ?? now
            let paused = focusOpen.map { max(0, $0.end.timeIntervalSince($0.start) - $0.duration) } ?? 0
            openSession = nil
            focus.mode = .strict
            focus.endsAt = until
            return StrictResume(startedAt: min(start, now), endsAt: until, pausedDuration: paused)
        }
        if let open = openSession, open.duration >= Session.minimumLength {
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

/// Window visibility when entering a group. Only apps Kairos hid for a group switch can be restored.
public struct GroupSwitch {
    public enum Action { case hide, unhide }

    private let behavior: SwitchBehavior
    private let apps: Set<String>

    public init(to group: AppGroup?, behavior: SwitchBehavior) {
        self.behavior = group == nil ? .leave : behavior
        apps = Set((behavior == .hideBlocked ? group?.blockedApps : group?.dockApps) ?? [])
    }

    public func action(for bundleId: String?, isHidden: Bool, hiddenBySwitch: Bool) -> Action? {
        let listed = bundleId.map(apps.contains) ?? false
        let shouldHide: Bool
        switch behavior {
        case .leave: shouldHide = false
        case .hideBlocked: shouldHide = listed
        case .hide: shouldHide = !listed
        }
        if shouldHide && bundleId != "com.apple.finder" { return isHidden ? nil : .hide }
        return hiddenBySwitch ? .unhide : nil
    }
}
