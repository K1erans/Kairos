import XCTest
@testable import KairosCore

final class DockFolderTests: XCTestCase {
    func testDownloadsAndTrashComeLastAfterASeparator() {
        var data = AppData.starter()
        data.settings.switchBehavior = .leave
        data.groups[0].dockApps = ["a"]
        data.focus.activeGroupId = data.groups[0].id
        let slots = DockGeometry.slots(for: data, running: ["x"])
        XCTAssertEqual(Array(slots.suffix(6)), [.app("a"), .separator, .running("x"), .separator, .folder(.downloads), .folder(.trash)])
        data.settings.showsDockFolders = false
        XCTAssertFalse(DockGeometry.slots(for: data).contains(.folder(.trash)))
    }

    func testUnpinnedOpenAppsShowEvenWithHideOnSwitchUnlessHiddenBySwitch() {
        var data = AppData.starter()
        data.groups[0].dockApps = ["work", "shared"]
        data.groups[1].dockApps = ["study", "shared"]
        data.settings.showsDockFolders = false
        let running = ["work", "study", "shared", "other"]

        // Default switchBehavior is .hide: opening an unpinned app still shows it on the bar.
        let workOpen = DockGeometry.slots(for: data, running: running)
        XCTAssertEqual(Array(workOpen.dropFirst(data.groups.count + 2)), [.app("work"), .app("shared"), .separator, .running("study"), .running("other")])

        // Apps hidden by Kairos on a group switch stay off the bar, while an unpinned app opened
        // in the active group still appears.
        let workHidden = DockGeometry.slots(for: data, running: running, hidden: ["study"])
        XCTAssertEqual(Array(workHidden.dropFirst(data.groups.count + 2)), [.app("work"), .app("shared"), .separator, .running("other")])

        data.focus = data.focusAfterSwitch(to: data.groups[1].id)
        let studyHidden = DockGeometry.slots(for: data, running: running, hidden: ["work", "other"])
        XCTAssertEqual(Array(studyHidden.dropFirst(data.groups.count + 2)), [.app("study"), .app("shared")])
    }

    func testEveryItemOnTheBarHasItsOwnKeyEvenWithTwoSeparators() {
        let g = DockGeometry(slots: [.app("a"), .separator, .running("x"), .separator, .folder(.trash)], icon: 36)
        let keys = g.layout(pointer: nil).keyed.map(\.key)
        XCTAssertEqual(Set(keys).count, 5)
    }

    func testTheSlotUnderThePointerIsTheOneThatGrowsThere() {
        let g = DockGeometry(slots: [.app("a"), .separator, .folder(.downloads), .folder(.trash)], icon: 36)
        let rest = g.layout(pointer: nil).items
        XCTAssertEqual(g.slot(at: rest[3].center), .folder(.trash))
        XCTAssertEqual(g.slot(at: rest[0].center), .app("a"))
        XCTAssertNil(g.slot(at: rest[1].center), "a separator takes no drops")
    }
}

final class DockDropTests: XCTestCase {
    let doc = URL(fileURLWithPath: "/tmp/report.pdf")
    let pic = URL(fileURLWithPath: "/tmp/photo.jpg")
    let notes = URL(fileURLWithPath: "/Applications/Notes.app")
    private func id(_ url: URL) -> String? { url.pathExtension == "app" ? "com.apple.\(url.deletingPathExtension().lastPathComponent)" : nil }

    func testFilesOnTrashGoToTheTrash() {
        XCTAssertEqual(DockDrop.plan([doc, pic], on: .folder(.trash), bundleId: id), .moveToTrash([doc, pic]))
    }

    func testDocumentsOnAnAppOpenInIt() {
        XCTAssertEqual(DockDrop.plan([doc], on: .app("com.apple.Preview"), bundleId: id), .open([doc], withApp: "com.apple.Preview"))
        XCTAssertEqual(DockDrop.plan([doc, notes], on: .running("com.apple.Preview"), bundleId: id), .open([doc], withApp: "com.apple.Preview"))
    }

    func testAnAppDroppedAnywhereElseJoinsTheGroup() {
        XCTAssertEqual(DockDrop.plan([notes], on: .group(UUID()), bundleId: id), .addApps(["com.apple.Notes"]))
        XCTAssertEqual(DockDrop.plan([notes], on: nil, bundleId: id), .addApps(["com.apple.Notes"]))
        XCTAssertEqual(DockDrop.plan([notes], on: .app("com.apple.Preview"), bundleId: id), .addApps(["com.apple.Notes"]))
    }

    func testDocumentsAnywhereElseDoNothing() {
        XCTAssertEqual(DockDrop.plan([doc], on: .folder(.downloads), bundleId: id), .nothing)
        XCTAssertEqual(DockDrop.plan([doc], on: nil, bundleId: id), .nothing)
        XCTAssertEqual(DockDrop.plan([], on: .folder(.trash), bundleId: id), .nothing)
    }
}

final class DockLaunchTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)

    func testAnAppPulsesUntilItIsRunning() {
        var l = DockLaunches()
        l.start("a", now: t0)
        l.update(running: [], now: t0.addingTimeInterval(2))
        XCTAssertEqual(l.ids, ["a"])
        l.update(running: ["a"], now: t0.addingTimeInterval(3))
        XCTAssertEqual(l.ids, [])
    }

    func testAnAppThatNeverStartsStopsPulsing() {
        var l = DockLaunches()
        l.start("a", now: t0)
        l.update(running: [], now: t0.addingTimeInterval(DockLaunches.timeout))
        XCTAssertEqual(l.ids, [])
    }
}

final class MoveTests: XCTestCase {
    func testMovingStopsAtTheEnds() {
        let apps = ["a", "b", "c"]
        XCTAssertEqual(apps.moving("b", by: -1), ["b", "a", "c"])
        XCTAssertEqual(apps.moving("b", by: 1), ["a", "c", "b"])
        XCTAssertEqual(apps.moving("a", by: -1), apps)
        XCTAssertEqual(apps.moving("c", by: 5), apps)
        XCTAssertEqual(apps.moving("z", by: 1), apps)
    }

    func testDraggedIconGoesToNearestPeerAndStaysThere() {
        let centers = [10.0, 50, 90, 130]
        XCTAssertEqual(DockReorder.index(centers: centers, pointer: -40), 0)
        XCTAssertEqual(DockReorder.index(centers: centers, pointer: 69), 1)
        XCTAssertEqual(DockReorder.index(centers: centers, pointer: 71), 2)
        XCTAssertEqual(DockReorder.index(centers: centers, pointer: 400), 3)
        XCTAssertNil(DockReorder.index(centers: [], pointer: 0))
        XCTAssertEqual(["a", "b", "c", "d"].moving("a", to: 2), ["b", "c", "a", "d"])
        XCTAssertEqual(["a", "b", "c", "d"].moving("d", to: 0), ["d", "a", "b", "c"])
        XCTAssertEqual(["a", "b"].moving("b", to: 1), ["a", "b"])
    }

    func testDraggedAppChangesSectionAtTheLineBetweenThem() {
        let pinned = [100.0, 140, 180], open = [240.0, 280]
        XCTAssertEqual(DockReorder.section(pointer: 60, pinned: pinned, open: open, icon: 36), .pinned(index: 0))
        XCTAssertEqual(DockReorder.section(pointer: 150, pinned: pinned, open: open, icon: 36), .pinned(index: 2))
        XCTAssertEqual(DockReorder.section(pointer: 205, pinned: pinned, open: open, icon: 36), .pinned(index: 3))
        XCTAssertEqual(DockReorder.section(pointer: 215, pinned: pinned, open: open, icon: 36), .open)
        // No other app open: well past the last pinned icon counts as dragging it off.
        XCTAssertEqual(DockReorder.section(pointer: 200, pinned: pinned, open: [], icon: 36), .pinned(index: 3))
        XCTAssertEqual(DockReorder.section(pointer: 210, pinned: pinned, open: [], icon: 36), .open)
    }
}
