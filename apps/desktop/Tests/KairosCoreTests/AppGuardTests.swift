import XCTest
@testable import KairosCore

final class AppGuardTests: XCTestCase {
    private var group: AppGroup {
        var g = AppGroup(name: "Work")
        g.blockedApps = ["com.hnc.Discord", "com.spotify.client"]
        g.dockApps = ["com.apple.Notes"]
        return g
    }

    private func sweep(_ apps: [AppGuard.App], mode: Mode = .focus, allowed: Set<String> = []) -> [String] {
        AppGuard.sweep(apps, mode: mode, group: group, isAllowed: { allowed.contains($0) }, own: "com.kairos.desktop")
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
        XCTAssertEqual(AppGuard.sweep(apps, mode: .focus, group: g, isAllowed: { _ in false }, own: "com.kairos.desktop"), [])
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
