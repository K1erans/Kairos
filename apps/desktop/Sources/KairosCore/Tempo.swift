import Foundation

public enum TempoPhase: String, Codable, Sendable {
    case idle, focus, shortBreak, longBreak
    public var isBreak: Bool { self == .shortBreak || self == .longBreak }
    public var label: String {
        switch self {
        case .idle: return "Idle"
        case .focus: return "Focus"
        case .shortBreak: return "Short break"
        case .longBreak: return "Long break"
        }
    }
}

public enum TempoEvent: Equatable {
    /// A phase ran to its end (or was skipped). Carries what is needed to log a session.
    case finished(phase: TempoPhase, start: Date, end: Date, interruptions: Int, completed: Bool)
    case started(phase: TempoPhase)
    /// The timer went idle: stopped by hand, or the Mac slept through the next phase.
    case stopped
    /// A break ran out and the timer went idle to wait for the owner to start the next focus
    /// phase ("Start the next focus session automatically" is off).
    case awaitingFocus
}

/// A strict focus phase that was running when Kairos quit, to pick up at launch.
public struct StrictResume: Equatable, Sendable {
    public var startedAt: Date
    public var endsAt: Date
    public init(startedAt: Date, endsAt: Date) { self.startedAt = startedAt; self.endsAt = endsAt }
}

/// What Kairos asks before a strict focus phase starts, naming when it ends.
public struct StrictConfirmation: Equatable, Sendable {
    public let title: String
    public let message: String

    public init(minutes: Int, endsAt: Date, locale: Locale = .current, timeZone: TimeZone = .current) {
        let f = DateFormatter()
        f.locale = locale
        f.timeZone = timeZone
        f.dateStyle = .none
        f.timeStyle = .short
        let time = f.string(from: endsAt)
        title = "Start strict focus until \(time)?"
        message = "For the next \(minutes) minutes, until \(time), this session cannot be stopped, skipped or quit, "
            + "and there are no \u{201C}Allow\u{201D} overrides. Blocked apps are hidden and blocked sites stay blocked, "
            + "even if Kairos is closed or the Mac restarts."
    }
}

/// Pomodoro state machine. Time is stored as `endsAt`, never as a ticking counter, so sleep
/// and lid-close cannot make it drift.
public struct TempoEngine: Equatable, Sendable {
    public private(set) var phase: TempoPhase = .idle
    public private(set) var startedAt: Date?
    public private(set) var endsAt: Date?
    /// Seconds left when paused; nil while running.
    public private(set) var pausedRemaining: TimeInterval?
    private var pausedAt: Date?
    /// Full length of the current phase, including any "+5 min".
    public private(set) var total: TimeInterval = 0
    public private(set) var focusCount = 0
    public private(set) var interruptions = 0
    public private(set) var preset: TempoPreset
    /// A preset chosen while a phase runs; it takes over when the next phase begins.
    private var pendingPreset: TempoPreset?
    /// Strict chosen by hand when the timer started. Holds until the timer stops.
    private var holdsStrict = false
    /// Idle because a break ran out and the owner has not started the next focus phase yet.
    /// The count of focus phases carries over, so the long break still comes on time.
    public private(set) var awaitingFocus = false

    public init(preset: TempoPreset = TempoPreset.defaults[0]) { self.preset = preset }

    public var isRunning: Bool { phase != .idle && pausedRemaining == nil }
    public var isPaused: Bool { pausedRemaining != nil }

    /// The mode this phase puts Kairos in.
    public var mode: Mode {
        switch phase {
        case .idle: return .off
        case .focus: return holdsStrict || preset.strict ? .strict : .focus
        case .shortBreak, .longBreak: return .onBreak
        }
    }

    /// Applies at once when idle, otherwise from the next phase, so a running phase keeps
    /// the length it was started with.
    public mutating func use(_ preset: TempoPreset) {
        if phase == .idle { self.preset = preset; pendingPreset = nil } else { pendingPreset = preset }
    }
    public func remaining(now: Date) -> TimeInterval {
        if let pausedRemaining { return pausedRemaining }
        guard let endsAt else { return 0 }
        return max(0, endsAt.timeIntervalSince(now))
    }

    public func duration(of phase: TempoPhase) -> TimeInterval {
        switch phase {
        case .idle: return 0
        case .focus: return TimeInterval(preset.focusMin * 60)
        case .shortBreak: return TimeInterval(preset.shortBreakMin * 60)
        case .longBreak: return TimeInterval(preset.longBreakMin * 60)
        }
    }

    /// 0...1 progress through the current phase.
    public func progress(now: Date) -> Double {
        guard phase != .idle, total > 0 else { return 0 }
        return min(1, max(0, 1 - remaining(now: now) / total))
    }

    /// Starts a new timer. `strict` holds strict mode for every focus phase until it stops.
    @discardableResult
    public mutating func start(_ newPhase: TempoPhase = .focus, strict: Bool = false, now: Date) -> [TempoEvent] {
        guard newPhase != .idle else { return stop(now: now) }
        var events = closeCurrent(now: now, completed: false)
        takePendingPreset()
        // Starting again after a break that waited carries on the same run of sessions.
        if !(awaitingFocus && newPhase == .focus) { focusCount = 0 }
        awaitingFocus = false
        holdsStrict = strict
        begin(newPhase, now: now)
        events.append(.started(phase: newPhase))
        return events
    }

    /// Picks up a strict focus phase saved before a quit. It holds strict, as a hand-picked
    /// Strict does, because the launch cannot tell which of the two started it.
    public mutating func resumeStrict(_ saved: StrictResume) {
        takePendingPreset()
        phase = .focus
        startedAt = saved.startedAt
        endsAt = saved.endsAt
        total = max(0, saved.endsAt.timeIntervalSince(saved.startedAt))
        pausedRemaining = nil
        pausedAt = nil
        interruptions = 0
        focusCount = 0
        holdsStrict = true
    }

    /// "Not now" after a break ran out: stop waiting. The mode is left alone, and the next focus
    /// phase starts a new count toward the long break.
    public mutating func endWait() {
        guard awaitingFocus else { return }
        awaitingFocus = false
        focusCount = 0
    }

    /// Whether the next focus phase of a running timer is strict, as a skip out of a break starts it.
    public var nextFocusIsStrict: Bool { holdsStrict || (pendingPreset ?? preset).strict }

    /// A phase that has run out has nothing left to pause; the next tick ends it.
    public mutating func pause(now: Date) {
        guard isRunning, remaining(now: now) > 0 else { return }
        pausedRemaining = remaining(now: now)
        pausedAt = now
        interruptions += 1
    }

    public mutating func resume(now: Date) {
        guard let left = pausedRemaining else { return }
        pausedRemaining = nil
        pausedAt = nil
        endsAt = now.addingTimeInterval(left)
    }

    public mutating func extend(minutes: Int, now: Date) {
        guard phase != .idle else { return }
        let extra = TimeInterval(minutes * 60)
        total += extra
        if let left = pausedRemaining { pausedRemaining = left + extra }
        else if let endsAt { self.endsAt = max(endsAt, now).addingTimeInterval(extra) }
    }

    /// Ends the current phase early and moves to the next one.
    @discardableResult
    public mutating func skip(now: Date) -> [TempoEvent] {
        guard phase != .idle else { return [] }
        if phase == .focus { interruptions += 1 }
        return advance(at: now, completed: false, now: now, waitsAfterBreak: false)
    }

    @discardableResult
    public mutating func stop(now: Date) -> [TempoEvent] {
        let events = closeCurrent(now: now, completed: false)
        reset()
        return events + [.stopped]
    }

    /// Call regularly (and on wake). Ends a phase that has run out at its own end time. After
    /// a sleep, the timer lands in the phase that holds `now`; a sleep that outlasted that
    /// phase as well stops the timer, and the phases slept through are never logged.
    /// `startsFocusAfterBreak` false ("Start the next focus session automatically" off): a break
    /// that runs out stops the timer and waits for the owner (`awaitingFocus`).
    @discardableResult
    public mutating func tick(now: Date, startsFocusAfterBreak: Bool = true) -> [TempoEvent] {
        guard isRunning, let endsAt, now >= endsAt else { return [] }
        return advance(at: endsAt, completed: true, now: now, waitsAfterBreak: !startsFocusAfterBreak)
    }

    /// The running phase as a session cut short at `now`, for saving before a quit.
    public func current(now: Date) -> TempoEvent? { closeCurrent(now: now, completed: false).first }

    private mutating func advance(at end: Date, completed: Bool, now: Date, waitsAfterBreak: Bool) -> [TempoEvent] {
        let finished = phase
        var events = closeCurrent(now: end, completed: completed)
        takePendingPreset()
        let next: TempoPhase
        if finished == .focus {
            if completed { focusCount += 1 }
            let long = completed && preset.cyclesBeforeLong > 0 && focusCount % preset.cyclesBeforeLong == 0
            next = long ? .longBreak : .shortBreak
        } else {
            next = .focus
        }
        if now > end, now >= end.addingTimeInterval(duration(of: next)) {
            reset()
            return events + [.stopped]
        }
        if waitsAfterBreak, finished.isBreak {
            let count = focusCount
            reset()
            focusCount = count
            awaitingFocus = true
            return events + [.awaitingFocus]
        }
        begin(next, now: end)
        events.append(.started(phase: next))
        return events
    }

    private mutating func takePendingPreset() {
        if let p = pendingPreset { preset = p; pendingPreset = nil }
    }

    private mutating func reset() {
        takePendingPreset()
        phase = .idle; startedAt = nil; endsAt = nil; pausedRemaining = nil; pausedAt = nil
        interruptions = 0; total = 0; focusCount = 0; holdsStrict = false
        awaitingFocus = false
    }

    private func closeCurrent(now: Date, completed: Bool) -> [TempoEvent] {
        guard phase != .idle, let startedAt else { return [] }
        // A paused phase stopped counting when it was paused.
        let end = pausedAt ?? now
        return [.finished(phase: phase, start: startedAt, end: max(startedAt, end), interruptions: interruptions, completed: completed)]
    }

    private mutating func begin(_ p: TempoPhase, now: Date) {
        phase = p
        startedAt = now
        total = duration(of: p)
        endsAt = now.addingTimeInterval(total)
        pausedRemaining = nil
        pausedAt = nil
        interruptions = 0
    }
}

/// The text next to Kairos's menu bar icon. Kept short: a long title pushes other menu bar
/// items off screen, or under the notch, where macOS hides them.
public enum MenuBarTitle {
    public static let idleLimit = 20
    public static let runningLimit = 16

    public static func make(name: String, phase: TempoPhase, remaining: TimeInterval, awaitingFocus: Bool) -> String {
        if awaitingFocus { return "Break over" }
        switch phase {
        case .idle: return cap(name, idleLimit)
        case .focus: return "\(cap(name, runningLimit)) · \(clock(remaining))"
        case .shortBreak, .longBreak: return "Break · \(clock(remaining))"
        }
    }

    /// The name with an ellipsis once it passes `limit` characters.

    static func cap(_ text: String, _ limit: Int) -> String {
        guard text.count > limit else { return text }
        return String(text.prefix(limit - 1)).trimmingCharacters(in: .whitespaces) + "\u{2026}"
    }
}

/// Time left as "m:ss", rounded up so a phase never shows 0:00 while it still runs.
public func clock(_ seconds: TimeInterval) -> String {
    let s = max(0, Int(seconds.rounded(.up)))
    return String(format: "%d:%02d", s / 60, s % 60)
}
