import XCTest
@testable import KairosCore

/// The request must match TypeSafe's SystemOneRequest and ChoiceQuestion schemas
/// (https://docs.typesafe.ai/api/system-one.md): `model`, `state` and `questions` are required,
/// and a choice question needs `type: "choice"` and 1 to 255 `criteria`.
final class JevRequestTests: XCTestCase {
    private func body(_ subject: JevClient.Subject) -> [String: Any] {
        JevClient.requestBody(task: "Write report", group: "Work", subject: subject)
    }

    func testEveryQuestionIsADocumentedChoiceQuestion() throws {
        var page = JevClient.Subject(kind: "page"); page.url = "https://x.com"; page.title = "X"; page.excerpt = "hi"
        var app = JevClient.Subject(kind: "app"); app.app = "Notes"; app.title = "Draft"
        for subject in [page, app] {
            let b = body(subject)
            XCTAssertTrue(JSONSerialization.isValidJSONObject(b))
            XCTAssertEqual(b["model"] as? String, "jev-latest")
            XCTAssertNotNil(b["state"])
            let questions = try XCTUnwrap(b["questions"] as? [String: [String: Any]])
            XCTAssertFalse(questions.isEmpty)
            for (_, q) in questions {
                XCTAssertEqual(q["type"] as? String, "choice", "ChoiceQuestion.type is required")
                let criteria = try XCTUnwrap(q["criteria"] as? [String: String])
                XCTAssertTrue((1...255).contains(criteria.count))
                XCTAssertEqual(Set(criteria.keys), ["relevant", "distracting", "unsure"])
            }
        }
    }

    func testTheDocumentedChoiceAnswerParses() throws {
        let json = #"{"model":"jev-latest","answers":{"focus":{"type":"choice","choice":"relevant","confidence":0.7,"probabilities":{"relevant":0.8,"distracting":0.15,"unsure":0.05}}},"usage":{"input_tokens":120,"output_tokens":12}}"#
        let v = try JevClient.parse(Data(json.utf8))
        XCTAssertEqual(v.choice, "relevant")
        XCTAssertFalse(v.shouldBlock(threshold: 0.7))
    }
}

final class JevReadinessTests: XCTestCase {
    func testJevThatIsOnButCannotRunSaysWhy() {
        XCTAssertEqual(JevReadiness(enabled: true, hasKey: false, hasTask: true, lastError: nil), .noKey)
        XCTAssertEqual(JevReadiness(enabled: true, hasKey: true, hasTask: false, lastError: nil), .noTask)
        XCTAssertEqual(JevReadiness(enabled: true, hasKey: false, hasTask: false, lastError: nil), .noKey, "the key comes first")
        XCTAssertEqual(JevReadiness(enabled: true, hasKey: true, hasTask: true, lastError: "HTTP 422"), .failing("HTTP 422"))
        for state in [JevReadiness.noKey, .noTask, .failing("HTTP 422")] { XCTAssertNotNil(state.problem) }
    }

    func testNothingToSayWhenItRunsOrIsOff() {
        XCTAssertEqual(JevReadiness(enabled: true, hasKey: true, hasTask: true, lastError: nil), .ready)
        XCTAssertEqual(JevReadiness(enabled: false, hasKey: false, hasTask: false, lastError: "x"), .off)
        XCTAssertNil(JevReadiness.ready.problem)
        XCTAssertNil(JevReadiness.off.problem)
    }
}
