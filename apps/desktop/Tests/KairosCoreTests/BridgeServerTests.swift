import XCTest
@testable import KairosCore

final class BridgeServerTests: XCTestCase {
    private func socketPath() -> String { NSTemporaryDirectory() + "kb-\(UUID().uuidString.prefix(8)).sock" }

    /// Connects, says hello, and returns the client plus the server-side id it was given.
    private func connect(_ server: BridgeServer, path: String, ids: LockedBox<[BridgeServer.ClientID]>) throws -> BridgeClient {
        let client = BridgeClient()
        XCTAssertTrue(client.connect(path: path))
        XCTAssertTrue(client.sendLine(Data(#"{"type":"hello"}"#.utf8)))
        let deadline = Date().addingTimeInterval(3)
        let before = ids.value.count
        while ids.value.count == before, Date() < deadline { usleep(5000) }
        return client
    }

    func testLargeMessagesToOneClientArriveWhole() throws {
        let path = socketPath()
        let server = BridgeServer(path: path)
        let ids = LockedBox<[BridgeServer.ClientID]>([])
        server.onMessage = { id, _ in ids.mutate { $0.append(id) } }
        try server.start()
        defer { server.stop() }
        let client = try connect(server, path: path, ids: ids)
        let id = try XCTUnwrap(ids.value.first)

        let count = 24
        let big = String(repeating: "x", count: 100_000)
        DispatchQueue.concurrentPerform(iterations: count) { n in
            server.send(["type": "state", "n": n, "pad": big], to: id)
        }
        var valid = 0, invalid = 0
        let done = expectation(description: "all lines read")
        Thread.detachNewThread {
            client.readLines { line in
                if (try? JSONSerialization.jsonObject(with: line)) is [String: Any] { valid += 1 } else { invalid += 1 }
                if valid + invalid == count { done.fulfill() }
            }
        }
        wait(for: [done], timeout: 20)
        XCTAssertEqual(invalid, 0, "messages interleaved on the wire")
        XCTAssertEqual(valid, count)
        client.close()
    }

    func testReplyToAClosedClientNeverReachesTheNextOne() throws {
        let path = socketPath()
        let server = BridgeServer(path: path)
        let ids = LockedBox<[BridgeServer.ClientID]>([])
        let gone = LockedBox(0)
        server.onMessage = { id, _ in ids.mutate { $0.append(id) } }
        server.onClientsChanged = { gone.mutate { $0 += 1 } }
        try server.start()
        defer { server.stop() }

        let first = try connect(server, path: path, ids: ids)
        let firstId = try XCTUnwrap(ids.value.first)
        first.close()
        let deadline = Date().addingTimeInterval(3)
        while gone.value == 0, Date() < deadline { usleep(5000) }

        // The kernel hands the next connection the lowest free descriptor, which is the one
        // the first client just gave up.
        let second = try connect(server, path: path, ids: ids)
        server.send(["type": "late reply for the first client"], to: firstId)
        server.send(["type": "for the second"], to: try XCTUnwrap(ids.value.last))
        var lines: [String] = []
        let got = expectation(description: "second client read its message")
        Thread.detachNewThread {
            second.readLines { line in
                let t = (try? JSONSerialization.jsonObject(with: line) as? [String: Any])?["type"] as? String ?? "?"
                lines.append(t)
                if t == "for the second" { got.fulfill() }
            }
        }
        wait(for: [got], timeout: 3)
        XCTAssertEqual(lines, ["for the second"])
        second.close()
    }

    func testASecondServerDoesNotStealALiveSocket() throws {
        let path = socketPath()
        let first = BridgeServer(path: path)
        let heard = LockedBox(0)
        first.onMessage = { _, _ in heard.mutate { $0 += 1 } }
        try first.start()
        defer { first.stop() }
        XCTAssertTrue(BridgeServer.isListening(at: path))

        let second = BridgeServer(path: path)
        XCTAssertThrowsError(try second.start()) { error in
            XCTAssertEqual(error as? BridgeServer.StartError, .alreadyRunning)
        }
        let client = BridgeClient()
        XCTAssertTrue(client.connect(path: path))
        XCTAssertTrue(client.sendLine(Data(#"{"type":"hello"}"#.utf8)))
        let deadline = Date().addingTimeInterval(3)
        while heard.value == 0, Date() < deadline { usleep(5000) }
        XCTAssertEqual(heard.value, 1, "the running server still owns the socket")
        client.close()
    }

    func testAStaleSocketFileIsReplaced() throws {
        let path = socketPath()
        let dead = BridgeServer(path: path)
        try dead.start()
        dead.stopListeningKeepingFile()
        XCTAssertTrue(FileManager.default.fileExists(atPath: path))
        XCTAssertFalse(BridgeServer.isListening(at: path))
        let server = BridgeServer(path: path)
        XCTAssertNoThrow(try server.start())
        XCTAssertTrue(BridgeServer.isListening(at: path))
        server.stop()
    }
}

final class LockedBox<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: T
    init(_ value: T) { stored = value }
    var value: T { lock.lock(); defer { lock.unlock() }; return stored }
    func mutate(_ f: (inout T) -> Void) { lock.lock(); f(&stored); lock.unlock() }
}
