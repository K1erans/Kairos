import AppKit
import KairosCore

/// Tracks the running instances hidden by Kairos, so a relaunch or a manually hidden app stays independent.
@MainActor
struct GroupAppVisibility {
    private var tracker = GroupVisibilityTracker<NSRunningApplication>()

    mutating func apply(_ policy: GroupSwitch, to apps: [NSRunningApplication]) {
        let regular = apps
            .filter { $0.activationPolicy == .regular && $0 != .current }
            .map { (app: $0, bundleId: $0.bundleIdentifier, isHidden: $0.isHidden) }
        tracker.apply(policy, to: regular, hide: { $0.hide() }, unhide: { $0.unhide() })
    }

    mutating func didHide(_ app: NSRunningApplication) {
        tracker.didHide(app, unhide: { $0.unhide() })
    }

    mutating func didUnhide(_ app: NSRunningApplication) {
        tracker.didUnhide(app, isHidden: app.isHidden)
    }

    mutating func didTerminate(_ app: NSRunningApplication) {
        tracker.didTerminate(app)
    }

    mutating func restore() {
        tracker.restoreAll(isHidden: { $0.isHidden }, unhide: { $0.unhide() })
    }
}
