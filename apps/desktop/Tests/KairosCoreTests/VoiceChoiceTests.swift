import XCTest
@testable import KairosCore

/// A voice id belongs to one provider. Carried to the other it makes every sentence fail.
final class VoiceChoiceTests: XCTestCase {
    private func voice(_ provider: VoiceProvider, _ id: String) throws -> String {
        let req = try XCTUnwrap(VoiceClient.request(provider: provider, apiKey: "k", voiceId: id, text: "Hi."))
        if provider == .elevenlabs { return req.url!.pathComponents.last! }
        let body = try XCTUnwrap(try JSONSerialization.jsonObject(with: req.httpBody!) as? [String: Any])
        return try XCTUnwrap(body["voice"] as? String)
    }

    func testChangingProviderForgetsTheOldProvidersVoice() {
        var s = Settings()
        s.setVoiceProvider(.elevenlabs)
        s.voiceId = "21m00Tcm4TlvDq8ikWAM"
        s.setVoiceProvider(.openai)
        XCTAssertEqual(s.voiceId, "")
        s.voiceId = "nova"
        s.setVoiceProvider(.openai)
        XCTAssertEqual(s.voiceId, "nova", "picking the same provider keeps the voice")
    }

    func testAVoiceSavedForTheOtherProviderFallsBackToTheDefault() throws {
        XCTAssertEqual(try voice(.openai, "21m00Tcm4TlvDq8ikWAM"), "alloy")
        XCTAssertEqual(try voice(.openai, "nova"), "nova")
        XCTAssertEqual(try voice(.elevenlabs, "nova"), VoiceClient.elevenLabsDefaultVoice)
        XCTAssertEqual(try voice(.elevenlabs, "pNInz6obpgDQGcFmaJgB"), "pNInz6obpgDQGcFmaJgB")
    }
}
