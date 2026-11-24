import XCTest
@testable import KairosCore

final class GroupVisibilityTests: XCTestCase {
    private var work: AppGroup {
        var g = AppGroup(name: "Work")
        g.dockApps = ["com.work.Editor"]
        g.blockedApps = ["com.chat.Slack"]
        return g
    }

    private var comms: AppGroup {
        var g = AppGroup(name: "Comms")
        g.dockApps = ["com.chat.Slack"]
        return g
    }

    func testDelayedHideConfirmationStillOwnsAndRestoresApp() {
        var tracker = GroupVisibilityTracker<String>()
        let hideSlack = GroupSwitch(to: work, behavior: .hideBlocked)
        let showSlack = GroupSwitch(to: comms, behavior: .hideBlocked)

        // AppKit returns false from hide() and delivers didHide asynchronously later.
        tracker.apply(
            hideSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: false)],
            hide: { _ in false },
            unhide: { _ in true }
        )
        XCTAssertEqual(tracker.ownership["slack"], .hideRequested)

        // No 1-second timeout: even if didHide lands much later, Kairos owns the hidden state.
        var unhidden: [String] = []
        tracker.didHide("slack", unhide: { unhidden.append($0); return true })
        XCTAssertEqual(tracker.ownership["slack"], .hidden)
        XCTAssertTrue(unhidden.isEmpty)

        // Switching to a group that allows Slack brings it back.
        tracker.apply(
            showSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: true)],
            hide: { _ in true },
            unhide: { unhidden.append($0); return true }
        )
        XCTAssertEqual(unhidden, ["slack"])
        XCTAssertNil(tracker.ownership["slack"])
    }

    func testSwitchingBackBeforeDelayedHideReversesLateDidHide() {
        var tracker = GroupVisibilityTracker<String>()
        let hideSlack = GroupSwitch(to: work, behavior: .hideBlocked)
        let showSlack = GroupSwitch(to: comms, behavior: .hideBlocked)

        tracker.apply(
            hideSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: false)],
            hide: { _ in false },
            unhide: { _ in true }
        )
        XCTAssertEqual(tracker.ownership["slack"], .hideRequested)

        // User switches to Comms before AppKit finishes hiding Slack.
        var unhidden: [String] = []
        tracker.apply(
            showSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: false)],
            hide: { _ in true },
            unhide: { unhidden.append($0); return true }
        )
        XCTAssertEqual(tracker.ownership["slack"], .restoreRequested)
        XCTAssertTrue(unhidden.isEmpty)

        // The stale hide lands afterwards: tracker immediately reverses it.
        tracker.didHide("slack", unhide: { unhidden.append($0); return true })
        XCTAssertEqual(unhidden, ["slack"])
        XCTAssertNil(tracker.ownership["slack"])
    }

    func testRestoreAllBeforeDelayedHideReversesLateDidHide() {
        var tracker = GroupVisibilityTracker<String>()
        let hideSlack = GroupSwitch(to: work, behavior: .hideBlocked)

        tracker.apply(
            hideSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: false)],
            hide: { _ in false },
            unhide: { _ in true }
        )
        var unhidden: [String] = []
        tracker.restoreAll(isHidden: { _ in false }, unhide: { unhidden.append($0); return true })
        XCTAssertEqual(tracker.ownership["slack"], .restoreRequested)

        tracker.didHide("slack", unhide: { unhidden.append($0); return true })
        XCTAssertEqual(unhidden, ["slack"])
        XCTAssertNil(tracker.ownership["slack"])
    }

    func testManuallyHiddenAppStaysUnownedAndUntouched() {
        var tracker = GroupVisibilityTracker<String>()
        let hideSlack = GroupSwitch(to: work, behavior: .hideBlocked)
        let showSlack = GroupSwitch(to: comms, behavior: .hideBlocked)

        var unhidden: [String] = []
        tracker.didHide("slack", unhide: { unhidden.append($0); return true })
        XCTAssertNil(tracker.ownership["slack"])

        tracker.apply(
            hideSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: true)],
            hide: { _ in XCTFail("already hidden"); return true },
            unhide: { unhidden.append($0); return true }
        )
        XCTAssertNil(tracker.ownership["slack"])

        tracker.apply(
            showSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: true)],
            hide: { _ in true },
            unhide: { unhidden.append($0); return true }
        )
        XCTAssertTrue(unhidden.isEmpty, "manually hidden app must not be unhidden on group switch")
    }

    func testFailedUnhideRemainsOwnedAndRetries() {
        var tracker = GroupVisibilityTracker<String>()
        let hideSlack = GroupSwitch(to: work, behavior: .hideBlocked)
        let showSlack = GroupSwitch(to: comms, behavior: .hideBlocked)

        tracker.apply(
            hideSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: false)],
            hide: { _ in true },
            unhide: { _ in true }
        )
        XCTAssertEqual(tracker.ownership["slack"], .hidden)

        // First unhide attempt fails.
        tracker.apply(
            showSlack,
            to: [(app: "slack", bundleId: "com.chat.Slack", isHidden: true)],
            hide: { _ in true },
            unhide: { _ in false }
        )
        XCTAssertEqual(tracker.ownership["slack"], .restoreRequested)

        // Retry succeeds and clears ownership.
        var retried = false
        tracker.restoreAll(isHidden: { _ in true }, unhide: { _ in retried = true; return true })
        XCTAssertTrue(retried)
        XCTAssertNil(tracker.ownership["slack"])
    }

    func testIsHiddenBySwitchTracksOnlyKairosHiddenApps() {
        var tracker = GroupVisibilityTracker<String>()
        let hideOthers = GroupSwitch(to: work, behavior: .hide)

        XCTAssertFalse(tracker.isHiddenBySwitch("notes"))
        tracker.apply(
            hideOthers,
            to: [(app: "notes", bundleId: "com.apple.Notes", isHidden: false)],
            hide: { _ in true },
            unhide: { _ in true }
        )
        XCTAssertTrue(tracker.isHiddenBySwitch("notes"))

        // User re-opens / unhides the app: no longer hidden by switch.
        tracker.didUnhide("notes", isHidden: false)
        XCTAssertFalse(tracker.isHiddenBySwitch("notes"))

        // Manual hide is not owned by the switch tracker.
        tracker.didHide("notes", unhide: { _ in true })
        XCTAssertFalse(tracker.isHiddenBySwitch("notes"))
    }

    func testSwitchingToStudyAndBackToPersonalRestoresUngroupedOpenApps() {
        var study = AppGroup(name: "Study")
        study.dockApps = ["com.study.Reader"]
        var personal = AppGroup(name: "Personal")
        personal.dockApps = ["com.chat.Discord"]

        var tracker = GroupVisibilityTracker<String>()
        let toStudy = GroupSwitch(to: study, behavior: .hide, otherGroupApps: Set(personal.dockApps))
        let toPersonal = GroupSwitch(to: personal, behavior: .hide, otherGroupApps: Set(study.dockApps))

        // Leaving Personal for Study hides both Personal's app and ungrouped Spotify.
        var hidden: Set<String> = []
        tracker.apply(
            toStudy,
            fromGroup: personal.id,
            to: [
                (app: "discord", bundleId: "com.chat.Discord", isHidden: false),
                (app: "spotify", bundleId: "com.spotify.client", isHidden: false),
                (app: "reader", bundleId: "com.study.Reader", isHidden: false),
            ],
            hide: { hidden.insert($0); return true },
            unhide: { hidden.remove($0); return true }
        )
        XCTAssertEqual(hidden, ["discord", "spotify"])
        XCTAssertTrue(tracker.isHiddenBySwitch("spotify"))

        // Switching back to Personal restores both Discord and ungrouped Spotify, and hides Reader.
        tracker.apply(
            toPersonal,
            fromGroup: study.id,
            to: [
                (app: "discord", bundleId: "com.chat.Discord", isHidden: true),
                (app: "spotify", bundleId: "com.spotify.client", isHidden: true),
                (app: "reader", bundleId: "com.study.Reader", isHidden: false),
            ],
            hide: { hidden.insert($0); return true },
            unhide: { hidden.remove($0); return true }
        )
        XCTAssertEqual(hidden, ["reader"])
        XCTAssertFalse(tracker.isHiddenBySwitch("spotify"))
        XCTAssertFalse(tracker.isHiddenBySwitch("discord"))
        XCTAssertTrue(tracker.isHiddenBySwitch("reader"))
    }
}
