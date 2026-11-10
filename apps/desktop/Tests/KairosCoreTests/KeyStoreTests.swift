import KairosCore
import XCTest

/// A scratch copy (KAIROS_HOME) must never read the owner's Keychain or make macOS ask for it.
final class KeyStoreTests: XCTestCase {
    func testTheRealProfileUsesTheKeychain() {
        XCTAssertEqual(KeyStore.at(environment: [:]), .keychain)
        XCTAssertEqual(KeyStore.at(environment: ["KAIROS_HOME": ""]), .keychain)
    }

    func testAScratchCopyKeepsKeysInItsScratchFolder() {
        XCTAssertEqual(KeyStore.at(environment: ["KAIROS_HOME": "/tmp/kairos-check-1"]),
                       .file(URL(fileURLWithPath: "/tmp/kairos-check-1/keys.json")))
    }

    func testScratchKeysRoundTrip() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let keys = ScratchKeys(url: dir.appendingPathComponent("keys.json"))
        XCTAssertNil(keys.get("anthropic"), "no file yet")
        XCTAssertTrue(keys.set("sk-test", for: "anthropic"))
        XCTAssertTrue(keys.set("ts-test", for: "typesafe"))
        XCTAssertEqual(keys.get("anthropic"), "sk-test")
        XCTAssertTrue(keys.set("", for: "anthropic"))
        XCTAssertNil(keys.get("anthropic"), "an empty value removes it")
        XCTAssertEqual(keys.get("typesafe"), "ts-test")
    }
}
