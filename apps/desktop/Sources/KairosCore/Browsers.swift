import Foundation

/// Web browsers by bundle id. Kairos Shield is a Chrome extension, so only Chromium browsers can
/// run it; site rules never reach any other browser.
public enum Browsers {
    /// In the order the starter groups prefer one.
    public static let chromium = [
        "com.google.Chrome", "com.brave.Browser", "company.thebrowser.Browser", "com.microsoft.edgemac",
        "com.vivaldi.Vivaldi", "org.chromium.Chromium", "com.google.Chrome.beta", "com.google.Chrome.canary",
        "com.operasoftware.Opera", "company.thebrowser.dia",
    ]
    public static let other: Set<String> = [
        "com.apple.Safari", "com.apple.SafariTechnologyPreview", "org.mozilla.firefox", "org.mozilla.firefoxdeveloperedition",
        "org.mozilla.nightly", "app.zen-browser.zen", "com.kagi.kagimacOS", "org.torproject.torbrowser",
        "com.duckduckgo.macos.browser", "org.waterfoxproject.waterfox", "net.mullvad.mullvadbrowser",
    ]

    public static func isChromium(_ bundleId: String) -> Bool { chromium.contains(bundleId) }
    public static func isBrowser(_ bundleId: String) -> Bool { isChromium(bundleId) || other.contains(bundleId) }

    /// The first installed Chromium browser, to pin in place of Safari.
    public static func preferredChromium(installed: (String) -> Bool) -> String? { chromium.first(where: installed) }
}
