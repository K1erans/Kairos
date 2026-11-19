import Foundation

/// Explicit per-app ownership state machine for group-switch window visibility.
///
/// Tracks only apps Kairos asked macOS to hide so manually hidden apps stay untouched,
/// and reconciles delayed hide/unhide notifications without relying on wall-clock timeouts.
public struct GroupVisibilityTracker<App: Hashable> {
    public enum Ownership: Equatable, Sendable {
        /// Kairos asked macOS to hide this app and is waiting for hide confirmation.
        case hideRequested
        /// Kairos owns this app's hidden state.
        case hidden
        /// Kairos asked to restore this app while a hide was pending or after unhide failed.
        case restoreRequested
    }

    public private(set) var ownership: [App: Ownership] = [:]

    public init() {}

    /// Reconciles visibility against a group switch policy.
    public mutating func apply(
        _ policy: GroupSwitch,
        to apps: [(app: App, bundleId: String?, isHidden: Bool)],
        hide: (App) -> Bool,
        unhide: (App) -> Bool
    ) {
        let live = Set(apps.map(\.app))
        ownership = ownership.filter { live.contains($0.key) }

        var toRestore: [(app: App, isHidden: Bool)] = []
        for entry in apps {
            let owned = ownership[entry.app] != nil
            switch policy.action(for: entry.bundleId, isHidden: entry.isHidden, hiddenBySwitch: owned) {
            case .hide:
                ownership[entry.app] = hide(entry.app) ? .hidden : .hideRequested
            case .unhide:
                toRestore.append((entry.app, entry.isHidden))
            case nil:
                // Policy wants the app hidden and it is already hidden: if Kairos requested
                // the hide (or had a failed restore), confirm hidden ownership.
                if entry.isHidden, let state = ownership[entry.app],
                   state == .hideRequested || state == .restoreRequested {
                    ownership[entry.app] = .hidden
                }
            }
        }
        for entry in toRestore {
            restoreApp(entry.app, isHidden: entry.isHidden, unhide: unhide)
        }
    }

    /// Reconciles a workspace `didHide` notification.
    public mutating func didHide(_ app: App, unhide: (App) -> Bool) {
        switch ownership[app] {
        case .hideRequested?:
            ownership[app] = .hidden
        case .restoreRequested?:
            if unhide(app) {
                ownership.removeValue(forKey: app)
            } else {
                ownership[app] = .hidden
            }
        case .hidden?, nil:
            break
        }
    }

    /// Reconciles a workspace `didUnhide` notification.
    public mutating func didUnhide(_ app: App, isHidden: Bool) {
        guard !isHidden else { return }
        switch ownership[app] {
        case .hidden?, .restoreRequested?:
            ownership.removeValue(forKey: app)
        case .hideRequested?, nil:
            break
        }
    }

    /// Drops ownership when an app terminates.
    public mutating func didTerminate(_ app: App) {
        ownership.removeValue(forKey: app)
    }

    /// Restores all apps currently owned by Kairos (e.g. on shutdown or when disabling hiding).
    public mutating func restoreAll(
        isHidden: (App) -> Bool,
        unhide: (App) -> Bool
    ) {
        for app in Array(ownership.keys) {
            restoreApp(app, isHidden: isHidden(app), unhide: unhide)
        }
    }

    private mutating func restoreApp(_ app: App, isHidden: Bool, unhide: (App) -> Bool) {
        switch ownership[app] {
        case .hideRequested?, .restoreRequested?:
            if isHidden {
                if unhide(app) {
                    ownership.removeValue(forKey: app)
                } else {
                    ownership[app] = .restoreRequested
                }
            } else {
                // Hide has not landed yet; stay in `.restoreRequested` so a late `didHide` is reversed.
                ownership[app] = .restoreRequested
            }
        case .hidden?:
            if isHidden {
                if unhide(app) {
                    ownership.removeValue(forKey: app)
                } else {
                    ownership[app] = .restoreRequested
                }
            } else {
                ownership.removeValue(forKey: app)
            }
        case nil:
            break
        }
    }
}
