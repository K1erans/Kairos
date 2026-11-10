import XCTest
@testable import KairosCore

/// Server-side fallback on a stream, as documented (claude-api skill, shared/model-migration.md,
/// "Echoing fallback turns back"; https://platform.claude.com/docs/en/build-with-claude/refusals-and-fallback):
/// "the retry happens on the same stream and already-received content is never invalidated ...
/// a mid-stream block keeps the partial, marks the boundary with the block, and continues - only
/// the partial's text blocks are passed to the fallback model as continuation context". The
/// `fallback` block arrives as an ordinary content_block_start; there is no special event.
final class SceneFallbackTests: XCTestCase {
    private func parse(_ events: [String]) throws -> ClaudeAPI.StreamParser {
        var parser = ClaudeAPI.StreamParser()
        for data in events {
            try parser.consume(line: "event: \((try JSONSerialization.jsonObject(with: Data(data.utf8)) as! [String: Any])["type"]!)")
            try parser.consume(line: "data: \(data)")
            try parser.consume(line: "")
        }
        return parser
    }

    func testAMidStreamFallbackContinuesThePartialScene() throws {
        let parser = try parse([
            #"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null}}"#,
            #"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            #"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<!doctype html><html><head><title>Heart"}}"#,
            #"{"type":"content_block_stop","index":0}"#,
            #"{"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
            #"{"type":"content_block_stop","index":1}"#,
            #"{"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}"#,
            #"{"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"</title></head><body></body></html>"}}"#,
            #"{"type":"content_block_stop","index":2}"#,
            #"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":40}}"#,
            #"{"type":"message_stop"}"#,
        ])
        XCTAssertEqual(try parser.result(), "<!doctype html><html><head><title>Heart</title></head><body></body></html>")
    }

    func testAFallbackBeforeAnyOutputIsSeamless() throws {
        let parser = try parse([
            #"{"type":"message_start","message":{"id":"msg_2","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"stop_reason":null}}"#,
            #"{"type":"content_block_start","index":0,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
            #"{"type":"content_block_stop","index":0}"#,
            #"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            #"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"<html></html>"}}"#,
            #"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        ])
        XCTAssertEqual(try parser.result(), "<html></html>")
    }

    func testARefusalAfterEveryFallbackDropsThePartial() throws {
        let parser = try parse([
            #"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            #"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<html>"}}"#,
            #"{"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
            #"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}}}"#,
        ])
        XCTAssertThrowsError(try parser.result()) { error in
            guard case ClaudeError.refused("cyber") = error else { return XCTFail("\(error)") }
        }
    }
}
