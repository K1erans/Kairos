//! Chrome native messaging host. The browser starts it and talks length-prefixed JSON on
//! stdin/stdout; it relays each message to the running Kairos app over its Unix socket as one
//! line of JSON, and each line back as a frame.

use kairos_core::bridge;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn write_frame(out: &mut impl Write, value: &[u8]) -> std::io::Result<()> {
    out.write_all(&(value.len() as u32).to_le_bytes())?;
    out.write_all(value)?;
    out.flush()
}

fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 * 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; n];
    input.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn connect() -> Option<UnixStream> {
    let path = bridge::socket_path();
    for _ in 0..15 {
        if let Ok(s) = UnixStream::connect(&path) {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn main() {
    let Some(stream) = connect() else {
        let msg = serde_json::json!({"type": "error", "message": "Kairos Desktop is not running."});
        let _ = write_frame(&mut std::io::stdout().lock(), msg.to_string().as_bytes());
        std::process::exit(1);
    };
    let mut writer = stream.try_clone().expect("socket clone");
    // Socket -> stdout.
    let reader = std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            if write_frame(&mut out, line.as_bytes()).is_err() {
                break;
            }
        }
        std::process::exit(0);
    });
    // stdin -> socket.
    let mut input = std::io::stdin().lock();
    while let Some(frame) = read_frame(&mut input) {
        // Messages are single-line JSON; re-encode to be sure there is no newline inside.
        let line = match serde_json::from_slice::<serde_json::Value>(&frame) {
            Ok(v) => v.to_string(),
            Err(_) => continue,
        };
        if writer.write_all(line.as_bytes()).and_then(|_| writer.write_all(b"\n")).is_err() {
            break;
        }
    }
    let _ = writer.shutdown(std::net::Shutdown::Both);
    let _ = reader.join();
}
