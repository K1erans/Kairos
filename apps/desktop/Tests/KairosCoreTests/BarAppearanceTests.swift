import KairosCore
import XCTest

/// Kairos Bar's look, against the owner's Dock read from a screenshot at 2x: a 16 pt tile in a
/// bar 30 pt thick (7 pt each side), ends rounded to about a third of the thickness.
final class BarAppearanceTests: XCTestCase {
    func testNewAndExistingProfilesAreDark() throws {
        XCTAssertEqual(Settings().barAppearance, .dark)
        let saved = try DataStore.decoder.decode(Settings.self, from: Data(#"{"dockEdge": "bottom", "iconSize": 24}"#.utf8))
        XCTAssertEqual(saved.barAppearance, .dark, "a profile from before the setting moves to Dark")
        let glass = try DataStore.decoder.decode(Settings.self, from: Data(#"{"barAppearance": "glass"}"#.utf8))
        XCTAssertEqual(glass.barAppearance, .glass)
    }

    func testTheChoicesReadAsInSettings() {
        XCTAssertEqual(BarAppearance.allCases.map(\.label), ["Dark (like the Dock)", "Liquid Glass"])
    }

    func testTheOwnersBarIsAboutOnePointSixTimesItsIcons() {
        let g = DockGeometry(slots: [.newGroup], icon: 24)
        XCTAssertEqual(g.padding, 7, "the Dock's 7 pt between icon and edge")
        XCTAssertEqual(g.thickness, 38)
        XCTAssertEqual(g.thickness / g.icon, 1.6, accuracy: 0.05)
        XCTAssertEqual(g.cornerRadius, 13)
    }

    func testTheDocksOwnSizeMatchesTheScreenshot() {
        let g = DockGeometry(slots: [.newGroup], icon: 16)
        XCTAssertEqual(g.thickness, 30, "30 pt thick around a 16 pt tile")
        XCTAssertEqual(g.cornerRadius, 10, "rounded about 10 pt, a third of the thickness")
    }

    func testLargerIconsKeepTheProportions() {
        for icon in stride(from: 20.0, through: 64, by: 2) {
            let g = DockGeometry(slots: [.newGroup], icon: icon)
            XCTAssertGreaterThanOrEqual(g.thickness / icon, 1.38, "\(icon)")
            XCTAssertLessThanOrEqual(g.thickness / icon, 1.7, "\(icon)")
            XCTAssertEqual(g.cornerRadius, (g.thickness / 3).rounded())
        }
    }
}
