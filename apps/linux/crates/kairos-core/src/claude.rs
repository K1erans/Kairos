//! Sends prompts to Claude for Kairos Read's 3D scenes: through the Claude Agent SDK helper on the
//! owner's Claude subscription, or through the Messages API with an Anthropic key. Blocking; the
//! app runs these on worker threads and cancels them through a `Cancel`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const MODEL: &str = "claude-opus-5-5";

#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeError {
    NoKey,
    Http(u16, String),
    Refused(String),
    Truncated,
    Empty,
    NoNode,
    NoHelper,
    NotLoggedIn,
    Helper(String),
    Cancelled,
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::NoKey => f.write_str("No Anthropic API key is set. Add one in Kairos Desktop > Settings, or choose \u{201C}Claude subscription\u{201D} under 3D scenes."),
            ClaudeError::Http(code, body) => write!(f, "Claude returned HTTP {code}: {}", body.chars().take(300).collect::<String>()),
            ClaudeError::Refused(why) => {
                if why.is_empty() { f.write_str("Claude declined this request.") } else { write!(f, "Claude declined this request ({why}).") }
            }
            ClaudeError::Truncated => f.write_str("The scene was too long and got cut off. Try a simpler subject."),
            ClaudeError::Empty => f.write_str("Claude returned nothing usable."),
            ClaudeError::NoNode => f.write_str("Kairos couldn\u{2019}t find Node.js, which it needs to write scenes with your Claude subscription. Install it (for example with nvm), or choose \u{201C}Anthropic API key\u{201D} in Kairos Desktop > Settings > 3D scenes."),
            ClaudeError::NoHelper => f.write_str("Kairos is missing its scene helper. Reinstall Kairos with apps/linux/scripts/install.sh, or choose \u{201C}Anthropic API key\u{201D} in Kairos Desktop > Settings > 3D scenes."),
            ClaudeError::NotLoggedIn => f.write_str("Claude Code isn\u{2019}t logged in on this computer. Open a terminal, run \u{201C}claude\u{201D}, type /login, then try again."),
            ClaudeError::Helper(line) => write!(f, "Claude couldn\u{2019}t write the scene: {line}"),
            ClaudeError::Cancelled => f.write_str("The scene build was cancelled."),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClaudePrompt {
    pub system: String,
    pub user: String,
    /// "low", "medium" or "high".
    pub effort: String,
    pub max_tokens: u32,
}

/// Set from any thread to stop a request.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub trait ClaudeBackend: Send + Sync {
    fn complete(&self, prompt: &ClaudePrompt, cancel: &Cancel) -> Result<String, ClaudeError>;
}

// MARK: Claude subscription

/// Runs `node scene-agent.mjs`; the prompt goes to its stdin as {"system","user","effort","model"},
/// the answer comes back on stdout, a failure is one line on stderr and a non-zero exit.
pub struct ClaudeSubscription {
    pub node: Option<PathBuf>,
    pub helper: Option<PathBuf>,
    pub model: String,
}

impl ClaudeSubscription {
    pub fn new(node: Option<PathBuf>, helper: Option<PathBuf>) -> Self {
        ClaudeSubscription { node, helper, model: MODEL.into() }
    }

    pub fn input(prompt: &ClaudePrompt, model: &str) -> Vec<u8> {
        let v = json!({"effort": prompt.effort, "model": model, "system": prompt.system, "user": prompt.user});
        serde_json::to_vec(&v).unwrap_or_default()
    }

    /// What a failed helper run means, from its stderr.
    pub fn failure(stderr: &str) -> ClaudeError {
        if stderr.contains("ERR_MODULE_NOT_FOUND") || stderr.contains("Cannot find package") {
            return ClaudeError::NoHelper;
        }
        let line = stderr
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with("at ") && !l.starts_with("Node.js v"))
            .last()
            .unwrap_or("");
        let lower = line.to_lowercase();
        let hints = ["/login", "not logged in", "log in", "authentication", "invalid api key", "oauth", "401"];
        if hints.iter().any(|h| lower.contains(h)) {
            return ClaudeError::NotLoggedIn;
        }
        if line.is_empty() {
            ClaudeError::Helper("the scene helper stopped without saying why.".into())
        } else {
            ClaudeError::Helper(line.chars().take(300).collect())
        }
    }
}

impl ClaudeBackend for ClaudeSubscription {
    fn complete(&self, prompt: &ClaudePrompt, cancel: &Cancel) -> Result<String, ClaudeError> {
        let node = self.node.as_ref().ok_or(ClaudeError::NoNode)?;
        let helper = self.helper.as_ref().filter(|h| h.exists()).ok_or(ClaudeError::NoHelper)?;
        // As on the Mac: a request cancelled before it starts never starts the helper.
        if cancel.is_cancelled() {
            return Err(ClaudeError::Cancelled);
        }
        let mut path: Vec<String> = vec![node.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()];
        path.extend(std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()).split(':').map(String::from));
        let mut child = Command::new(node)
            .arg(helper)
            .current_dir(helper.parent().unwrap_or(helper))
            // An API key here would be billed instead of the subscription; the helper drops it too.
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env("PATH", path.join(":"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ClaudeError::Helper(e.to_string()))?;
        let input = Self::input(prompt, &self.model);
        let mut stdin = child.stdin.take();
        std::thread::spawn(move || {
            if let Some(s) = stdin.as_mut() {
                let _ = s.write_all(&input);
            }
        });
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let out_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_end(&mut b);
            }
            b
        });
        let err_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_end(&mut b);
            }
            b
        });
        let status = loop {
            if cancel.is_cancelled() {
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGTERM);
                }
                let _ = child.wait();
                return Err(ClaudeError::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(ClaudeError::Helper(e.to_string())),
            }
        };
        let stdout = out_t.join().unwrap_or_default();
        let stderr = err_t.join().unwrap_or_default();
        if cancel.is_cancelled() {
            return Err(ClaudeError::Cancelled);
        }
        if !status.success() {
            return Err(Self::failure(&String::from_utf8_lossy(&stderr)));
        }
        let text = String::from_utf8_lossy(&stdout).trim().to_string();
        if text.is_empty() {
            return Err(ClaudeError::Empty);
        }
        Ok(text)
    }
}

// MARK: Anthropic API key

pub const API_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// The Messages API over raw HTTP, billed to an Anthropic API key. Requests stream so long scenes
/// cannot time out.
pub struct ClaudeApi {
    pub api_key: Option<String>,
}

pub fn api_request_body(prompt: &ClaudePrompt) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": prompt.max_tokens,
        "stream": true,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": prompt.effort},
        // If a safety classifier declines, the API retries on Anthropic's recommended model.
        "fallbacks": "default",
        "system": prompt.system,
        "messages": [{"role": "user", "content": prompt.user}],
    })
}

impl ClaudeBackend for ClaudeApi {
    fn complete(&self, prompt: &ClaudePrompt, cancel: &Cancel) -> Result<String, ClaudeError> {
        let key = self.api_key.as_deref().filter(|k| !k.is_empty()).ok_or(ClaudeError::NoKey)?;
        // As on the Mac: a request cancelled before it starts is never sent.
        if cancel.is_cancelled() {
            return Err(ClaudeError::Cancelled);
        }
        let agent = ureq::AgentBuilder::new().timeout_read(Duration::from_secs(600)).build();
        let resp = agent
            .post(API_ENDPOINT)
            .set("x-api-key", key)
            .set("anthropic-version", "2026-10-21")
            .set("anthropic-beta", "server-side-fallback-2026-07-01")
            .set("Content-Type", "application/json")
            .send_string(&api_request_body(prompt).to_string());
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let text: String = r.into_string().unwrap_or_default().chars().take(2000).collect();
                return Err(ClaudeError::Http(code, text));
            }
            Err(e) => {
                if cancel.is_cancelled() {
                    return Err(ClaudeError::Cancelled);
                }
                return Err(ClaudeError::Http(0, e.to_string()));
            }
        };
        let mut parser = StreamParser::default();
        for line in BufReader::new(resp.into_reader()).lines() {
            if cancel.is_cancelled() {
                return Err(ClaudeError::Cancelled);
            }
            let line = line.map_err(|e| ClaudeError::Http(0, e.to_string()))?;
            parser.consume(&line)?;
        }
        if cancel.is_cancelled() {
            return Err(ClaudeError::Cancelled);
        }
        parser.result()
    }
}

/// Accumulates text from the Messages API's server-sent events.
#[derive(Default)]
pub struct StreamParser {
    pub text: String,
    pub stop_reason: Option<String>,
    refusal_category: String,
}

impl StreamParser {
    pub fn consume(&mut self, line: &str) -> Result<(), ClaudeError> {
        let Some(payload) = line.strip_prefix("data:") else { return Ok(()) };
        let Ok(obj) = serde_json::from_str::<Value>(payload.trim()) else { return Ok(()) };
        let Some(kind) = obj.get("type").and_then(|t| t.as_str()) else { return Ok(()) };
        match kind {
            "content_block_delta" => {
                if let Some(d) = obj.get("delta") {
                    if d.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                        if let Some(t) = d.get("text").and_then(|t| t.as_str()) {
                            self.text.push_str(t);
                        }
                    }
                }
            }
            "content_block_start" => {
                if let Some(b) = obj.get("content_block") {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            self.text.push_str(t);
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(d) = obj.get("delta") {
                    if let Some(r) = d.get("stop_reason").and_then(|r| r.as_str()) {
                        self.stop_reason = Some(r.to_string());
                    }
                    if let Some(c) = d.get("stop_details").and_then(|s| s.get("category")).and_then(|c| c.as_str()) {
                        self.refusal_category = c.to_string();
                    }
                }
            }
            "error" => {
                let msg = obj.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("stream error");
                return Err(ClaudeError::Http(0, msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn result(&self) -> Result<String, ClaudeError> {
        match self.stop_reason.as_deref() {
            Some("refusal") => return Err(ClaudeError::Refused(self.refusal_category.clone())),
            Some("max_tokens") => return Err(ClaudeError::Truncated),
            _ => {}
        }
        let t = self.text.trim();
        if t.is_empty() {
            Err(ClaudeError::Empty)
        } else {
            Ok(t.to_string())
        }
    }
}
