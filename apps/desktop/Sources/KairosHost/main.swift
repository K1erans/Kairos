import Foundation
import KairosCore

// Native messaging host. Chrome starts one per extension connection and talks to it over
// stdin/stdout; this relays every message to Kairos Desktop over its Unix socket and back.

let client = BridgeClient()
var connected = false
for _ in 0..<15 {
    if client.connect() { connected = true; break }
    usleep(200_000)
}

func writeStdout(_ payload: Data) {
    FileHandle.standardOutput.write(NativeFraming.frame(payload))
}

guard connected else {
    writeStdout(Data(#"{"type":"error","message":"Kairos Desktop is not running"}"#.utf8))
    exit(1)
}

// App -> Chrome
Thread.detachNewThread {
    client.readLines { writeStdout($0) }
    exit(0) // the app went away; Chrome's extension will reconnect
}

// Chrome -> app
var buffer = Data()
let stdin = FileHandle.standardInput
while true {
    let chunk = stdin.availableData
    if chunk.isEmpty { break } // Chrome closed the port
    buffer.append(chunk)
    for frame in NativeFraming.drain(&buffer) {
        // Re-serialise so an embedded newline can never split a message.
        guard let obj = try? JSONSerialization.jsonObject(with: frame),
              let line = try? JSONSerialization.data(withJSONObject: obj) else { continue }
        if !client.sendLine(line) { exit(0) }
    }
}
client.close()
