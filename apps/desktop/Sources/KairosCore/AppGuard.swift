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

    /// A Chromium browser that has just opened needs a moment before its Shield connects.
    public static let shieldGrace: TimeInterval = 6

    public enum Reason: Equatable, Sendable {
        /// On the group's blocked apps.
        case listed
        /// A browser Kairos Shield is not running in, where site rules cannot reach.
        case browserWithoutShield
    }

    /// What the guard checks an app against.
    public struct Context {
        public var mode: Mode
        public var group: AppGroup?
        /// Kairos's own bundle id.
        public var own: String?
        /// A Shield client is connected. Its hello does not say which browser it lives in, so it
        /// is taken to cover every Chromium browser.
        public var shieldConnected: Bool
        public var blocksUnshieldedBrowsers: Bool
        public var isAllowed: (String) -> Bool

        public init(mode: Mode, group: AppGroup?, own: String?, shieldConnected: Bool, blocksUnshieldedBrowsers: Bool,
                    isAllowed: @escaping (String) -> Bool) {
            self.mode = mode; self.group = group; self.own = own; self.shieldConnected = shieldConnected
            self.blocksUnshieldedBrowsers = blocksUnshieldedBrowsers; self.isAllowed = isAllowed
        }

        /// Why this app is blocked right now, or nil. Jev is asked separately.
        public func reason(_ bundleId: String) -> Reason? {
            guard mode.blocks, let group, bundleId != own, !neverGuard.contains(bundleId), !isAllowed(bundleId) else { return nil }
            if group.blockedApps.contains(bundleId) { return .listed }
            if blocksUnshieldedBrowsers, Browsers.isBrowser(bundleId), !(Browsers.isChromium(bundleId) && shieldConnected) {
                return .browserWithoutShield
            }
            return nil
        }

        /// The visible apps to hide when blocking starts, the group changes or an "Allow" runs out.
        public func sweep(_ apps: [App]) -> [(bundleId: String, reason: Reason)] {
            apps.compactMap { app in app.visible ? reason(app.bundleId).map { (app.bundleId, $0) } : nil }
        }
    }
}
