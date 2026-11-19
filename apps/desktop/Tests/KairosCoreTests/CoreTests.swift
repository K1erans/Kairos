import XCTest
@testable import KairosCore

final class QuickAddTests: XCTestCase {
    var cal: Calendar { var c = Calendar(identifier: .gregorian); c.timeZone = TimeZone(identifier: "UTC")!; return c }
    // Monday 9 Nov 2026, 10:00 UTC
    var now: Date { cal.date(from: DateComponents(year: 2026, month: 11, day: 9, hour: 10))! }
    let groups = ["Work", "Study"]

    private func parse(_ s: String) -> ParsedTask { QuickAdd.parse(s, groups: groups, now: now, calendar: cal) }
    private func day(_ d: Int, _ h: Int = 17, _ m: Int = 0) -> Date? {
        cal.date(from: DateComponents(year: 2026, month: 11, day: d, hour: h, minute: m))
    }

    func testShortWeekdaysAreWordsInsideTheTitle() {
        XCTAssertEqual(parse("Buy sun hat").title, "Buy sun hat")
        XCTAssertNil(parse("Buy sun hat").due)
        let sat = parse("Research sat scores friday")
        XCTAssertEqual(sat.title, "Research sat scores")
        XCTAssertEqual(sat.due, day(13), "full weekday names always count")
    }

    func testShortWeekdaysCountAtTheEndOrBeforeATime() {
        XCTAssertEqual(parse("Call mom sun").due, day(15))
        XCTAssertEqual(parse("Call mom sun").title, "Call mom")
        XCTAssertEqual(parse("Call mom sun @Work !high").due, day(15), "only tokens follow it")
        XCTAssertEqual(parse("Gym sat 9am then swim").due, day(14, 9))
        XCTAssertEqual(parse("Gym sat 9am then swim").title, "Gym then swim")
    }

    func testDurationSpellings() {
        XCTAssertEqual(parse("a ~45min").estimateMin, 45)
        XCTAssertEqual(parse("a ~45mins").estimateMin, 45)
        XCTAssertEqual(parse("a ~2hr").estimateMin, 120)
        XCTAssertEqual(parse("a ~2hrs").estimateMin, 120)
        XCTAssertEqual(parse("a ~1h30").estimateMin, 90)
        XCTAssertEqual(parse("a ~1h30m").estimateMin, 90)
        XCTAssertEqual(parse("a ~90").estimateMin, 90)
        XCTAssertNil(parse("a ~h").estimateMin)
        XCTAssertNil(parse("a ~").estimateMin)
        XCTAssertEqual(parse("a ~soon").title, "a ~soon")
    }

    func testUnknownGroupStaysInTheTitle() {
        let p = parse("Plan trip @Holiday")
        XCTAssertEqual(p.title, "Plan trip @Holiday")
        XCTAssertNil(p.groupName)
        XCTAssertEqual(parse("Plan trip @work").groupName, "Work", "known groups match in any case")
    }

    func testParsesAllFiveTokens() {
        let p = QuickAdd.parse("Draft blog post tomorrow 3pm @Work ~45m !high", groups: groups, now: now, calendar: cal)
        XCTAssertEqual(p.title, "Draft blog post")
        XCTAssertEqual(p.groupName, "Work")
        XCTAssertEqual(p.estimateMin, 45)
        XCTAssertEqual(p.priority, 3)
        XCTAssertEqual(p.due, cal.date(from: DateComponents(year: 2026, month: 11, day: 10, hour: 15)))
    }

    func testPlainTitleKeepsEverything() {
        let p = QuickAdd.parse("Buy 3 apples", groups: groups, now: now, calendar: cal)
        XCTAssertEqual(p.title, "Buy 3 apples")
        XCTAssertNil(p.due); XCTAssertNil(p.estimateMin); XCTAssertEqual(p.priority, 0)
    }

    func testWeekdayAndDurations() {
        let p = QuickAdd.parse("Review fri 9:30am ~1h30m #Study !low", groups: groups, now: now, calendar: cal)
        XCTAssertEqual(p.title, "Review")
        XCTAssertEqual(p.estimateMin, 90)
        XCTAssertEqual(p.groupName, "Study")
        XCTAssertEqual(p.due, cal.date(from: DateComponents(year: 2026, month: 11, day: 13, hour: 9, minute: 30)))
    }

    func testBareTimeInThePastRollsToTomorrow() {
        let p = QuickAdd.parse("Call 8am", groups: groups, now: now, calendar: cal)
        XCTAssertEqual(p.due, cal.date(from: DateComponents(year: 2026, month: 11, day: 10, hour: 8)))
    }

    func testSameWeekdayMeansNextWeek() {
        let p = QuickAdd.parse("Plan monday", groups: groups, now: now, calendar: cal)
        XCTAssertEqual(p.due, cal.date(from: DateComponents(year: 2026, month: 11, day: 16, hour: 17)))
    }
}

final class TempoTests: XCTestCase {
    let t0 = Date(timeIntervalSince1970: 1_000_000)

    func testFullCycleAndLongBreak() {
        var e = TempoEngine(preset: .init(name: "T", focusMin: 25, shortBreakMin: 5, longBreakMin: 15, cyclesBeforeLong: 2))
        e.start(now: t0)
        XCTAssertEqual(e.phase, .focus)
        XCTAssertEqual(e.remaining(now: t0.addingTimeInterval(60)), 24 * 60)
        var events = e.tick(now: t0.addingTimeInterval(25 * 60))
        XCTAssertEqual(e.phase, .shortBreak)
        XCTAssertEqual(events.first, .finished(phase: .focus, start: t0, end: t0.addingTimeInterval(1500), duration: 1500, interruptions: 0, completed: true))
        events = e.tick(now: t0.addingTimeInterval(30 * 60))
        XCTAssertEqual(e.phase, .focus)
        _ = e.tick(now: t0.addingTimeInterval(55 * 60))
        XCTAssertEqual(e.phase, .longBreak)
    }

    func testSleepPastTheEndStillEndsOnTime() {
        var e = TempoEngine()
        e.start(now: t0)
        let events = e.tick(now: t0.addingTimeInterval(3 * 3600)) // woke hours later
        guard case let .finished(_, _, end, duration, _, completed)? = events.first else { return XCTFail() }
        XCTAssertEqual(end, t0.addingTimeInterval(1500))
        XCTAssertEqual(duration, 1500)
        XCTAssertTrue(completed)
    }

    func testPauseResumeExtendAndSkip() {
        var e = TempoEngine()
        e.start(now: t0)
        e.pause(now: t0.addingTimeInterval(600))
        XCTAssertEqual(e.remaining(now: t0.addingTimeInterval(9999)), 900)
        XCTAssertTrue(e.tick(now: t0.addingTimeInterval(9999)).isEmpty)
        e.resume(now: t0.addingTimeInterval(1000))
        XCTAssertEqual(e.endsAt, t0.addingTimeInterval(1900))
        e.extend(minutes: 5, now: t0.addingTimeInterval(1000))
        XCTAssertEqual(e.endsAt, t0.addingTimeInterval(2200))
        XCTAssertEqual(e.interruptions, 1)
        let events = e.skip(now: t0.addingTimeInterval(1100))
        XCTAssertEqual(events.first, .finished(phase: .focus, start: t0, end: t0.addingTimeInterval(1100), duration: 700, interruptions: 2, completed: false))
        XCTAssertEqual(e.phase, .shortBreak)
    }
}

final class RulesTests: XCTestCase {
    func testHostMatching() {
        XCTAssertTrue(Rules.hostMatches("www.youtube.com", entry: "youtube.com"))
        XCTAssertTrue(Rules.hostMatches("m.youtube.com", entry: "https://YouTube.com/feed"))
        XCTAssertFalse(Rules.hostMatches("notyoutube.com", entry: "youtube.com"))
        XCTAssertFalse(Rules.hostMatches("youtube.com", entry: ""))
        XCTAssertEqual(Rules.hostOf(urlString: "https://www.reddit.com/r/swift?x=1"), "reddit.com")
    }

    func testOverrideLedger() {
        var l = OverrideLedger()
        let now = Date(timeIntervalSince1970: 0)
        XCTAssertEqual(l.request(target: "x.com", reason: "", mode: .focus, max: 3, minutes: 5, now: now), .failure(.noReason))
        XCTAssertEqual(l.request(target: "x.com", reason: "need a link", mode: .strict, max: 3, minutes: 5, now: now), .failure(.strict))
        XCTAssertEqual(l.request(target: "x.com", reason: "need a link", mode: .focus, max: 1, minutes: 5, now: now), .success(now.addingTimeInterval(300)))
        XCTAssertTrue(l.isAllowed("mobile.x.com", now: now.addingTimeInterval(299)))
        XCTAssertFalse(l.isAllowed("x.com", now: now.addingTimeInterval(301)))
        XCTAssertEqual(l.request(target: "y.com", reason: "again", mode: .focus, max: 1, minutes: 5, now: now), .failure(.exhausted))
        l.reset()
        XCTAssertEqual(l.remaining(max: 1), 1)
    }

    func testStatePayloadIsValidJSONWithNulls() throws {
        var data = AppData.starter()
        data.focus.mode = .focus
        let payload = BridgeState.payload(data: data, ledger: OverrideLedger())
        XCTAssertTrue(JSONSerialization.isValidJSONObject(payload))
        XCTAssertTrue(payload["currentTask"] is NSNull)
        XCTAssertEqual(payload["mode"] as? String, "focus")
        XCTAssertEqual(payload["overridesLeft"] as? Int, 3)
        let rules = try XCTUnwrap(payload["rules"] as? [String: Any])
        XCTAssertEqual((rules["blockSites"] as? [String])?.contains("youtube.com"), true)
    }

    func testSmartNextPrefersDueThenPriority() {
        let g = UUID()
        var a = TaskItem(title: "a", groupId: g); a.priority = 3
        var b = TaskItem(title: "b", groupId: g); b.due = Date(timeIntervalSince1970: 10)
        var c = TaskItem(title: "c", groupId: g); c.due = Date(timeIntervalSince1970: 5); c.estimateMin = 120
        XCTAssertEqual(SmartNext.suggest(tasks: [a, b, c], groupId: g)?.title, "c")
        XCTAssertEqual(SmartNext.suggest(tasks: [a, b, c], groupId: g, availableMin: 30)?.title, "b")
        XCTAssertEqual(SmartNext.suggest(tasks: [a], groupId: UUID())?.title, nil)
    }
}

final class BridgeTests: XCTestCase {
    func testFramingRoundTripAndPartialFrames() {
        let a = NativeFraming.frame(Data(#"{"a":1}"#.utf8)), b = NativeFraming.frame(Data(#"{"b":"é"}"#.utf8))
        var buf = a + b.prefix(3)
        XCTAssertEqual(NativeFraming.drain(&buf).map { String(decoding: $0, as: UTF8.self) }, [#"{"a":1}"#])
        buf.append(b.dropFirst(3))
        XCTAssertEqual(NativeFraming.drain(&buf).map { String(decoding: $0, as: UTF8.self) }, [#"{"b":"é"}"#])
        XCTAssertTrue(buf.isEmpty)
    }

    func testSocketRoundTrip() throws {
        let path = NSTemporaryDirectory() + "kairos-test-\(getpid()).sock"
        let server = BridgeServer(path: path)
        let got = expectation(description: "server got hello")
        server.onMessage = { client, msg in
            if msg["type"] as? String == "hello" {
                server.send(["type": "state", "n": 7], to: client)
                got.fulfill()
            }
        }
        try server.start()
        defer { server.stop() }
        let client = BridgeClient()
        XCTAssertTrue(client.connect(path: path))
        XCTAssertTrue(client.sendLine(Data(#"{"type":"hello","client":"shield"}"#.utf8)))
        let reply = expectation(description: "client got state")
        Thread.detachNewThread {
            client.readLines { line in
                let obj = try? JSONSerialization.jsonObject(with: line) as? [String: Any]
                if obj?["n"] as? Int == 7 { reply.fulfill() }
            }
        }
        wait(for: [got, reply], timeout: 3)
        client.close()
    }
}

final class ClientTests: XCTestCase {
    func testJevParseAndThreshold() throws {
        let json = #"{"model":"jev-1.13.0","answers":{"focus":{"choice":"distracting","confidence":0.93,"probabilities":{"relevant":0.03,"distracting":0.93,"unsure":0.04}}},"usage":{"input_tokens":700,"output_tokens":0}}"#
        let v = try JevClient.parse(Data(json.utf8))
        XCTAssertEqual(v.choice, "distracting")
        XCTAssertTrue(v.shouldBlock(threshold: 0.7))
        XCTAssertFalse(v.shouldBlock(threshold: 0.95))
        XCTAssertFalse(JevVerdict.unsure.shouldBlock(threshold: 0.7))
        XCTAssertThrowsError(try JevClient.parse(Data("{}".utf8)))
    }

    func testStreamParser() throws {
        var p = ClaudeAPI.StreamParser()
        try p.consume(line: "event: content_block_delta")
        try p.consume(line: #"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hm"}}"#)
        try p.consume(line: #"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"<!doctype html><html>"}}"#)
        try p.consume(line: #"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"</html>"}}"#)
        try p.consume(line: #"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}"#)
        XCTAssertEqual(try p.result(), "<!doctype html><html></html>")

        var refused = ClaudeAPI.StreamParser()
        try refused.consume(line: #"data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}}}"#)
        XCTAssertThrowsError(try refused.result())
    }

    func testExtractHTMLAndCandidates() throws {
        let wrapped = "Here you go:\n```html\n<!DOCTYPE html>\n<html><body>x</body></html>\n```\nEnjoy"
        XCTAssertEqual(try SceneWriter.extractHTML(wrapped), "<!DOCTYPE html>\n<html><body>x</body></html>")
        XCTAssertThrowsError(try SceneWriter.extractHTML("sorry, no"))
        let c = SceneWriter.parseCandidates(#"Sure: [{"name":"Jet engine","kind":"mechanism","why":"Shows airflow."}]"#)
        XCTAssertEqual(c, [SceneCandidate(name: "Jet engine", kind: "mechanism", why: "Shows airflow.")])
        XCTAssertEqual(SceneWriter.parseCandidates("none"), [])
        XCTAssertTrue(JSONSerialization.isValidJSONObject(ClaudeAPI.requestBody(ClaudePrompt(system: "s", user: "u", effort: "low", maxTokens: 10))))
    }

    func testOpenAppsNotPinnedFollowASeparator() {
        var data = AppData.starter()
        data.settings.showsDockFolders = false // the folders' own separator is tested in DockItemsTests
        data.groups[0].dockApps = ["a", "b"]
        data.focus.activeGroupId = data.groups[0].id
        let slots = DockGeometry.slots(for: data, running: ["b", "x", "a", "y", "x"])
        XCTAssertEqual(Array(slots.suffix(5)), [.app("a"), .app("b"), .separator, .running("x"), .running("y")])
        XCTAssertFalse(DockGeometry.slots(for: data, running: ["a"]).contains(.separator))
        XCTAssertFalse(DockGeometry.Slot.separator.magnifies)
        data.groups[0].dockApps = []
        XCTAssertEqual(Array(DockGeometry.slots(for: data, running: ["x"]).suffix(3)), [.addApp, .separator, .running("x")])
    }

    func testOlderFilesWithoutNewFieldsStillLoad() throws {
        let old = #"{"groups":[{"name":"Work","blockSites":["x.com"]}],"tasks":[],"sessions":[],"blockEvents":[],"settings":{"iconSize":40},"focus":{"mode":"off"},"onboarded":true}"#
        let data = try DataStore.decoder.decode(AppData.self, from: Data(old.utf8))
        XCTAssertEqual(data.groups[0].blockSites, ["x.com"])
        XCTAssertEqual(data.groups[0].desktop, 0)
        XCTAssertEqual(data.settings.iconSize, 40)
        XCTAssertEqual(data.settings.voiceProvider, .system)
        XCTAssertEqual(data.settings.presets.count, 3)
    }

    func testVoiceRequests() throws {
        let el = try XCTUnwrap(VoiceClient.request(provider: .elevenlabs, apiKey: "k", voiceId: "", text: "Hi"))
        XCTAssertEqual(el.url?.absoluteString, "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM?output_format=mp3_44100_128")
        XCTAssertEqual(el.value(forHTTPHeaderField: "xi-api-key"), "k")
        let oa = try XCTUnwrap(VoiceClient.request(provider: .openai, apiKey: "k", voiceId: "nova", text: "Hi"))
        let body = try XCTUnwrap(JSONSerialization.jsonObject(with: try XCTUnwrap(oa.httpBody)) as? [String: String])
        XCTAssertEqual(body["voice"], "nova"); XCTAssertEqual(body["input"], "Hi")
        XCTAssertNil(VoiceClient.request(provider: .system, apiKey: "k", voiceId: "", text: "Hi"))
        let voices = VoiceClient.parseElevenLabsVoices(Data(#"{"voices":[{"voice_id":"a1","name":"Rachel"},{"name":"broken"}]}"#.utf8))
        XCTAssertEqual(voices, [CloudVoice(id: "a1", name: "Rachel")])
    }

    func testStoreRoundTripAndCorruptBackup() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("kairos-\(UUID().uuidString)")
        let store = DataStore(url: dir.appendingPathComponent("kairos.json"))
        var data = AppData.starter()
        data.tasks.append(TaskItem(title: "t", groupId: data.groups[0].id))
        try store.save(data)
        XCTAssertEqual(store.load().tasks.map(\.title), ["t"])
        try Data("not json".utf8).write(to: store.url)
        XCTAssertEqual(store.load().groups.count, 3)
        let files = try FileManager.default.contentsOfDirectory(atPath: dir.path)
        XCTAssertTrue(files.contains { $0.hasPrefix("kairos.corrupt-") })
    }
}

final class DockGeometryTests: XCTestCase {
    let groups = (0..<3).map { _ in UUID() }
    var dock: DockGeometry {
        DockGeometry(slots: groups.map { .group($0) } + [.newGroup, .divider] + ["a", "b", "c", "d"].map { .app($0) }, icon: 36)
    }

    func testFalloffIsFullUnderThePointerAndRestAtRange() {
        let d = dock
        XCTAssertEqual(d.scale(distance: 0), d.maxScale, accuracy: 1e-9)
        XCTAssertEqual(d.scale(distance: d.range), 1, accuracy: 1e-9)
        XCTAssertEqual(d.scale(distance: d.range * 3), 1, accuracy: 1e-9)
        let samples = stride(from: 0.0, through: d.range, by: 2).map { d.scale(distance: $0) }
        XCTAssertEqual(samples, samples.sorted(by: >), "shrinks steadily with distance")
        XCTAssertEqual(d.scale(distance: -10), d.scale(distance: 10), accuracy: 1e-9)
    }

    func testAtRestTheBarFitsItsSlotsExactly() {
        let l = dock.layout(pointer: nil)
        XCTAssertEqual(l.start, 0)
        XCTAssertEqual(l.end, dock.restLength, accuracy: 1e-9)
        XCTAssertNil(l.hovered)
        XCTAssertTrue(l.items.allSatisfy { $0.scale == 1 })
    }

    func testMagnifiedIconsNeverOverlapAndStayInsideTheBar() {
        let d = dock
        for p in stride(from: -20.0, through: d.restLength + 20, by: 0.5) {
            let l = d.layout(pointer: p)
            for (a, b) in zip(l.items, l.items.dropFirst()) {
                XCTAssertLessThanOrEqual(a.center + a.size / 2 + d.gap, b.center - b.size / 2 + 1e-6, "overlap at pointer \(p)")
            }
            XCTAssertLessThanOrEqual(l.start + d.padding, l.items[0].center - l.items[0].size / 2 + 1e-6)
            XCTAssertGreaterThanOrEqual(l.end - d.padding, l.items.last!.center + l.items.last!.size / 2 - 1e-6)
        }
    }

    func testTheIconUnderThePointerStaysUnderItAndNeighboursGrowLess() {
        let d = dock
        let rest = d.layout(pointer: nil)
        let target = 5 // app "a", just after the divider
        let p = rest.items[target].center + 5
        let l = d.layout(pointer: p)
        XCTAssertEqual(l.hovered, target)
        XCTAssertLessThan(abs(l.items[target].center - p), l.items[target].size / 2)
        XCTAssertGreaterThan(l.items[target].scale, l.items[target + 1].scale)
        XCTAssertGreaterThan(l.items[target + 1].scale, 1)
        XCTAssertEqual(l.items[0].scale, 1, "far icons stay at rest size")
        XCTAssertEqual(l.items[4].scale, 1, "the divider never grows")
    }

    func testOverflowCoversEveryPointerPosition() {
        let d = dock
        let o = d.overflow
        XCTAssertGreaterThan(o.lead + o.trail, 0)
        for p in stride(from: -d.range, through: d.restLength + d.range, by: 0.25) {
            let l = d.layout(pointer: p)
            XCTAssertGreaterThanOrEqual(l.start, -o.lead - 1e-6)
            XCTAssertLessThanOrEqual(l.end, d.restLength + o.trail + 1e-6)
        }
    }

    func testHitZoneEngagesOnTheBarAndHoldsOverMagnifiedIcons() {
        let d = dock, inset = 4.0
        let mid = d.restLength / 2
        XCTAssertTrue(d.engages(axis: mid, cross: 0, inset: inset, engaged: false), "pushing against the screen edge engages")
        XCTAssertTrue(d.engages(axis: mid, cross: inset + d.thickness - 1, inset: inset, engaged: false))
        let overIcon = inset + d.thickness + d.headroom / 2
        XCTAssertFalse(d.engages(axis: mid, cross: overIcon, inset: inset, engaged: false), "a magnified icon's reach does not engage from outside")
        XCTAssertTrue(d.engages(axis: mid, cross: overIcon, inset: inset, engaged: true), "but holds once engaged")
        XCTAssertFalse(d.engages(axis: mid, cross: inset + d.thickness + d.headroom + 2, inset: inset, engaged: true))
        XCTAssertFalse(d.engages(axis: -5, cross: 10, inset: inset, engaged: false))
    }

    func testSlotsOfferNewGroupAndAddAppForAnEmptyGroup() {
        var data = AppData.starter()
        data.settings.showsDockFolders = false
        XCTAssertEqual(DockGeometry.slots(for: data).filter { $0 == .newGroup }.count, 1)
        XCTAssertFalse(DockGeometry.slots(for: data).contains(.addApp))
        data.groups[0].dockApps = []
        XCTAssertEqual(DockGeometry.slots(for: data).suffix(2), [.divider, .addApp])
    }
}
