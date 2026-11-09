import Foundation

public enum BridgePaths {
    public static var socket: String {
        DataStore.defaultDirectory.appendingPathComponent("bridge.sock").path
    }
    public static let hostName = "com.kairos.host"
    public static let extensionIDs = [
        "jicllkgbnjlcddcddjldjnapfaigmjoi", // Kairos Shield
        "npkcajmchbpcjigdehpiijkhkpedddcd", // Kairos Read
    ]
}

enum SocketIO {
    static func address(_ path: String) -> sockaddr_un? {
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: addr.sun_path)
        guard bytes.count < capacity else { return nil }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            for (i, b) in bytes.enumerated() { raw[i] = b }
            raw[bytes.count] = 0
        }
        return addr
    }

    static func writeAll(_ fd: Int32, _ data: Data) -> Bool {
        data.withUnsafeBytes { raw -> Bool in
            var offset = 0
            while offset < raw.count {
                let n = write(fd, raw.baseAddress! + offset, raw.count - offset)
                if n < 0 { if errno == EINTR { continue }; return false }
                if n == 0 { return false }
                offset += n
            }
            return true
        }
    }
}

/// One connected native host. Its serial queue owns the descriptor: every write runs on it,
/// and the descriptor is closed on it too, so writes never interleave and a write queued after
/// the close is dropped instead of landing on a recycled descriptor.
private final class Connection {
    let id: BridgeServer.ClientID
    var name = ""
    private var fd: Int32
    private let queue: DispatchQueue

    init(id: BridgeServer.ClientID, fd: Int32) {
        self.id = id
        self.fd = fd
        queue = DispatchQueue(label: "kairos.bridge.client.\(id)")
    }

    func write(_ data: Data) {
        queue.async { [self] in
            guard fd >= 0 else { return }
            _ = SocketIO.writeAll(fd, data)
        }
    }

    /// Wakes the reader, which then calls `close()`.
    func hangUp() {
        queue.async { [self] in if fd >= 0 { shutdown(fd, SHUT_RDWR) } }
    }

    func close() {
        queue.async { [self] in
            guard fd >= 0 else { return }
            Darwin.close(fd)
            fd = -1
        }
    }
}

/// Unix-socket server inside Kairos Desktop. Each connected native host is one client.
/// Messages are newline-delimited JSON objects.
public final class BridgeServer {
    public typealias Message = [String: Any]
    /// Never reused, unlike a file descriptor, so a late reply cannot reach a newer client.
    public typealias ClientID = Int

    public enum StartError: Error, Equatable {
        /// Another process is already listening on the socket.
        case alreadyRunning
    }

    public var onMessage: ((_ client: ClientID, _ message: Message) -> Void)?
    public var onClientsChanged: (() -> Void)?

    private let path: String
    private var listenFD: Int32 = -1
    private let lock = NSLock()
    private var clients: [ClientID: Connection] = [:]
    private var nextID: ClientID = 1

    public init(path: String = BridgePaths.socket) { self.path = path }

    /// True when a live server accepts connections at `path`. A socket file left behind by a
    /// crash refuses the connection.
    public static func isListening(at path: String = BridgePaths.socket) -> Bool {
        let client = BridgeClient()
        defer { client.close() }
        return client.connect(path: path)
    }

    /// Throws `StartError.alreadyRunning` rather than take the socket from a live server.
    public func start() throws {
        signal(SIGPIPE, SIG_IGN)
        try FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
        if Self.isListening(at: path) { throw StartError.alreadyRunning }
        unlink(path)
        guard var addr = SocketIO.address(path) else { throw POSIXError(.ENAMETOOLONG) }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard bound == 0, listen(fd, 8) == 0 else {
            let code = errno; close(fd)
            throw POSIXError(.init(rawValue: code) ?? .EIO)
        }
        chmod(path, 0o600)
        listenFD = fd
        Thread.detachNewThread { [weak self] in self?.acceptLoop(fd) }
    }

    public func stop() {
        stopListeningKeepingFile()
        unlink(path)
    }

    /// Stops accepting and hangs up every client, but leaves the socket file, as a crash would.
    func stopListeningKeepingFile() {
        if listenFD >= 0 { close(listenFD); listenFD = -1 }
        lock.lock(); let all = Array(clients.values); lock.unlock()
        all.forEach { $0.hangUp() }
    }

    public var clientNames: [String] {
        lock.lock(); defer { lock.unlock() }
        return clients.values.map(\.name).filter { !$0.isEmpty }.sorted()
    }

    public func setName(_ name: String, for client: ClientID) {
        lock.lock(); clients[client]?.name = name; lock.unlock()
        onClientsChanged?()
    }

    public func send(_ message: Message, to client: ClientID) {
        guard JSONSerialization.isValidJSONObject(message),
              var data = try? JSONSerialization.data(withJSONObject: message) else { return }
        data.append(0x0A)
        lock.lock(); let connection = clients[client]; lock.unlock()
        connection?.write(data)
    }

    /// Sends to every client, or only to clients with the given name.
    public func broadcast(_ message: Message, to name: String? = nil) {
        lock.lock()
        let targets = clients.values.filter { name == nil || $0.name == name }.map(\.id)
        lock.unlock()
        targets.forEach { send(message, to: $0) }
    }

    private func acceptLoop(_ fd: Int32) {
        while true {
            let client = accept(fd, nil, nil)
            if client < 0 {
                if errno == EINTR { continue }
                return // listener closed
            }
            var on: Int32 = 1
            setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
            lock.lock()
            let connection = Connection(id: nextID, fd: client)
            nextID += 1
            clients[connection.id] = connection
            lock.unlock()
            Thread.detachNewThread { [weak self] in self?.readLoop(connection, fd: client) }
        }
    }

    private func readLoop(_ connection: Connection, fd: Int32) {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let n = read(fd, &chunk, chunk.count)
            if n < 0, errno == EINTR { continue }
            if n <= 0 { break }
            buffer.append(chunk, count: n)
            while let nl = buffer.firstIndex(of: 0x0A) {
                let line = buffer.subdata(in: buffer.startIndex..<nl)
                buffer.removeSubrange(buffer.startIndex...nl)
                if let obj = try? JSONSerialization.jsonObject(with: line) as? Message {
                    onMessage?(connection.id, obj)
                }
            }
        }
        lock.lock(); clients.removeValue(forKey: connection.id); lock.unlock()
        connection.close()
        onClientsChanged?()
    }
}

/// Client side, used by the native messaging host (and tests).
public final class BridgeClient {
    public private(set) var fd: Int32 = -1

    public init() {}

    public func connect(path: String = BridgePaths.socket) -> Bool {
        signal(SIGPIPE, SIG_IGN)
        guard var addr = SocketIO.address(path) else { return false }
        let s = socket(AF_UNIX, SOCK_STREAM, 0)
        guard s >= 0 else { return false }
        let ok = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.connect(s, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) }
        }
        guard ok == 0 else { Darwin.close(s); return false }
        fd = s
        return true
    }

    public func sendLine(_ data: Data) -> Bool {
        var d = data; d.append(0x0A)
        return SocketIO.writeAll(fd, d)
    }

    /// Blocks, calling `handler` with each newline-delimited payload until the socket closes.
    public func readLines(_ handler: (Data) -> Void) {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            let n = read(fd, &chunk, chunk.count)
            if n < 0, errno == EINTR { continue }
            if n <= 0 { return }
            buffer.append(chunk, count: n)
            while let nl = buffer.firstIndex(of: 0x0A) {
                handler(buffer.subdata(in: buffer.startIndex..<nl))
                buffer.removeSubrange(buffer.startIndex...nl)
            }
        }
    }

    public func close() { if fd >= 0 { Darwin.close(fd); fd = -1 } }
}

/// Chrome native-messaging framing: 4-byte little-endian length, then UTF-8 JSON.
public enum NativeFraming {
    public static func frame(_ payload: Data) -> Data {
        var len = UInt32(payload.count).littleEndian
        var out = Data(bytes: &len, count: 4)
        out.append(payload)
        return out
    }

    /// Pulls complete frames off the front of `buffer`.
    public static func drain(_ buffer: inout Data) -> [Data] {
        var frames: [Data] = []
        while buffer.count >= 4 {
            let start = buffer.startIndex
            let len = Int(buffer[start]) | Int(buffer[start + 1]) << 8 | Int(buffer[start + 2]) << 16 | Int(buffer[start + 3]) << 24
            guard buffer.count >= 4 + len else { break }
            frames.append(buffer.subdata(in: (start + 4)..<(start + 4 + len)))
            buffer.removeSubrange(start..<(start + 4 + len))
        }
        return frames
    }

    /// Native messaging manifest for one browser.
    public static func hostManifest(hostPath: String) -> [String: Any] {
        [
            "name": BridgePaths.hostName,
            "description": "Kairos Desktop bridge",
            "path": hostPath,
            "type": "stdio",
            "allowed_origins": BridgePaths.extensionIDs.map { "chrome-extension://\($0)/" },
        ]
    }

    /// Browser name -> NativeMessagingHosts directory, relative to ~/Library/Application Support.
    public static let browserDirs: [(name: String, dir: String)] = [
        ("Google Chrome", "Google/Chrome/NativeMessagingHosts"),
        ("Chrome Beta", "Google/Chrome Beta/NativeMessagingHosts"),
        ("Chrome Canary", "Google/Chrome Canary/NativeMessagingHosts"),
        ("Chromium", "Chromium/NativeMessagingHosts"),
        ("Brave", "BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        ("Microsoft Edge", "Microsoft Edge/NativeMessagingHosts"),
        ("Arc", "Arc/User Data/NativeMessagingHosts"),
        ("Vivaldi", "Vivaldi/NativeMessagingHosts"),
    ]
}
