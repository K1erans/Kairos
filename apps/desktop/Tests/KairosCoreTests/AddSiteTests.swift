import XCTest
@testable import KairosCore

/// PROTOCOL.md: `rule.addSite` adds the site to the active group's block or allow list and
/// removes it from the other list.
final class AddSiteTests: XCTestCase {
    private func data() -> AppData {
        var d = AppData()
        var g = AppGroup(name: "Work")
        g.blockSites = ["youtube.com"]
        g.allowSites = ["docs.google.com"]
        d.groups = [g, AppGroup(name: "Study")]
        d.focus.activeGroupId = g.id
        return d
    }

    func testBlockingASiteMovesItOffTheAllowList() {
        var d = data()
        XCTAssertTrue(d.addSite("https://www.Docs.Google.com/document/1", to: .block, strictLocked: false))
        XCTAssertEqual(d.activeGroup?.blockSites, ["youtube.com", "docs.google.com"])
        XCTAssertEqual(d.activeGroup?.allowSites, [])
        XCTAssertEqual(d.groups[1].blockSites, [], "only the active group changes")
    }

    func testAlwaysAllowMovesItOffTheBlockList() {
        var d = data()
        XCTAssertTrue(d.addSite("youtube.com", to: .allow, strictLocked: false))
        XCTAssertEqual(d.activeGroup?.blockSites, [])
        XCTAssertEqual(d.activeGroup?.allowSites, ["docs.google.com", "youtube.com"])
    }

    func testAddingWhatIsAlreadyThereChangesNothing() {
        var d = data()
        XCTAssertFalse(d.addSite("youtube.com", to: .block, strictLocked: false))
        XCTAssertFalse(d.addSite("", to: .block, strictLocked: false))
    }

    func testStrictRefusesAnAllowButStillTakesABlock() {
        var d = data()
        XCTAssertFalse(d.addSite("youtube.com", to: .allow, strictLocked: true))
        XCTAssertEqual(d.activeGroup?.blockSites, ["youtube.com"])
        XCTAssertTrue(d.addSite("reddit.com", to: .block, strictLocked: true))
    }

    func testBlockingAnAppFromTheBarAndUnblockingItOutsideStrict() {
        var d = data()
        XCTAssertTrue(d.setAppBlocked("com.hnc.Discord", true, strictLocked: true), "blocking more is fine in Strict")
        XCTAssertEqual(d.activeGroup?.blockedApps, ["com.hnc.Discord"])
        XCTAssertFalse(d.setAppBlocked("com.hnc.Discord", false, strictLocked: true))
        XCTAssertTrue(d.setAppBlocked("com.hnc.Discord", false, strictLocked: false))
        XCTAssertEqual(d.activeGroup?.blockedApps, [])
    }
}
