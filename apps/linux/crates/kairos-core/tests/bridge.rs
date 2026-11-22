mod common;
use common::*;
use kairos_core::bridge::{drain, frame, host_manifest, is_listening, BridgeEvent, BridgeServer, ClientId, StartError};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Short, because a Unix socket path has a small limit on macOS.
fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("kb-{}.sock", &uuid::Uuid::new_v4().simple().to_string()[..8]))
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    f()
}

struct Recorder {
    ids: Arc<Mutex<Vec<ClientId>>>,
    changes: Arc<Mutex<usize>>,
}

fn start(server: &BridgeServer) -> Recorder {
    let ids = Arc::new(Mutex::new(vec![]));
    let changes = Arc::new(Mutex::new(0));
    let (i, c) = (ids.clone(), changes.clone());
    server
        .start(Arc::new(move |e| match e {
            BridgeEvent::Message(id, _) => i.lock().unwrap().push(id),
            BridgeEvent::ClientsChanged => *c.lock().unwrap() += 1,
        }))
        .unwrap();
    Recorder { ids, changes }
}

/// Connects, says hello, and returns the stream once the server has heard it.
fn connect(path: &PathBuf, rec: &Recorder) -> UnixStream {
    let before = rec.ids.lock().unwrap().len();
    let mut s = UnixStream::connect(path).unwrap();
    s.write_all(b"{\"type\":\"hello\",\"client\":\"shield\"}\n").unwrap();
    assert!(wait_until(|| rec.ids.lock().unwrap().len() > before));
    s
}

#[test]
fn framing_round_trip_and_partial_frames() {
    let a = frame(br#"{"a":1}"#);
    let b = frame(r#"{"b":"é"}"#.as_bytes());
    let mut buf = a.clone();
    buf.extend_from_slice(&b[..3]);
    assert_eq!(drain(&mut buf), vec![br#"{"a":1}"#.to_vec()]);
    buf.extend_from_slice(&b[3..]);
    assert_eq!(drain(&mut buf), vec![r#"{"b":"é"}"#.as_bytes().to_vec()]);
    assert!(buf.is_empty());
}

#[test]
fn the_manifest_names_the_host_and_both_extensions() {
    let m = host_manifest("/usr/lib/kairos/kairos-host");
    assert_eq!(m["name"], "com.kairos.host");
    assert_eq!(m["type"], "stdio");
    assert_eq!(m["allowed_origins"].as_array().unwrap().len(), 2);
}

#[test]
fn socket_round_trip() {
    let path = socket_path();
    let server = BridgeServer::new(path.clone());
    let rec = start(&server);
    let s = connect(&path, &rec);
    let id = rec.ids.lock().unwrap()[0];
    server.set_name("shield", id);
    assert_eq!(server.client_names(), vec!["shield"]);
    server.send(&json!({"type": "state", "n": 7}), id);
    let mut line = String::new();
    BufReader::new(s.try_clone().unwrap()).read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["n"], 7);
    server.stop();
    assert!(!path.exists(), "stopping removes the socket file");
}

#[test]
fn large_messages_to_one_client_arrive_whole() {
    let path = socket_path();
    let server = BridgeServer::new(path.clone());
    let rec = start(&server);
    let client = connect(&path, &rec);
    let id = rec.ids.lock().unwrap()[0];

    let count = 24;
    let reader = {
        let s = client.try_clone().unwrap();
        std::thread::spawn(move || {
            let (mut valid, mut invalid) = (0, 0);
            let mut lines = BufReader::new(s);
            let mut line = Vec::new();
            while valid + invalid < count {
                line.clear();
                if lines.read_until(b'\n', &mut line).unwrap_or(0) == 0 {
                    break;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(Value::Object(_)) => valid += 1,
                    _ => invalid += 1,
                }
            }
            (valid, invalid)
        })
    };
    let big = "x".repeat(100_000);
    let senders: Vec<_> = (0..count)
        .map(|n| {
            let server = server.clone();
            let big = big.clone();
            std::thread::spawn(move || server.send(&json!({"type": "state", "n": n, "pad": big}), id))
        })
        .collect();
    for s in senders {
        s.join().unwrap();
    }
    let (valid, invalid) = reader.join().unwrap();
    assert_eq!(invalid, 0, "messages interleaved on the wire");
    assert_eq!(valid, count);
    server.stop();
}

#[test]
fn reply_to_a_closed_client_never_reaches_the_next_one() {
    let path = socket_path();
    let server = BridgeServer::new(path.clone());
    let rec = start(&server);
    let first = connect(&path, &rec);
    let first_id = rec.ids.lock().unwrap()[0];
    drop(first);
    assert!(wait_until(|| *rec.changes.lock().unwrap() > 0));

    let second = connect(&path, &rec);
    let second_id = *rec.ids.lock().unwrap().last().unwrap();
    assert_ne!(first_id, second_id);
    server.send(&json!({"type": "late reply for the first client"}), first_id);
    server.send(&json!({"type": "for the second"}), second_id);
    let mut line = String::new();
    BufReader::new(second).read_line(&mut line).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "for the second");
    server.stop();
}

#[test]
fn a_second_server_does_not_steal_a_live_socket() {
    let path = socket_path();
    let first = BridgeServer::new(path.clone());
    let rec = start(&first);
    assert!(is_listening(&path));
    let second = BridgeServer::new(path.clone());
    assert!(matches!(second.start(Arc::new(|_| {})), Err(StartError::AlreadyRunning)));
    let _client = connect(&path, &rec);
    assert_eq!(rec.ids.lock().unwrap().len(), 1, "the running server still owns the socket");
    first.stop();
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let path = socket_path();
    drop(UnixListener::bind(&path).unwrap()); // as a crash leaves it
    assert!(path.exists());
    assert!(!is_listening(&path));
    let server = BridgeServer::new(path.clone());
    assert!(server.start(Arc::new(|_| {})).is_ok());
    assert!(is_listening(&path));
    server.stop();
}

#[test]
fn the_socket_lives_in_the_data_folder() {
    let home = scratch_home();
    assert_eq!(kairos_core::bridge::socket_path(), home.join("bridge.sock"));
}

#[test]
fn servers_starting_together_end_with_exactly_one_listening() {
    let path = socket_path();
    let servers: Vec<BridgeServer> = (0..8).map(|_| BridgeServer::new(path.clone())).collect();
    let results: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = servers.iter().map(|s| scope.spawn(move || s.start(Arc::new(|_| {})).is_ok())).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|ok| **ok).count(), 1, "{results:?}");
    assert!(is_listening(&path));
    for s in &servers {
        s.stop();
    }
}

#[test]
fn stopping_an_old_server_leaves_a_newer_servers_socket() {
    let path = socket_path();
    let old = BridgeServer::new(path.clone());
    assert!(old.start(Arc::new(|_| {})).is_ok());
    // The old server's socket goes away (as a crash of its listener would), and a new one binds.
    std::fs::remove_file(&path).unwrap();
    let new = BridgeServer::new(path.clone());
    assert!(new.start(Arc::new(|_| {})).is_ok());
    old.stop();
    assert!(is_listening(&path), "the old server did not delete the new server's socket");
    new.stop();
    assert!(!path.exists());
}
