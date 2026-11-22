//! Where API keys are kept. The real profile keeps them in the login keyring (Secret Service,
//! which the app talks to). A scratch copy (`KAIROS_HOME`) keeps them in a plain file in its
//! scratch folder instead, so it never reads the owner's keyring.

use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum KeyStore {
    Keyring,
    File(PathBuf),
}

impl KeyStore {
    pub fn at(kairos_home: Option<&str>) -> KeyStore {
        match kairos_home {
            Some(h) if !h.is_empty() => KeyStore::File(PathBuf::from(h).join("keys.json")),
            _ => KeyStore::Keyring,
        }
    }
    pub fn current() -> KeyStore {
        KeyStore::at(std::env::var("KAIROS_HOME").ok().as_deref())
    }
}

/// A scratch copy's keys: account -> key, as JSON.
pub struct ScratchKeys {
    pub path: PathBuf,
}

impl ScratchKeys {
    fn all(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.path).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
    }

    pub fn get(&self, account: &str) -> Option<String> {
        self.all().get(account).cloned()
    }

    /// An empty value removes the key.
    pub fn set(&self, value: &str, account: &str) -> bool {
        let mut keys = self.all();
        if value.is_empty() {
            keys.remove(account);
        } else {
            keys.insert(account.to_string(), value.to_string());
        }
        let data = serde_json::to_vec(&keys).unwrap_or_default();
        crate::store::write_atomic(&self.path, &data).is_ok()
    }
}
