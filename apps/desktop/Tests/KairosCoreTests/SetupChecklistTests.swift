import XCTest
@testable import KairosCore

final class SetupChecklistTests: XCTestCase {
    func testANewMacHasEverythingLeftToDo() {
        let list = SetupChecklist(SetupFacts())
        XCTAssertEqual(list.items.map(\.step), [.shield, .read, .jevKey, .claudeKey, .accessibility, .openAtLogin, .dockReplacement])
        XCTAssertEqual(list.doneCount, 0)
        XCTAssertFalse(list.isComplete)
    }

    func testAVoiceKeyIsAskedForOnlyWhenACloudVoiceIsChosen() {
        var facts = SetupFacts()
        XCTAssertFalse(SetupChecklist(facts).items.contains { $0.step == .voiceKey })
        facts.voiceProvider = .elevenlabs
        let item = SetupChecklist(facts).items.first { $0.step == .voiceKey }
        XCTAssertEqual(item?.title, "Add your ElevenLabs key")
        XCTAssertEqual(item?.done, false)
        facts.hasVoiceKey = true
        XCTAssertEqual(SetupChecklist(facts).items.first { $0.step == .voiceKey }?.done, true)
    }

    func testAnExtensionCountsOnceItHasConnected() {
        var facts = SetupFacts()
        facts.seenClients = ["shield"]
        let list = SetupChecklist(facts)
        XCTAssertEqual(list.items.first { $0.step == .shield }?.done, true)
        XCTAssertEqual(list.items.first { $0.step == .read }?.done, false)
    }

    func testEverythingDoneCompletesTheList() {
        var facts = SetupFacts()
        facts.seenClients = ["shield", "read"]
        facts.hasJevKey = true; facts.hasClaudeKey = true
        facts.voiceProvider = .openai; facts.hasVoiceKey = true
        facts.accessibility = true; facts.openAtLogin = true; facts.replacesDock = true
        let list = SetupChecklist(facts)
        XCTAssertTrue(list.isComplete)
        XCTAssertEqual(list.doneCount, 8)
    }

    func testSeenExtensionsAndHidingTheListSurviveARelaunch() throws {
        var data = AppData()
        data.seenClients = ["shield"]
        data.setupHidden = true
        let back = try DataStore.decoder.decode(AppData.self, from: DataStore.encoder.encode(data))
        XCTAssertEqual(back.seenClients, ["shield"])
        XCTAssertTrue(back.setupHidden)
    }
}
