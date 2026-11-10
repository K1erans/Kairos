import Foundation
import KairosCore
import XCTest

/// Scenes reach Claude two ways behind one interface. The subscription backend runs a helper
/// with `node`; these tests run a shell script as the helper with /bin/sh standing in for node,
/// so they need neither Node nor a Claude login and never call Claude.
final class SceneBackendTests: XCTestCase {
    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("scene-helper-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    /// A fake `scene-agent.mjs`: `body` runs with HERE set to its folder.
    private func helper(_ body: String) throws -> ClaudeSubscription {
        let url = dir.appendingPathComponent("scene-agent.sh")
        try "HERE=$(dirname \"$0\")\n\(body)\n".write(to: url, atomically: true, encoding: .utf8)
        return ClaudeSubscription(node: URL(fileURLWithPath: "/bin/sh"), helper: url,
                                  environment: ["PATH": "/usr/bin", "HOME": NSHomeDirectory(), "ANTHROPIC_API_KEY": "sk-should-not-pass"])
    }

    private let prompt = ClaudePrompt(system: "sys", user: "hello", effort: "low", maxTokens: 10)

    private func error(_ work: () async throws -> some Any) async -> ClaudeError? {
        do { _ = try await work(); return nil } catch { return error as? ClaudeError }
    }

    // MARK: Claude subscription

    func testTheHelperGetsThePromptOnStdinAndItsAnswerComesBack() async throws {
        let backend = try helper(#"cat > "$HERE/input.json"; env > "$HERE/env.txt"; printf 'An answer'"#)
        let answer = try await backend.complete(prompt)
        XCTAssertEqual(answer, "An answer")
        let input = try JSONSerialization.jsonObject(with: Data(contentsOf: dir.appendingPathComponent("input.json"))) as? [String: String]
        XCTAssertEqual(input, ["system": "sys", "user": "hello", "effort": "low", "model": "claude-opus-5-5"])
        let env = try String(contentsOf: dir.appendingPathComponent("env.txt"), encoding: .utf8)
        XCTAssertTrue(env.contains("PATH=/bin:/usr/bin"), "node's folder comes first, for anything it starts")
        XCTAssertFalse(env.contains("ANTHROPIC_API_KEY"), "an API key would be billed instead of the subscription")
    }

    func testAGeneratedSceneIsTheDocumentInTheAnswer() async throws {
        let backend = try helper(#"cat > "$HERE/input.json"; printf 'Here it is:\n```html\n<!doctype html><html><body>clock</body></html>\n```\n'"#)
        let html = try await SceneWriter(backend: backend).generate(title: "Clocks", text: "A pendulum swings.", subject: "Pendulum clock")
        XCTAssertEqual(html, "<!doctype html><html><body>clock</body></html>")
        let input = try JSONSerialization.jsonObject(with: Data(contentsOf: dir.appendingPathComponent("input.json"))) as? [String: String]
        XCTAssertEqual(input?["system"], SceneWriter.sceneSystem)
        XCTAssertEqual(input?["effort"], "high")
        XCTAssertTrue(input?["user"]?.contains("Build a scene of: Pendulum clock") == true)
    }

    func testALargeSceneDoesNotStallThePipes() async throws {
        let backend = try helper(#"cat > /dev/null; printf '<!doctype html><html><body>'; head -c 400000 /dev/zero | tr '\0' 'a'; printf '</body></html>'"#)
        let html = try await SceneWriter(backend: backend).generate(title: "t", text: String(repeating: "word ", count: 20_000), subject: "s")
        XCTAssertEqual(html.utf8.count, 400_000 + "<!doctype html><html><body></body></html>".utf8.count)
    }

    func testClaudeCodeNotLoggedInSaysHowToLogIn() async throws {
        let backend = try helper(#"cat > /dev/null; echo 'Invalid API key · Please run /login' >&2; exit 1"#)
        let failure = await error { try await backend.complete(self.prompt) }
        XCTAssertEqual(failure, .notLoggedIn)
        XCTAssertTrue(failure?.errorDescription?.contains("/login") == true)
    }

    func testAnyOtherFailureGivesTheHelpersLine() async throws {
        let backend = try helper(#"cat > /dev/null; echo 'starting' >&2; echo 'Claude stopped early (error_max_turns).' >&2; exit 1"#)
        let failure = await error { try await backend.complete(self.prompt) }
        XCTAssertEqual(failure, .helper("Claude stopped early (error_max_turns)."))
    }

    func testMissingNodeOrHelperSayWhatToDo() async throws {
        let noNode = ClaudeSubscription(node: nil, helper: dir.appendingPathComponent("scene-agent.mjs"))
        let a = await error { try await noNode.complete(self.prompt) }
        XCTAssertEqual(a, .noNode)
        XCTAssertTrue(a?.errorDescription?.contains("brew install node") == true)
        let noHelper = ClaudeSubscription(node: URL(fileURLWithPath: "/bin/sh"), helper: dir.appendingPathComponent("missing.mjs"))
        let b = await error { try await noHelper.complete(self.prompt) }
        XCTAssertEqual(b, .noHelper)
    }

    func testAHelperWithoutItsPackagesCountsAsMissing() {
        let node = """
        node:internal/modules/esm/resolve:839
          throw new ERR_MODULE_NOT_FOUND(packageName, fileURLToPath(base), null);
        Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@anthropic-ai/claude-agent-sdk' imported from /x/scene-agent.mjs
            at packageResolve (node:internal/modules/esm/resolve:839:9)
        Node.js v22.11.0
        """
        XCTAssertEqual(ClaudeSubscription.failure(stderr: node), .noHelper)
        XCTAssertEqual(ClaudeSubscription.failure(stderr: ""), .helper("the scene helper stopped without saying why."))
    }

    func testCancellingStopsTheHelperProcess() async throws {
        let backend = try helper(#"echo $$ > "$HERE/pid"; exec sleep 30"#)
        let started = Date()
        let task = Task { try await backend.complete(self.prompt) }
        let pidFile = dir.appendingPathComponent("pid")
        while !FileManager.default.fileExists(atPath: pidFile.path), Date().timeIntervalSince(started) < 5 { try await Task.sleep(for: .milliseconds(20)) }
        try await Task.sleep(for: .milliseconds(50))
        task.cancel()
        let failure = await error { try await task.value }
        XCTAssertEqual(failure, .cancelled)
        XCTAssertEqual(failure?.errorDescription, "The scene build was cancelled.")
        XCTAssertLessThan(Date().timeIntervalSince(started), 5, "it did not wait for the helper to finish")
        let pid = try XCTUnwrap(Int32(String(contentsOf: pidFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)))
        XCTAssertNotEqual(kill(pid, 0), 0, "the helper is gone")
    }

    func testCancellingBeforeTheHelperStartsNeverStartsIt() async throws {
        let backend = try helper(#"touch "$HERE/ran""#)
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await backend.complete(self.prompt)
        }
        let failure = await error { try await task.value }
        XCTAssertEqual(failure, .cancelled)
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir.appendingPathComponent("ran").path))
    }

    // MARK: Anthropic API key

    func testCancellingAnAPIRequestDropsIt() async throws {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [SilentServer.self]
        let api = ClaudeAPI(apiKey: "sk-test", session: URLSession(configuration: config))
        let task = Task { try await api.complete(self.prompt) }
        try await Task.sleep(for: .milliseconds(100))
        task.cancel()
        let failure = await error { try await task.value }
        XCTAssertEqual(failure, .cancelled)
    }

    func testTheAPIBackendStillNeedsAKey() async {
        let failure = await error { try await ClaudeAPI(apiKey: nil).complete(self.prompt) }
        XCTAssertEqual(failure, .noKey)
    }

    // MARK: Finding node and the helper

    func testNodeIsLookedForWhereMacsKeepIt() {
        XCTAssertEqual(NodeLocator.candidates(home: "/Users/o", nvmVersions: ["v9.11.2", "v22.11.0", "v20.3.1"]), [
            "/opt/homebrew/bin/node", "/usr/local/bin/node", "/Users/o/.volta/bin/node",
            "/Users/o/.nvm/versions/node/v22.11.0/bin/node", "/Users/o/.nvm/versions/node/v20.3.1/bin/node",
            "/Users/o/.nvm/versions/node/v9.11.2/bin/node",
        ])
    }

    func testTheFirstNodeFoundWinsAndTheLoginShellIsTheFallback() {
        var asked = false
        let shell = { () -> String? in asked = true; return "Welcome!\n/Users/o/.asdf/shims/node\n" }
        XCTAssertEqual(NodeLocator.find(home: "/Users/o", nvmVersions: [], isExecutable: { $0 == "/usr/local/bin/node" }, loginShell: shell),
                       "/usr/local/bin/node")
        XCTAssertFalse(asked, "the shell is only asked when nothing else is there")
        XCTAssertEqual(NodeLocator.find(home: "/Users/o", nvmVersions: [], isExecutable: { $0.hasPrefix("/Users/o/.asdf") }, loginShell: shell),
                       "/Users/o/.asdf/shims/node")
        XCTAssertNil(NodeLocator.find(home: "/Users/o", nvmVersions: [], isExecutable: { _ in false }, loginShell: shell))
        XCTAssertNil(NodeLocator.find(home: "/Users/o", nvmVersions: [], isExecutable: { !$0.hasPrefix("/") }, loginShell: { "node not found" }),
                     "an answer that is no path is not taken")
    }

    func testTheHelperIsInTheAppUnlessPointedElsewhere() {
        let resources = URL(fileURLWithPath: "/Applications/Kairos.app/Contents/Resources")
        XCTAssertEqual(SceneHelper.url(resources: resources, environment: [:])?.path,
                       "/Applications/Kairos.app/Contents/Resources/scene-agent/scene-agent.mjs")
        XCTAssertEqual(SceneHelper.url(resources: resources, environment: ["KAIROS_SCENE_AGENT": "/src/scene-agent/scene-agent.mjs"])?.path,
                       "/src/scene-agent/scene-agent.mjs")
        XCTAssertFalse(SceneHelper.isInstalled(dir.appendingPathComponent("scene-agent.mjs")))
    }

    func testAHelperCountsAsInstalledWithItsSDK() throws {
        let helper = dir.appendingPathComponent("scene-agent.mjs")
        try "".write(to: helper, atomically: true, encoding: .utf8)
        XCTAssertFalse(SceneHelper.isInstalled(helper), "npm ci has not run")
        try FileManager.default.createDirectory(at: dir.appendingPathComponent("node_modules/@anthropic-ai/claude-agent-sdk"), withIntermediateDirectories: true)
        XCTAssertTrue(SceneHelper.isInstalled(helper))
    }

    // MARK: Settings and the checklist

    func testTheSubscriptionIsTheDefaultForNewAndExistingProfiles() throws {
        XCTAssertEqual(Settings().sceneBackend, .subscription)
        let saved = try DataStore.decoder.decode(Settings.self, from: Data(#"{"dockEdge": "bottom"}"#.utf8))
        XCTAssertEqual(saved.sceneBackend, .subscription)
        XCTAssertEqual(SceneBackend.allCases.map(\.label), ["Claude subscription", "Anthropic API key"])
    }

    func testSettingsSaysWhatIsMissing() {
        let none = SceneSetup()
        XCTAssertFalse(none.status(.subscription, hasClaudeKey: true).ready)
        XCTAssertTrue(none.status(.subscription, hasClaudeKey: true).text.contains("brew install node"))
        var noHelper = SceneSetup(node: URL(fileURLWithPath: "/opt/homebrew/bin/node"))
        XCTAssertTrue(noHelper.status(.subscription, hasClaudeKey: false).text.contains("build-app.sh"))
        noHelper.helperInstalled = true
        let ready = noHelper.status(.subscription, hasClaudeKey: false)
        XCTAssertTrue(ready.ready)
        XCTAssertTrue(ready.text.contains("/opt/homebrew/bin/node"))
        XCTAssertFalse(none.status(.apiKey, hasClaudeKey: false).ready)
        XCTAssertTrue(none.status(.apiKey, hasClaudeKey: true).ready)
    }

    func testTheChecklistAsksForNodeAndTheHelperNotAKey() {
        var facts = SetupFacts()
        var item: SetupChecklist.Item? { SetupChecklist(facts).items.first { $0.step == .scenes } }
        XCTAssertEqual(item?.done, false)
        XCTAssertTrue(item?.detail.contains("Node.js") == true)
        XCTAssertFalse(item?.title.contains("Anthropic") == true, "no API key is demanded")
        facts.hasNode = true
        XCTAssertTrue(item?.detail.contains("scene helper") == true)
        facts.hasSceneHelper = true
        XCTAssertEqual(item?.done, true)
        XCTAssertEqual(item?.title, "Claude subscription ready")

        facts.sceneBackend = .apiKey
        XCTAssertEqual(item?.done, false)
        XCTAssertEqual(item?.title, "Add an Anthropic key (optional)")
        facts.hasClaudeKey = true
        XCTAssertEqual(item?.done, true)
    }
}

/// Accepts a request and never answers, like a server still thinking.
private final class SilentServer: URLProtocol {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {}
    override func stopLoading() {}
}
