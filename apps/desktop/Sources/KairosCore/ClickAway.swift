import Foundation

/// Closes a box on any click outside it, the bare desktop included, from when it opens until it
/// closes. The window installs mouse-down monitors through `install`, which reports each click
/// with whether it landed in the box; the monitors exist only while the box is open.
public final class ClickAway<Monitor> {
    public typealias Install = (_ report: @escaping (_ inside: Bool) -> Void) -> [Monitor]

    private let install: Install
    private let remove: (Monitor) -> Void
    private var monitors: [Monitor] = []
    private var onAway: (() -> Void)?

    public init(install: @escaping Install, remove: @escaping (Monitor) -> Void) {
        self.install = install
        self.remove = remove
    }

    public var isWatching: Bool { onAway != nil }

    /// Starts watching; `onAway` runs once, on the first click outside.
    public func start(onAway: @escaping () -> Void) {
        stop()
        self.onAway = onAway
        monitors = install { [weak self] inside in
            guard !inside, let self, let away = self.onAway else { return }
            self.stop()
            away()
        }
    }

    /// Removes the monitors. Safe to call when not watching.
    public func stop() {
        monitors.forEach(remove)
        monitors = []
        onAway = nil
    }
}
