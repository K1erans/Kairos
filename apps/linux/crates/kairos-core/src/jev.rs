//! Client for TypeSafe AI's Jev (POST /v1/systemone). One `choice` question per check.

use crate::time::{self, Date};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

#[derive(Clone, Debug, PartialEq)]
pub struct JevVerdict {
    pub choice: String,
    pub confidence: f64,
    pub probabilities: BTreeMap<String, f64>,
    pub cached: bool,
}

impl JevVerdict {
    pub fn unsure() -> Self {
        JevVerdict { choice: "unsure".into(), confidence: 0.0, probabilities: BTreeMap::new(), cached: false }
    }

    /// Block when the probability of "distracting" reaches the group's threshold.
    pub fn should_block(&self, threshold: f64) -> bool {
        let p = self
            .probabilities
            .get("distracting")
            .copied()
            .unwrap_or(if self.choice == "distracting" { self.confidence } else { 0.0 });
        p >= threshold
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JevError {
    NoKey,
    Http(u16, String),
    BadResponse,
    Network(String),
}

impl std::fmt::Display for JevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JevError::NoKey => f.write_str("No TypeSafe API key is set."),
            JevError::Http(code, body) => write!(f, "Jev returned HTTP {code}: {}", body.chars().take(200).collect::<String>()),
            JevError::BadResponse => f.write_str("Jev returned an answer Kairos could not read."),
            JevError::Network(e) => write!(f, "Jev could not be reached: {e}"),
        }
    }
}

/// What is being judged: a web page or an app window.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Subject {
    pub kind: String,
    pub url: String,
    pub title: String,
    pub description: String,
    pub excerpt: String,
    pub app: String,
}

impl Subject {
    pub fn new(kind: &str) -> Self {
        Subject { kind: kind.into(), ..Default::default() }
    }
    fn cache_key(&self) -> String {
        if self.kind == "page" {
            self.url.clone()
        } else {
            format!("{}|{}", self.app, self.title)
        }
    }
}

pub fn request_body(task: &str, group: &str, subject: &Subject) -> Value {
    let mut state = serde_json::Map::new();
    state.insert("task".into(), json!(task));
    state.insert("group".into(), json!(group));
    if subject.kind == "page" {
        state.insert("url".into(), json!(subject.url));
        state.insert("title".into(), json!(subject.title));
        if !subject.description.is_empty() {
            state.insert("description".into(), json!(subject.description));
        }
        state.insert("excerpt".into(), json!(subject.excerpt));
    } else {
        state.insert("app".into(), json!(subject.app));
        if !subject.title.is_empty() {
            state.insert("window_title".into(), json!(subject.title));
        }
    }
    let noun = if subject.kind == "page" { "page" } else { "app window" };
    json!({
        "model": "jev-latest",
        "state": state,
        "questions": {
            "focus": {
                "type": "choice",
                "instructions": format!("Is this {noun} useful for the user's current task?"),
                "criteria": {
                    "relevant": "Directly helps with the task, or is a tool the task needs",
                    "distracting": "Unrelated entertainment, social media, shopping or news",
                    "unsure": "Could be either",
                },
            },
        },
    })
}

pub fn parse(data: &[u8]) -> Result<JevVerdict, JevError> {
    let root: Value = serde_json::from_slice(data).map_err(|_| JevError::BadResponse)?;
    let focus = root.get("answers").and_then(|a| a.get("focus")).ok_or(JevError::BadResponse)?;
    let choice = focus.get("choice").and_then(|c| c.as_str()).ok_or(JevError::BadResponse)?.to_string();
    let mut probs = BTreeMap::new();
    if let Some(Value::Object(m)) = focus.get("probabilities") {
        for (k, v) in m {
            if let Some(n) = v.as_f64() {
                probs.insert(k.clone(), n);
            }
        }
    }
    let confidence = focus.get("confidence").and_then(|c| c.as_f64()).or_else(|| probs.get(&choice).copied()).unwrap_or(0.0);
    Ok(JevVerdict { choice, confidence, probabilities: probs, cached: false })
}

pub struct JevClient {
    cache: Mutex<HashMap<String, (JevVerdict, Date)>>,
    pub cache_ttl: f64,
    pub timeout: Duration,
}

impl Default for JevClient {
    fn default() -> Self {
        JevClient { cache: Mutex::new(HashMap::new()), cache_ttl: 24.0 * 3600.0, timeout: Duration::from_secs(2) }
    }
}

impl JevClient {
    /// Blocking: call off the UI thread.
    pub fn check(&self, api_key: Option<&str>, task: &str, group: &str, group_id: &str, subject: &Subject) -> Result<JevVerdict, JevError> {
        let key = api_key.filter(|k| !k.is_empty()).ok_or(JevError::NoKey)?;
        let now = time::now();
        let cache_key = format!("{group_id}|{task}|{}", subject.cache_key());
        if let Some((v, at)) = self.cache.lock().unwrap().get(&cache_key) {
            if time::since(now, *at) < self.cache_ttl {
                let mut hit = v.clone();
                hit.cached = true;
                return Ok(hit);
            }
        }
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let resp = agent
            .post(ENDPOINT)
            .set("Authorization", &format!("Bearer {key}"))
            .set("Content-Type", "application/json")
            .send_string(&request_body(task, group, subject).to_string());
        let body = match resp {
            Ok(r) => r.into_string().map_err(|e| JevError::Network(e.to_string()))?,
            Err(ureq::Error::Status(code, r)) => return Err(JevError::Http(code, r.into_string().unwrap_or_default())),
            Err(e) => return Err(JevError::Network(e.to_string())),
        };
        let verdict = parse(body.as_bytes())?;
        self.cache.lock().unwrap().insert(cache_key, (verdict.clone(), now));
        Ok(verdict)
    }
}

/// Whether Jev can judge anything for the active group, and if not, why.
#[derive(Clone, Debug, PartialEq)]
pub enum JevReadiness {
    Off,
    Ready,
    NoKey,
    NoTask,
    Failing(String),
}

impl JevReadiness {
    pub fn new(enabled: bool, has_key: bool, has_task: bool, last_error: Option<&str>) -> Self {
        if !enabled {
            JevReadiness::Off
        } else if !has_key {
            JevReadiness::NoKey
        } else if !has_task {
            JevReadiness::NoTask
        } else if let Some(e) = last_error {
            JevReadiness::Failing(e.to_string())
        } else {
            JevReadiness::Ready
        }
    }

    pub fn problem(&self) -> Option<String> {
        match self {
            JevReadiness::Off | JevReadiness::Ready => None,
            JevReadiness::NoKey => Some("Jev is on but has no TypeSafe API key, so only your block lists apply. Add the key in Settings.".into()),
            JevReadiness::NoTask => Some("Jev is on but there is no current task to compare with, so it is not judging anything. Start a task.".into()),
            JevReadiness::Failing(e) => Some(format!("Jev could not answer the last check: {e}")),
        }
    }
}
