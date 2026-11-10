import CoreGraphics
import XCTest
@testable import KairosCore

/// Kairos Bar in a full-screen app's Space, and across Space changes.
final class FullScreenBarTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 2_000_000)
    let away = DockAutoHide.Inputs()
    let onBar = DockAutoHide.Inputs(onBar: true)
    let arrive = DockAutoHide.Inputs(atEdge: true, pushed: true)
    let rest = DockAutoHide.Inputs(atEdge: true)
    let push = DockAutoHide.Inputs(atEdge: true, pushed: true)

    private func run(_ a: inout DockAutoHide, _ steps: [(Double, DockAutoHide.Inputs)], rule: DockAutoHide.Rule) {
        for (t, inputs) in steps { a.update(inputs, rule: rule, now: t0 + t) }
    }

    func testEnteringAFullScreenSpaceTakesTheBarAwayAtOnce() {
        for start in [DockAutoHide.Rule.pinned, .autoHide] {
            var a = DockAutoHide(rule: start, now: t0)
            run(&a, [(0, onBar), (1, away)], rule: start)
            a.update(away, rule: .fullScreen, now: t0 + 1.1)
            XCTAssertEqual(a.state, .hidden(armedAt: nil), "from \(start): gone with the swipe, no slide")
            XCTAssertEqual(a.visible(at: t0 + 1.1), 0)
        }
    }

    func testPinnedHidesInFullScreenAndIsBackOnTheDesktop() {
        var a = DockAutoHide(rule: .pinned, now: t0)
        a.update(away, rule: .fullScreen, now: t0 + 1)
        XCTAssertEqual(a.visible(at: t0 + 1), 0)
        a.update(away, rule: .pinned, now: t0 + 2)
        XCTAssertEqual(a.state, .shown(leftAt: nil))
        XCTAssertEqual(a.visible(at: t0 + 2), 1, "there when the swipe back lands")
        XCTAssertNil(a.nextDecision(), "pinned, nothing to wake for")
    }

    func testRestingAtTheEdgeDoesNotRevealInFullScreen() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        run(&a, [(1, arrive), (1.2, rest), (2, rest), (5, rest)], rule: .fullScreen)
        XCTAssertEqual(a.state, .hidden(armedAt: t0 + 1))
        XCTAssertNil(a.nextDecision(), "only a push brings it out, and a push is an event")
    }

    func testOneLongShoveDoesNotRevealInFullScreen() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        let shove = stride(from: 1.0, through: 2.0, by: 0.01).map { ($0, push) }
        run(&a, shove, rule: .fullScreen)
        XCTAssertEqual(a.visible(at: t0 + 2), 0, "a push that keeps going is still the first push")
    }

    func testASecondPushRevealsInFullScreen() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        run(&a, [(1, arrive), (1.05, push), (1.1, push)], rule: .fullScreen)
        XCTAssertEqual(a.visible(at: t0 + 1.1), 0)
        // A pause against the edge, then push again.
        let again = t0 + 1.1 + DockAutoHide.secondPushPause + 0.01
        a.update(push, rule: .fullScreen, now: again)
        XCTAssertEqual(a.state, .revealing(from: 0, at: again))
        XCTAssertTrue(a.isOut)
    }

    func testLeavingTheEdgeBetweenPushesStartsOver() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        run(&a, [(1, arrive), (1.5, away), (1.6, arrive)], rule: .fullScreen)
        XCTAssertEqual(a.state, .hidden(armedAt: t0 + 1.6), "arriving again is a first push")
    }

    func testRevealedInFullScreenItHidesWhenThePointerLeaves() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        run(&a, [(1, arrive), (2, push)], rule: .fullScreen)
        run(&a, [(2 + DockAutoHide.slide, onBar), (3, onBar)], rule: .fullScreen)
        XCTAssertEqual(a.state, .shown(leftAt: nil))
        run(&a, [(4, away), (4 + DockAutoHide.hideDelay, away)], rule: .fullScreen)
        XCTAssertEqual(a.state, .hiding(from: 1, at: t0 + 4 + DockAutoHide.hideDelay))
    }

    func testTheBarInUseStaysWhenAFullScreenAppComesForward() {
        var a = DockAutoHide(rule: .pinned, now: t0)
        a.update(onBar, rule: .fullScreen, now: t0 + 1)
        XCTAssertEqual(a.state, .shown(leftAt: nil), "not snatched from under the pointer")
    }

    func testASwipeBetweenDesktopsLeavesTheBarAsItWas() {
        // Auto-hide on, pointer on the bar: out before the swipe and after it.
        var shown = DockAutoHide(rule: .autoHide, now: t0)
        run(&shown, [(0, onBar), (1, onBar)], rule: .autoHide)
        let before = shown
        shown.update(onBar, rule: .autoHide, now: t0 + 1.5)
        XCTAssertEqual(shown, before)
        // Auto-hide on, pointer elsewhere: hidden before and after, with nothing scheduled.
        var hidden = DockAutoHide(rule: .autoHide, now: t0)
        run(&hidden, [(0, away), (0.5, away), (0.8, away)], rule: .autoHide)
        XCTAssertEqual(hidden.state, .hidden(armedAt: nil))
        hidden.update(away, rule: .autoHide, now: t0 + 3)
        XCTAssertEqual(hidden.state, .hidden(armedAt: nil))
        // Pinned: out the whole time.
        var pinned = DockAutoHide(rule: .pinned, now: t0)
        for t in stride(from: 0.0, through: 3, by: 0.5) { pinned.update(away, rule: .pinned, now: t0 + t) }
        XCTAssertEqual(pinned.visible(at: t0 + 3), 1)
    }

    func testBetweenAutoHideAndFullScreenAHiddenBarStaysHidden() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        a.update(away, rule: .autoHide, now: t0 + 1)
        XCTAssertEqual(a.state, .hidden(armedAt: nil), "back on a desktop with auto-hide, it does not pop out")
    }

    func testTheRuleFollowsBothSettingsAndTheSpace() {
        typealias R = DockAutoHide.Rule
        // Auto-hide off, full-screen hiding on: pinned on desktops, hidden in full screen.
        XCTAssertEqual(R(autoHide: false, hidesInFullScreen: true, fullScreen: false), .pinned)
        XCTAssertEqual(R(autoHide: false, hidesInFullScreen: true, fullScreen: true), .fullScreen)
        // Both off: always out, over full-screen apps too.
        XCTAssertEqual(R(autoHide: false, hidesInFullScreen: false, fullScreen: false), .pinned)
        XCTAssertEqual(R(autoHide: false, hidesInFullScreen: false, fullScreen: true), .pinned)
        // Auto-hide on, either way: auto-hide on desktops, hidden in full screen.
        for hides in [true, false] {
            XCTAssertEqual(R(autoHide: true, hidesInFullScreen: hides, fullScreen: false), .autoHide)
            XCTAssertEqual(R(autoHide: true, hidesInFullScreen: hides, fullScreen: true), .fullScreen)
        }
    }

    /// Runs the window's loop: an update at every decision time, pointer away.
    private func idle(_ a: inout DockAutoHide, rule: DockAutoHide.Rule, from start: Double, for seconds: Double) {
        var now = t0 + start
        a.update(away, rule: rule, now: now)
        while let next = a.nextDecision(), next <= t0 + start + seconds, next > now {
            now = next
            a.update(away, rule: rule, now: now)
        }
        a.update(away, rule: rule, now: t0 + start + seconds)
    }

    func testPinnedWithFullScreenHidingStaysOnAnEmptyDesktopAndHidesInFullScreen() {
        func rule(_ fullScreen: Bool) -> DockAutoHide.Rule { .init(autoHide: false, hidesInFullScreen: true, fullScreen: fullScreen) }
        var a = DockAutoHide(rule: rule(false), now: t0)
        idle(&a, rule: rule(false), from: 0, for: 60)
        XCTAssertEqual(a.visible(at: t0 + 60), 1, "a minute on an empty desktop with the pointer away: still out")
        idle(&a, rule: rule(true), from: 61, for: 10)
        XCTAssertEqual(a.visible(at: t0 + 71), 0, "gone in the full-screen app")
        run(&a, [(72, arrive), (72.5, push)], rule: rule(true))
        XCTAssertTrue(a.isOut, "a second push brings it out there")
        a.update(away, rule: rule(false), now: t0 + 73)
        XCTAssertEqual(a.state, .shown(leftAt: nil))
        XCTAssertEqual(a.visible(at: t0 + 73), 1, "back on the desktop at once, without touching the edge")
    }

    func testReturningFromFullScreenIsASnapNotASlide() {
        var a = DockAutoHide(rule: .fullScreen, now: t0)
        a.update(away, rule: .pinned, now: t0 + 1)
        XCTAssertFalse(a.isSliding)
        XCTAssertNil(a.nextDecision(), "nothing animates after the swipe lands")
    }

    func testBothOffKeepsTheBarOverFullScreenApps() {
        let rule = DockAutoHide.Rule(autoHide: false, hidesInFullScreen: false, fullScreen: true)
        var a = DockAutoHide(rule: .pinned, now: t0)
        idle(&a, rule: rule, from: 0, for: 30)
        XCTAssertEqual(a.visible(at: t0 + 30), 1)
    }

    func testAutoHideWithFullScreenHidingOffStillKeepsOutOfFullScreen() {
        let desktop = DockAutoHide.Rule(autoHide: true, hidesInFullScreen: false, fullScreen: false)
        let full = DockAutoHide.Rule(autoHide: true, hidesInFullScreen: false, fullScreen: true)
        var a = DockAutoHide(rule: desktop, now: t0)
        idle(&a, rule: desktop, from: 0, for: 5)
        XCTAssertEqual(a.visible(at: t0 + 5), 0, "auto-hide as before on a desktop")
        run(&a, [(6, rest), (6 + DockAutoHide.revealDelay, rest)], rule: desktop)
        XCTAssertTrue(a.isOut, "resting on the edge brings it out on a desktop")
        a.update(away, rule: full, now: t0 + 7)
        XCTAssertEqual(a.visible(at: t0 + 7), 0)
        run(&a, [(8, rest), (9, rest)], rule: full)
        XCTAssertFalse(a.isOut, "resting is not enough in full screen")
        run(&a, [(9.5, push)], rule: full)
        XCTAssertTrue(a.isOut, "the second push is")
    }

    func testFullScreenHidingIsOnForNewAndExistingProfiles() throws {
        XCTAssertTrue(Settings().hidesInFullScreen)
        let saved = try DataStore.decoder.decode(Settings.self, from: Data(#"{"autoHideBar": false}"#.utf8))
        XCTAssertTrue(saved.hidesInFullScreen)
    }

    // MARK: Telling a full-screen Space from the window list

    /// The owner's display: 1470 x 956, a camera housing 32 pt tall, a 34 pt menu bar.
    let display = CGRect(x: 0, y: 0, width: 1470, height: 956)
    /// Read from the owner's Mac on a desktop: the menu bar, a status item, Kairos Bar.
    let menuBar = FullScreenSpace.Window(layer: 24, bounds: CGRect(x: 0, y: 0, width: 1470, height: 33))
    let menuBarItems = FullScreenSpace.Window(layer: 25, bounds: CGRect(x: 1200, y: 0, width: 30, height: 34))
    let kairosBar = FullScreenSpace.Window(layer: 25, bounds: CGRect(x: 438, y: 861, width: 594, height: 95))

    /// In a full-screen Space the menu bar's window is gone and the visible frame reaches the top.
    private func detect(_ front: CGRect, menuBarHidden: Bool, topInset: Double = 32) -> Bool {
        let chrome = menuBarHidden ? [kairosBar] : [menuBar, menuBarItems, kairosBar]
        return FullScreenSpace.detect(windows: chrome + [.init(layer: 0, bounds: front)],
                                      display: display, topInset: topInset, visibleReachesTop: menuBarHidden)
    }

    func testAFullScreenAppBelowTheCameraHousingIsFullScreen() {
        XCTAssertTrue(detect(CGRect(x: 0, y: 32, width: 1470, height: 924), menuBarHidden: true))
    }

    func testAFullScreenAppUsingTheWholeDisplayIsFullScreen() {
        XCTAssertTrue(detect(CGRect(x: 0, y: 0, width: 1470, height: 956), menuBarHidden: true, topInset: 0))
    }

    func testSplitViewIsFullScreen() {
        XCTAssertTrue(detect(CGRect(x: 0, y: 32, width: 731, height: 924), menuBarHidden: true))
    }

    func testAZoomedWindowOnADesktopIsNot() {
        // Read from the owner's Mac: with the real Dock hidden, a zoomed window starts at y 33,
        // inside the housing's 32 pt, and reaches the bottom.
        XCTAssertFalse(detect(CGRect(x: 0, y: 33, width: 1470, height: 923), menuBarHidden: false))
        XCTAssertFalse(detect(CGRect(x: 0, y: 32, width: 1470, height: 924), menuBarHidden: false),
                       "even when it reaches as high as the housing, the menu bar shows on a desktop")
    }

    func testEitherMenuBarSignalIsEnough() {
        let fullScreenApp = FullScreenSpace.Window(layer: 0, bounds: CGRect(x: 0, y: 32, width: 1470, height: 924))
        XCTAssertTrue(FullScreenSpace.detect(windows: [kairosBar, fullScreenApp], display: display, topInset: 32, visibleReachesTop: false),
                      "the menu bar's window is gone though the visible frame has not caught up")
        XCTAssertTrue(FullScreenSpace.detect(windows: [menuBar, kairosBar, fullScreenApp], display: display, topInset: 32, visibleReachesTop: true),
                      "the visible frame reaches the top though the window list still has the menu bar")
    }

    func testAnOrdinaryWindowIsNot() {
        XCTAssertFalse(detect(CGRect(x: 200, y: 120, width: 900, height: 600), menuBarHidden: false))
    }

    func testNoWindowsIsNot() {
        XCTAssertFalse(FullScreenSpace.detect(windows: [menuBar, menuBarItems, kairosBar], display: display, topInset: 32, visibleReachesTop: false))
    }

    func testOnlyTheFrontmostNormalWindowCounts() {
        let small = FullScreenSpace.Window(layer: 0, bounds: CGRect(x: 100, y: 100, width: 400, height: 300))
        let tall = FullScreenSpace.Window(layer: 0, bounds: display)
        XCTAssertFalse(FullScreenSpace.detect(windows: [small, tall], display: display, topInset: 0, visibleReachesTop: true))
        let tooltip = FullScreenSpace.Window(layer: 0, bounds: CGRect(x: 10, y: 10, width: 60, height: 20))
        XCTAssertTrue(FullScreenSpace.detect(windows: [tooltip, tall], display: display, topInset: 0, visibleReachesTop: true),
                      "a tiny helper window in front is skipped")
    }

    func testAWindowOnAnotherDisplayIsNot() {
        let other = FullScreenSpace.Window(layer: 0, bounds: CGRect(x: 1470, y: 0, width: 1920, height: 1080))
        XCTAssertFalse(FullScreenSpace.detect(windows: [other], display: display, topInset: 32, visibleReachesTop: true))
    }

    // The owner's Mac keeps the menu bar showing in full screen, which the window list cannot
    // tell from a zoomed window; the window server's Space type can.
    func testTheWindowServersSpaceTypeDecidesWhenItIsKnown() {
        let id = "37D8832A-2D66-02CA-B9F7-8F30A301B230"
        func managed(_ type: Any?) -> [[String: Any]] {
            [["Display Identifier": "OTHER", "Current Space": ["type": 0]],
             ["Display Identifier": id, "Current Space": type.map { ["type": $0] } ?? [:]]]
        }
        XCTAssertEqual(FullScreenSpace.managedSpaceIsFullScreen(managed(4), display: id.lowercased()), true)
        XCTAssertEqual(FullScreenSpace.managedSpaceIsFullScreen(managed(0), display: id), false)
        XCTAssertNil(FullScreenSpace.managedSpaceIsFullScreen(managed(nil), display: id))
        XCTAssertNil(FullScreenSpace.managedSpaceIsFullScreen(managed(4), display: "missing"))
        XCTAssertEqual(FullScreenSpace.managedSpaceIsFullScreen([["Display Identifier": "Main", "Current Space": ["type": 4]]], display: id), true)
    }
}
