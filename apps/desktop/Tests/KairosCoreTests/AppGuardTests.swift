import XCTest
@testable import KairosCore

final class AppGuardTests: XCTestCase {
    private var group: AppGroup {
        var g = AppGroup(name: "Work")
        g.blockedApps = ["com.hnc.Discord", "com.spotify.client"]
        g.dockApps = ["com.apple.Notes"]
        return g
    }

    private func sweep(_ apps: [AppGuard.App], mode: Mode = .focus, allowed: Set<String> = [], group: AppGroup? = nil) -> [String] {
        AppGuard.Context(mode: mode, group: group ?? self.group, own: "com.kairos.desktop", shieldConnected: true,
                         blocksUnshieldedBrowsers: true, isAllowed: { allowed.contains($0) }).sweep(apps).map(\.bundleId)
    }

    func testASweepHidesBlockedAppsThatAreAlreadyOnScreen() {
        let apps = [AppGuard.App(bundleId: "com.hnc.Discord", visible: true), .init(bundleId: "com.apple.Notes", visible: true),
                    .init(bundleId: "com.spotify.client", visible: true)]
        XCTAssertEqual(sweep(apps), ["com.hnc.Discord", "com.spotify.client"])
        XCTAssertEqual(sweep(apps, mode: .strict), ["com.hnc.Discord", "com.spotify.client"])
    }

    func testItLeavesHiddenAllowedAndUnblockedAppsAlone() {
        let apps = [AppGuard.App(bundleId: "com.hnc.Discord", visible: false), .init(bundleId: "com.spotify.client", visible: true)]
        XCTAssertEqual(sweep(apps), ["com.spotify.client"], "an app that is already hidden is left hidden, not hidden again")
        XCTAssertEqual(sweep(apps, allowed: ["com.spotify.client"]), [], "an unexpired Allow wins")
        XCTAssertEqual(sweep(apps, mode: .off), [])
        XCTAssertEqual(sweep(apps, mode: .onBreak), [])
    }

    func testKairosAndTheSystemAreNeverHidden() {
        var g = group
        g.blockedApps += ["com.apple.finder", "com.kairos.desktop"]
        let apps = ["com.apple.finder", "com.kairos.desktop"].map { AppGuard.App(bundleId: $0, visible: true) }
        XCTAssertEqual(sweep(apps, group: g), [])
    }

    func testTheNextGrantToRunOutIsTheEarliestUnexpiredOne() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        var ledger = OverrideLedger()
        XCTAssertNil(ledger.nextExpiry(after: now))
        _ = ledger.request(target: "a", reason: "r", mode: .focus, max: 3, minutes: 5, now: now)
        _ = ledger.request(target: "b", reason: "r", mode: .focus, max: 3, minutes: 5, now: now.addingTimeInterval(60))
        XCTAssertEqual(ledger.nextExpiry(after: now), now.addingTimeInterval(300))
        XCTAssertEqual(ledger.nextExpiry(after: now.addingTimeInterval(301)), now.addingTimeInterval(360))
        XCTAssertNil(ledger.nextExpiry(after: now.addingTimeInterval(400)))
    }
}

/// Site rules only exist inside a browser running Kairos Shield, so in a blocking mode a browser
/// Shield is not in is handled like a blocked app.
final class UnshieldedBrowserTests: XCTestCase {
    private func reason(_ bid: String, shield: Bool, setting: Bool = true, mode: Mode = .focus, allowed: Bool = false,
                        group: AppGroup = AppGroup(name: "Work")) -> AppGuard.Reason? {
        AppGuard.Context(mode: mode, group: group, own: "com.kairos.desktop", shieldConnected: shield,
                         blocksUnshieldedBrowsers: setting, isAllowed: { _ in allowed }).reason(bid)
    }

    func testBrowsersShieldCannotRunInAreBlockedWhileBlocking() {
        for bid in ["com.apple.Safari", "org.mozilla.firefox"] {
            XCTAssertEqual(reason(bid, shield: true), .browserWithoutShield, bid)
            XCTAssertEqual(reason(bid, shield: false), .browserWithoutShield, bid)
        }
    }

    func testAChromiumBrowserIsBlockedOnlyWhileNoShieldIsConnected() {
        for bid in ["com.google.Chrome", "com.brave.Browser", "company.thebrowser.Browser"] {
            XCTAssertEqual(reason(bid, shield: false), .browserWithoutShield, bid)
            XCTAssertNil(reason(bid, shield: true), "any connected Shield is taken to cover every Chromium browser")
        }
    }

    func testTheSettingModeAndAnAllowTurnItOff() {
        XCTAssertNil(reason("com.apple.Safari", shield: false, setting: false))
        XCTAssertNil(reason("com.apple.Safari", shield: false, mode: .off))
        XCTAssertNil(reason("com.apple.Safari", shield: false, mode: .onBreak))
        XCTAssertNil(reason("com.apple.Safari", shield: false, allowed: true))
        XCTAssertNil(reason("com.apple.Notes", shield: false), "not a browser")
    }

    func testAListedBlockSaysSoFirst() {
        var g = AppGroup(name: "Work")
        g.blockedApps = ["com.apple.Safari"]
        XCTAssertEqual(reason("com.apple.Safari", shield: false, group: g), .listed)
    }

    func testTheStarterPinsAnInstalledChromiumBrowserInsteadOfSafari() {
        let brave = AppData.starter(browser: Browsers.preferredChromium(installed: { $0 == "com.brave.Browser" }))
        XCTAssertFalse(brave.groups.contains { $0.dockApps.contains("com.apple.Safari") })
        XCTAssertEqual(brave.groups[0].dockApps[1], "com.brave.Browser", "in Safari's place")
        XCTAssertTrue(brave.groups[1].dockApps.contains("com.brave.Browser"))
        let none = AppData.starter(browser: Browsers.preferredChromium(installed: { _ in false }))
        XCTAssertTrue(none.groups[0].dockApps.contains("com.apple.Safari"), "no Chromium browser: Safari stays")
    }
}
