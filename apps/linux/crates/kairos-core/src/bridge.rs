//! The link to the browser extensions: a Unix socket server in Kairos (newline-delimited JSON
//! objects, one client per connected native host), the client side the native host uses, and
//! Chrome's native messaging framing and manifests.

use crate::store::{config_dir, default_directory, is_scratch};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type Message = Map<String, Value>;
/// Never reused, unlike a file descriptor, so a late reply cannot reach a newer client.
pub type ClientId = u64;

pub const HOST_NAME: &str = "com.kairos.host";
pub const EXTENSION_IDS: &[&str] = &[
    "jicllkgbnjlcddcddjldjnapfaigmjoi", // Kairos Shield
    "npkcajmchbpcjigdehpiijkhkpedddcd", // Kairos Read
];

pub fn socket_path() -> PathBuf {
    default_directory().join("bridge.sock")
}

pub enum BridgeEvent {
    Message(ClientId, Message),
    ClientsChanged,
}

struct Client {
    name: String,
    stream: Arc<Mutex<UnixStream>>,
}

#[derive(Clone)]
pub struct BridgeServer {
    path: PathBuf,
    clients: Arc<Mutex<HashMap<ClientId, Client>>>,
    listener: Arc<Mutex<Option<UnixListener>>>,
    /// (device, inode) of the socket file this server bound, so `stop` removes only its own.
    bound: Arc<Mutex<Option<(u64, u64)>>>,
}

/// Holds `<socket>.lock` until dropped. The app's single-instance lock (`kairos.lock`) already
/// keeps a second Kairos on the same data folder from getting this far, but it lets a launch
/// through when it can't make its lock file, and `BridgeServer` is also used on its own. This
/// lock makes "is a server listening? if not, remove the stale socket and bind" one step, so two
/// servers starting together can't both remove and bind, and a server stopping can't delete the
/// socket a new one has just bound.
struct BindLock(#[allow(dead_code)] Option<std::fs::File>);

fn bind_lock(socket: &Path) -> BindLock {
    let mut name = socket.as_os_str().to_owned();
    name.push(".lock");
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(PathBuf::from(name)).ok();
    // Without a lock file, carry on: the instance lock still covers the usual case.
    BindLock(file.filter(|f| f.lock().is_ok()))
}

fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[derive(Debug)]
pub enum StartError {
    /// Another process is already listening on the socket.
    AlreadyRunning,
    Io(std::io::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::AlreadyRunning => f.write_str("another Kairos is already running"),
            StartError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// True when a live server accepts connections at `path`. A socket file left behind by a crash
/// refuses the connection.
pub fn is_listening(path: &Path) -> bool {
    UnixStream::connect(path).is_ok()
}

impl BridgeServer {
    pub fn new(path: PathBuf) -> Self {
        BridgeServer {
            path,
            clients: Arc::new(Mutex::new(HashMap::new())),
            listener: Arc::new(Mutex::new(None)),
            bound: Arc::new(Mutex::new(None)),
        }
    }

    /// Fails with `AlreadyRunning` rather than take the socket from a live server.
    pub fn start(&self, on_event: Arc<dyn Fn(BridgeEvent) + Send + Sync>) -> Result<(), StartError> {
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(StartError::Io)?;
        }
        let lock = bind_lock(&self.path);
        if is_listening(&self.path) {
            return Err(StartError::AlreadyRunning);
        }
        let _ = std::fs::remove_file(&self.path);
        let listener = UnixListener::bind(&self.path).map_err(StartError::Io)?;
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        *self.bound.lock().unwrap() = file_id(&self.path);
        drop(lock);
        let accept = listener.try_clone().map_err(StartError::Io)?;
        *self.listener.lock().unwrap() = Some(listener);
        let clients = self.clients.clone();
        std::thread::spawn(move || {
            let mut next: ClientId = 1;
            for conn in accept.incoming() {
                let Ok(stream) = conn else { break };
                let id = next;
                next += 1;
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                let Ok(reader) = stream.try_clone() else { continue };
                let writer = Arc::new(Mutex::new(stream));
                clients.lock().unwrap().insert(id, Client { name: String::new(), stream: writer });
                let clients = clients.clone();
                let on_event = on_event.clone();
                std::thread::spawn(move || {
                    let mut lines = BufReader::new(reader);
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        match lines.read_until(b'\n', &mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                if let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(&line) {
                                    on_event(BridgeEvent::Message(id, m));
                                }
                            }
                        }
                    }
                    clients.lock().unwrap().remove(&id);
                    on_event(BridgeEvent::ClientsChanged);
                });
            }
        });
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(l) = self.listener.lock().unwrap().take() {
            // Wakes the accept loop.
            let _ = UnixStream::connect(&self.path);
            drop(l);
        }
        for c in self.clients.lock().unwrap().values() {
            let _ = c.stream.lock().unwrap().shutdown(std::net::Shutdown::Both);
        }
        if let Some(id) = self.bound.lock().unwrap().take() {
            let _lock = bind_lock(&self.path);
            if file_id(&self.path) == Some(id) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    pub fn client_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.clients.lock().unwrap().values().map(|c| c.name.clone()).filter(|n| !n.is_empty()).collect();
        names.sort();
        names
    }

    pub fn set_name(&self, name: &str, client: ClientId) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            c.name = name.to_string();
        }
    }

    pub fn send(&self, message: &Value, client: ClientId) {
        let stream = self.clients.lock().unwrap().get(&client).map(|c| c.stream.clone());
        if let Some(s) = stream {
            let mut data = serde_json::to_vec(message).unwrap_or_default();
            data.push(b'\n');
            // The write timeout set at accept keeps a stalled browser from holding the caller.
            let _ = s.lock().unwrap().write_all(&data);
        }
    }

    /// Sends to every client, or only to clients with the given name.
    pub fn broadcast(&self, message: &Value, name: Option<&str>) {
        let ids: Vec<ClientId> = self
            .clients
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| name.map(|n| c.name == n).unwrap_or(true))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.send(message, id);
        }
    }
}

/// Chrome native messaging framing: 4-byte little-endian length, then UTF-8 JSON.
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(payload);
    out
}

/// Pulls complete frames off the front of `buffer`.
pub fn drain(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = vec![];
    loop {
        if buffer.len() < 4 {
            break;
        }
        let len = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        if buffer.len() < 4 + len {
            break;
        }
        frames.push(buffer[4..4 + len].to_vec());
        buffer.drain(..4 + len);
    }
    frames
}

/// Native messaging manifest for one browser.
pub fn host_manifest(host_path: &str) -> Value {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "Kairos Desktop bridge",
        "path": host_path,
        "type": "stdio",
        "allowed_origins": EXTENSION_IDS.iter().map(|id| format!("chrome-extension://{id}/")).collect::<Vec<_>>(),
    })
}

/// Browser name -> NativeMessagingHosts directory, relative to ~/.config.
pub const BROWSER_DIRS: &[(&str, &str)] = &[
    ("Google Chrome", "google-chrome/NativeMessagingHosts"),
    ("Chrome Beta", "google-chrome-beta/NativeMessagingHosts"),
    ("Chrome Dev", "google-chrome-unstable/NativeMessagingHosts"),
    ("Chromium", "chromium/NativeMessagingHosts"),
    ("Brave", "BraveSoftware/Brave-Browser/NativeMessagingHosts"),
    ("Microsoft Edge", "microsoft-edge/NativeMessagingHosts"),
    ("Microsoft Edge Beta", "microsoft-edge-beta/NativeMessagingHosts"),
    ("Vivaldi", "vivaldi/NativeMessagingHosts"),
    ("Opera", "opera/NativeMessagingHosts"),
    ("Helium", "net.imput.helium/NativeMessagingHosts"),
];

/// Writes the native messaging manifest for every Chromium browser that has a profile folder.
/// Returns the browsers it was installed for.
pub fn install_manifests(host_path: &Path) -> Vec<String> {
    if is_scratch() || !host_path.exists() {
        return vec![];
    }
    let json = serde_json::to_vec_pretty(&host_manifest(&host_path.to_string_lossy())).unwrap_or_default();
    let config = config_dir();
    let mut done = vec![];
    for (name, dir) in BROWSER_DIRS {
        let dir = config.join(dir);
        // Only browsers that are installed (and have been opened) have the profile folder.
        let Some(parent) = dir.parent() else { continue };
        if !parent.exists() {
            continue;
        }
        if std::fs::create_dir_all(&dir).is_ok()
            && crate::store::write_atomic(&dir.join(format!("{HOST_NAME}.json")), &json).is_ok()
        {
            done.push(name.to_string());
        }
    }
    done
}

/// The browsers whose manifest points at `host_path`.
pub fn installed_browsers(host_path: &Path) -> Vec<String> {
    let config = config_dir();
    BROWSER_DIRS
        .iter()
        .filter(|(_, dir)| {
            let file = config.join(dir).join(format!("{HOST_NAME}.json"));
            std::fs::read(&file)
                .ok()
                .and_then(|d| serde_json::from_slice::<Value>(&d).ok())
                .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(|p| Path::new(p) == host_path))
                .unwrap_or(false)
        })
        .map(|(n, _)| n.to_string())
        .collect()
}
