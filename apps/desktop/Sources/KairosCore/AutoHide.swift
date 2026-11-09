import Foundation

/// Auto-hide for Kairos Bar, after the macOS Dock: it slides off its edge shortly after the
/// pointer leaves, and back after the pointer rests on that screen edge for a moment. Pure, so
/// the window only feeds it the pointer and applies `visible(at:)`.
public struct DockAutoHide: Equatable, Sendable {
    public enum State: Equatable, Sendable {
        /// Off screen. `armedAt` is when the pointer reached the edge, while it stays there.
        case hidden(armedAt: Date?)
        /// Sliding in; `from` is how far out it was at `at` (0 hidden, 1 shown).
        case revealing(from: Double, at: Date)
        /// On screen. `leftAt` is when the pointer left it, while it stays away.
        case shown(leftAt: Date?)
        case hiding(from: Double, at: Date)
    }

    /// What holds the bar out or calls it back.
    public struct Inputs: Equatable, Sendable {
        /// The pointer is in the thin strip along the bar's screen edge.
        public var atEdge: Bool
        /// The pointer is on the bar.
        public var onBar: Bool
        /// A context menu is open, the divider is being dragged, or an app is dragged over it.
        public var held: Bool
        public init(atEdge: Bool = false, onBar: Bool = false, held: Bool = false) {
            self.atEdge = atEdge; self.onBar = onBar; self.held = held
        }
        var keepsOut: Bool { atEdge || onBar || held }
    }

    public static let revealDelay: TimeInterval = 0.2
    public static let hideDelay: TimeInterval = 0.5
    /// Time for the whole slide; a reversal mid-way takes the remaining share.
    public static let slide: TimeInterval = 0.3
    /// Width of the strip along the screen edge that calls the bar back.
    public static let edgeStrip = 2.0

    public private(set) var state: State

    /// Starts out on screen, so turning auto-hide on shows where the bar lives before it goes.
    public init(now: Date) { state = .shown(leftAt: now) }

    public mutating func update(_ inputs: Inputs, now: Date) {
        switch state {
        case let .hidden(armedAt):
            guard inputs.atEdge else { state = .hidden(armedAt: nil); return }
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
        case let .hidden(armedAt): return armedAt?.addingTimeInterval(Self.revealDelay)
        case let .revealing(from, at): return at.addingTimeInterval((1 - from) * Self.slide)
        case let .shown(leftAt): return leftAt?.addingTimeInterval(Self.hideDelay)
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
