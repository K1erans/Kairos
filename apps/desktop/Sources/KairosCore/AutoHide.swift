import Foundation

/// When Kairos Bar is on screen, after the macOS Dock. Pinned it is always out. With auto-hide
/// it slides off its edge shortly after the pointer leaves, and back after the pointer rests on
/// that screen edge for a moment. While a full-screen app has the Space it is hidden whatever the
/// setting, and only a second push against the edge brings it out. Pure, so the window only feeds
/// it the pointer and the Space, and applies `visible(at:)`.
public struct DockAutoHide: Equatable, Sendable {
    public enum State: Equatable, Sendable {
        /// Off screen. `armedAt` is when the pointer reached the edge, or last pushed against it
        /// in a full-screen Space, while it stays there.
        case hidden(armedAt: Date?)
        /// Sliding in; `from` is how far out it was at `at` (0 hidden, 1 shown).
        case revealing(from: Double, at: Date)
        /// On screen. `leftAt` is when the pointer left it, while it stays away.
        case shown(leftAt: Date?)
        case hiding(from: Double, at: Date)
    }

    /// What governs the bar on the current Space.
    public enum Rule: Equatable, Sendable {
        /// Auto-hide is off and the Space is a desktop: always out.
        case pinned
        /// Auto-hide is on and the Space is a desktop.
        case autoHide
        /// A full-screen app has the Space: hidden whatever the setting, out on a second push.
        case fullScreen

        public init(autoHide: Bool, fullScreen: Bool) {
            self = fullScreen ? .fullScreen : autoHide ? .autoHide : .pinned
        }
    }

    /// What holds the bar out or calls it back.
    public struct Inputs: Equatable, Sendable {
        /// The pointer is in the thin strip along the bar's screen edge.
        public var atEdge: Bool
        /// The pointer is on the bar.
        public var onBar: Bool
        /// A context menu is open, the divider is being dragged, or an app is dragged over it.
        public var held: Bool
        /// The pointer moved toward the screen edge, which it cannot pass.
        public var pushed: Bool
        public init(atEdge: Bool = false, onBar: Bool = false, held: Bool = false, pushed: Bool = false) {
            self.atEdge = atEdge; self.onBar = onBar; self.held = held; self.pushed = pushed
        }
        var keepsOut: Bool { atEdge || onBar || held }
    }

    public static let revealDelay: TimeInterval = 0.2
    public static let hideDelay: TimeInterval = 0.5
    /// Time for the whole slide; a reversal mid-way takes the remaining share.
    public static let slide: TimeInterval = 0.3
    /// Width of the strip along the screen edge that calls the bar back.
    public static let edgeStrip = 2.0
    /// In a full-screen Space, how long the pointer must rest against the edge between the push
    /// that took it there and the push that brings the bar out.
    public static let secondPushPause: TimeInterval = 0.15

    public private(set) var state: State
    public private(set) var rule: Rule

    /// With auto-hide it starts out on screen, so turning auto-hide on shows where the bar lives
    /// before it goes.
    public init(rule: Rule = .autoHide, now: Date) {
        self.rule = rule
        state = rule == .fullScreen ? .hidden(armedAt: nil) : .shown(leftAt: rule == .pinned ? nil : now)
    }

    /// `rule` is the rule from now on; nil keeps the current one.
    public mutating func update(_ inputs: Inputs, rule next: Rule? = nil, now: Date) {
        if let next, next != rule { follow(next, inputs) }
        if rule == .pinned { state = .shown(leftAt: nil); return }
        switch state {
        case let .hidden(armedAt):
            guard inputs.atEdge else { state = .hidden(armedAt: nil); return }
            if rule == .fullScreen {
                // Reaching the edge, or pushing on without a pause, only arms it.
                guard let armed = armedAt, inputs.pushed else { state = .hidden(armedAt: armedAt ?? now); return }
                state = now.timeIntervalSince(armed) >= Self.secondPushPause ? .revealing(from: 0, at: now) : .hidden(armedAt: now)
                return
            }
            let armed = armedAt ?? now
            state = now.timeIntervalSince(armed) >= Self.revealDelay ? .revealing(from: 0, at: now) : .hidden(armedAt: armed)
        case .revealing:
            if progress(at: now) >= 1 { state = .shown(leftAt: inputs.keepsOut ? nil : now) }
        case let .shown(leftAt):
            guard !inputs.keepsOut else { state = .shown(leftAt: nil); return }
            let left = leftAt ?? now
            state = now.timeIntervalSince(left) >= Self.hideDelay ? .hiding(from: 1, at: now) : .shown(leftAt: left)
        case .hiding:
            let p = progress(at: now)
            if inputs.keepsOut {
                state = .revealing(from: p, at: now) // straight back, no delay
            } else if p <= 0 {
                state = .hidden(armedAt: nil)
            }
        }
    }

    /// A new rule comes with a Space change or a setting, so the bar jumps to where it belongs
    /// instead of sliding: a swipe into a full-screen app finds it gone, one back to a desktop
    /// with auto-hide off finds it there. Between auto-hide and full screen it stays as it is,
    /// unless it is out in a full-screen Space with nothing holding it.
    private mutating func follow(_ next: Rule, _ inputs: Inputs) {
        defer { rule = next }
        switch next {
        case .pinned:
            state = .shown(leftAt: nil)
        case .autoHide:
            break
        case .fullScreen:
            if !(inputs.onBar || inputs.held) { state = .hidden(armedAt: nil) }
        }
    }

    /// Linear position along the slide, 0 hidden to 1 shown.
    public func progress(at now: Date) -> Double {
        let p: Double
        switch state {
        case .hidden: return 0
        case .shown: return 1
        case let .revealing(from, at): p = from + now.timeIntervalSince(at) / Self.slide
        case let .hiding(from, at): p = from - now.timeIntervalSince(at) / Self.slide
        }
        // Dates far from their reference lose a little precision; a slide due now is done.
        return p >= 1 - 1e-6 ? 1 : p <= 1e-6 ? 0 : p
    }

    /// How much of the bar is on screen, eased in and out. Continuous through a reversal,
    /// because easing is applied to the shared linear position.
    public func visible(at now: Date) -> Double {
        let p = progress(at: now)
        return p * p * (3 - 2 * p)
    }

    /// Whether the bar takes the pointer: only once it is on its way in or out.
    public var isOut: Bool {
        switch state {
        case .revealing, .shown: return true
        case .hidden, .hiding: return false
        }
    }

    public var isSliding: Bool {
        switch state {
        case .revealing, .hiding: return true
        case .hidden, .shown: return false
        }
    }

    /// When `update` must run again even if the pointer does not move.
    public func nextDecision() -> Date? {
        switch state {
        case let .hidden(armedAt):
            // In a full-screen Space only a push brings it out, and a push is an event.
            return rule == .autoHide ? armedAt?.addingTimeInterval(Self.revealDelay) : nil
        case let .revealing(from, at): return at.addingTimeInterval((1 - from) * Self.slide)
        case let .shown(leftAt): return rule == .pinned ? nil : leftAt?.addingTimeInterval(Self.hideDelay)
        case let .hiding(from, at): return at.addingTimeInterval(from * Self.slide)
        }
    }
}

extension DockGeometry {
    /// Whether a pointer `fromEdge` points in from the screen edge, at `axis` along the bar,
    /// is in the strip that calls a hidden bar back. It spans the bar's length at rest.
    public func touchesEdge(axis: Double, fromEdge: Double) -> Bool {
        fromEdge <= DockAutoHide.edgeStrip && axis >= 0 && axis <= restLength
    }
}
