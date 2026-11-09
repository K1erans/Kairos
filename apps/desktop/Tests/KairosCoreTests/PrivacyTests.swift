import XCTest
@testable import KairosCore

/// PROTOCOL.md: `neverAnalyse` applies to every path that sends page content off the Mac, and the
/// app answers `error {id, message}` for a listed host.
final class PrivacyTests: XCTestCase {
    private let list = ["mail.google.com", "mybank.com"]

    func testVoiceAndScenesFromAListedPageAreRefused() {
        for type in ["tts.speak", "tts.prefetch", "scene.candidates", "scene.generate", "scene.edit"] {
            let message: [String: Any] = ["type": type, "id": "1", "text": "Your statement is ready.", "url": "https://www.mybank.com/statements"]
            let refusal = Privacy.refusal(message, neverAnalyse: list)
            XCTAssertNotNil(refusal, type)
            XCTAssertTrue(refusal?.contains("mybank.com") == true, "the message names the site")
        }
        let mail: [String: Any] = ["type": "tts.speak", "text": "Hi", "url": "https://mail.google.com/mail/u/0/#inbox"]
        XCTAssertNotNil(Privacy.refusal(mail, neverAnalyse: list))
    }

    func testOtherPagesAndOtherRequestsGoThrough() {
        XCTAssertNil(Privacy.refusal(["type": "tts.speak", "text": "Hi", "url": "https://en.wikipedia.org/wiki/Kairos"], neverAnalyse: list))
        XCTAssertNil(Privacy.refusal(["type": "tts.speak", "text": "Hi", "url": "https://notmybank.com/"], neverAnalyse: list),
                     "a host only matches the entry or its subdomains")
        XCTAssertNil(Privacy.refusal(["type": "read.log", "url": "https://mybank.com/"], neverAnalyse: list), "nothing leaves the Mac")
        XCTAssertNil(Privacy.refusal(["type": "tts.speak", "text": "Hi"], neverAnalyse: list), "no url to check")
    }

    func testJevIsNotToldABrowsersPageTitle() {
        let chrome = Privacy.appSubject(name: "Google Chrome", bundleId: "com.google.Chrome", windowTitle: "Inbox (3) - me@gmail.com - Gmail")
        XCTAssertEqual(chrome.app, "Google Chrome")
        XCTAssertEqual(chrome.title, "")
        XCTAssertEqual(Privacy.appSubject(name: "Safari", bundleId: "com.apple.Safari", windowTitle: "My Bank").title, "")
        XCTAssertEqual(Privacy.appSubject(name: "Notes", bundleId: "com.apple.Notes", windowTitle: "Draft").title, "Draft")
    }
}
