import Foundation

/// Decides which apps the app guard hides. Pure, so the rules are testable without running apps;
/// the app feeds it what NSWorkspace reports.
public enum AppGuard {
    /// Hiding these would lock the owner out of their Mac or out of Kairos itself.
    public static let neverGuard: Set<String> = [
        "com.apple.finder", "com.apple.dock", "com.apple.systempreferences", "com.apple.loginwindow",
        "com.apple.Spotlight", "com.apple.controlcenter", "com.apple.notificationcenterui", "com.apple.SecurityAgent",
        "com.apple.Terminal", "com.apple.ActivityMonitor",
    ]

    public struct App: Equatable, Sendable {
        public var bundleId: String
        /// On screen: not hidden by the owner or by Kairos.
        public var visible: Bool
        public init(bundleId: String, visible: Bool) { self.bundleId = bundleId; self.visible = visible }
    }

    /// Whether the group's lists block this app right now. Jev is asked separately.
    public static func blocks(_ bundleId: String, mode: Mode, group: AppGroup?, isAllowed: (String) -> Bool, own: String?) -> Bool {
        guard mode.blocks, let group, bundleId != own, !neverGuard.contains(bundleId), !isAllowed(bundleId) else { return false }
        return group.blockedApps.contains(bundleId)
    }

    /// The visible apps to hide when blocking starts, the group changes or an "Allow" runs out.
    public static func sweep(_ apps: [App], mode: Mode, group: AppGroup?, isAllowed: (String) -> Bool, own: String?) -> [String] {
        apps.filter { $0.visible && blocks($0.bundleId, mode: mode, group: group, isAllowed: isAllowed, own: own) }.map(\.bundleId)
    }
}
