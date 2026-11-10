import KairosCore
import XCTest

/// During a locked strict focus phase the active group's rules can block more but never less,
/// whichever way the change arrives: the Rules page (a whole group written through
/// `updateGroup`), Kairos Shield over the bridge (`rule.addSite`, `rule.removeElement`), or
/// Kairos Bar's "Block in <group>".
final class RuleLockTests: XCTestCase {
    private func work() -> AppGroup {
        var g = AppGroup(name: "Work")
        g.blockSites = ["youtube.com"]
        g.allowSites = ["docs.google.com"]
        g.blockedApps = ["com.hnc.Discord"]
        g.hiddenElements = ["news.com": ["#promo"]]
        g.jevEnabled = true
        g.jevThreshold = 0.7
        return g
    }

    private func data() -> AppData {
        var d = AppData()
        d.groups = [work(), AppGroup(name: "Study")]
        d.focus.activeGroupId = d.groups[0].id
        return d
    }

    func testWhatLoosensTheRules() {
        let old = work()
        let looser: [(String, (inout AppGroup) -> Void)] = [
            ("unblocking a site", { $0.blockSites = [] }),
            ("allowing a site", { $0.allowSites.append("reddit.com") }),
            ("unblocking an app", { $0.blockedApps = [] }),
            ("showing a hidden element", { $0.showElement("#promo", on: "news.com") }),
            ("turning Jev off", { $0.jevEnabled = false }),
            ("making Jev slower to block", { $0.jevThreshold = 0.9 }),
            ("swapping a blocked site for another", { $0.blockSites = ["reddit.com"] }),
        ]
        for (what, change) in looser {
            var new = old
            change(&new)
            XCTAssertTrue(RuleLock.loosens(old, to: new), what)
        }
    }

    func testWhatOnlyTightensOrLeavesTheRulesAlone() {
        let old = work()
        let fine: [(String, (inout AppGroup) -> Void)] = [
            ("blocking a site", { $0.blockSites.append("reddit.com") }),
            ("taking a site off the allowed list", { $0.allowSites = [] }),
            ("blocking an app", { $0.blockedApps.append("com.valvesoftware.steam") }),
            ("hiding another element", { $0.hideElement("#related", on: "youtube.com") }),
            ("making Jev quicker to block", { $0.jevThreshold = 0.6 }),
            ("renaming and recolouring", { $0.name = "Deep work"; $0.colorHex = "#FF0000" }),
            ("changing the apps on Kairos Bar", { $0.dockApps = ["com.apple.Safari"] }),
        ]
        for (what, change) in fine {
            var new = old
            change(&new)
            XCTAssertFalse(RuleLock.loosens(old, to: new), what)
        }
        var off = old
        off.jevEnabled = false
        var on = off
        on.jevEnabled = true
        on.jevThreshold = 0.95
        XCTAssertFalse(RuleLock.loosens(off, to: on), "turning Jev on is stricter at any threshold")
    }

    func testTheRulesPageCannotLoosenTheActiveGroup() {
        var d = data()
        let id = d.groups[0].id
        XCTAssertFalse(d.updateGroup(id, strictLocked: true) { $0.blockSites = [] })
        XCTAssertFalse(d.updateGroup(id, strictLocked: true) { $0.allowSites.append("reddit.com") })
        XCTAssertFalse(d.updateGroup(id, strictLocked: true) { $0.jevEnabled = false })
        XCTAssertFalse(d.updateGroup(id, strictLocked: true) { $0.blockSites.append("reddit.com"); $0.blockedApps = [] },
                       "a change that loosens anything is refused as a whole")
        XCTAssertEqual(d.groups[0], work().with(id: id), "nothing changed")

        XCTAssertTrue(d.updateGroup(id, strictLocked: true) { $0.blockSites.append("reddit.com") }, "adding a block is fine")
        XCTAssertTrue(d.updateGroup(id, strictLocked: true) { $0.name = "Deep work" })
        XCTAssertEqual(d.groups[0].blockSites, ["youtube.com", "reddit.com"])

        XCTAssertTrue(d.updateGroup(id, strictLocked: false) { $0.blockSites = [] }, "once the phase ends it is free again")
    }

    func testOtherGroupsStayEditable() {
        var d = data()
        let study = d.groups[1].id
        XCTAssertTrue(d.updateGroup(study, strictLocked: true) { $0.allowSites = ["reddit.com"] },
                      "only the active group's rules are in force")
    }

    func testTheBridgeCannotShowAHiddenElementOrAllowASite() {
        var d = data()
        XCTAssertFalse(d.setElementHidden("#promo", on: "news.com", false, strictLocked: true), "rule.removeElement")
        XCTAssertEqual(d.activeGroup?.hiddenElements["news.com"], ["#promo"])
        XCTAssertTrue(d.setElementHidden("#related", on: "youtube.com", true, strictLocked: true), "rule.addElement")
        XCTAssertFalse(d.addSite("reddit.com", to: .allow, strictLocked: true), "rule.addSite allow")
        XCTAssertTrue(d.addSite("docs.google.com", to: .block, strictLocked: true), "rule.addSite block, even off the allowed list")
        XCTAssertEqual(d.activeGroup?.allowSites, [])
        XCTAssertFalse(d.setAppBlocked("com.hnc.Discord", false, strictLocked: true), "Kairos Bar's Unblock")
    }

    func testTheLineSaysWhenItUnlocks() {
        let at = Calendar.current.date(bySettingHour: 10, minute: 45, second: 0, of: Date())!
        let note = RuleLock.note(until: at)
        XCTAssertTrue(note.hasPrefix("Strict focus until \(at.formatted(date: .omitted, time: .shortened)):"), note)
        XCTAssertTrue(note.contains("block more"))
        XCTAssertTrue(RuleLock.note(until: nil).contains("until this session ends"))
    }
}

private extension AppGroup {
    func with(id: UUID) -> AppGroup {
        var g = self
        g.id = id
        return g
    }
}
