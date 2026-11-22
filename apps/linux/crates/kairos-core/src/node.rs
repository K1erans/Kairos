//! Finds `node` for the scene helper, and the helper itself. An app started from the menu gets
//! the session's PATH, which usually lacks nvm's and Volta's folders, so the usual places are
//! tried by name first, then the login shell is asked, the way a terminal would find it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn candidates(home: &str, nvm_versions: &[String]) -> Vec<String> {
    let mut list = vec![
        format!("{home}/.volta/bin/node"),
        "/usr/local/bin/node".to_string(),
        "/usr/bin/node".to_string(),
    ];
    let mut versions = nvm_versions.to_vec();
    versions.sort_by_key(|v| std::cmp::Reverse(version(v)));
    list.extend(versions.iter().map(|v| format!("{home}/.nvm/versions/node/{v}/bin/node")));
    list
}

/// "v22.11.0" -> [22, 11, 0]
pub fn version(name: &str) -> Vec<u64> {
    let trimmed = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    trimmed
        .split('.')
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0))
        .collect()
}

pub fn find(home: &str, nvm_versions: &[String], is_executable: &dyn Fn(&str) -> bool, login_shell: &dyn Fn() -> Option<String>) -> Option<String> {
    if let Some(found) = candidates(home, nvm_versions).into_iter().find(|c| is_executable(c)) {
        return Some(found);
    }
    let answer = login_shell()?;
    let path = answer.lines().map(|l| l.trim()).filter(|l| l.starts_with('/')).last()?.to_string();
    if is_executable(&path) {
        Some(path)
    } else {
        None
    }
}

fn is_executable(p: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

static CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `find` on this machine. A path found is kept for the rest of the run; not finding one is not.
pub fn locate() -> Option<PathBuf> {
    if let Some(p) = CACHE.lock().unwrap().clone() {
        return Some(p);
    }
    let home = crate::store::home_dir().to_string_lossy().to_string();
    let nvm: Vec<String> = std::fs::read_dir(format!("{home}/.nvm/versions/node"))
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    let found = find(&home, &nvm, &is_executable, &ask_login_shell).map(PathBuf::from);
    if let Some(f) = &found {
        *CACHE.lock().unwrap() = Some(f.clone());
    }
    found
}

/// `command -v node` in the owner's login shell, given three seconds.
fn ask_login_shell() -> Option<String> {
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/bash".into());
    let mut child = Command::new(shell)
        .args(["-l", "-c", "command -v node"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

/// The scene helper, `scene-agent.mjs` with its node_modules, which the installer puts in
/// `<share>/kairos/scene-agent`. `KAIROS_SCENE_AGENT` points elsewhere.
pub fn helper_url(resources: Option<&Path>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KAIROS_SCENE_AGENT") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    resources.map(|r| r.join("scene-agent/scene-agent.mjs"))
}

pub fn helper_installed(url: Option<&Path>) -> bool {
    let Some(url) = url else { return false };
    url.exists() && url.parent().map(|d| d.join("node_modules/@anthropic-ai/claude-agent-sdk").exists()).unwrap_or(false)
}

/// What the Claude subscription backend needs, and the line Settings shows for it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SceneSetup {
    pub node: Option<PathBuf>,
    pub helper: Option<PathBuf>,
    pub helper_installed: bool,
}

impl SceneSetup {
    pub fn new(node: Option<PathBuf>, helper: Option<PathBuf>) -> Self {
        let installed = helper_installed(helper.as_deref());
        SceneSetup { node, helper, helper_installed: installed }
    }

    pub fn status(&self, backend: crate::models::SceneBackend, has_claude_key: bool) -> (bool, String) {
        use crate::models::SceneBackend;
        match backend {
            SceneBackend::ApiKey => {
                if has_claude_key {
                    (true, "Scenes use the Anthropic key under API keys, billed to that API account.".into())
                } else {
                    (false, "Add an Anthropic key under API keys, or choose \u{201C}Claude subscription\u{201D}.".into())
                }
            }
            SceneBackend::Subscription => {
                let Some(node) = &self.node else {
                    return (false, "Node.js wasn\u{2019}t found. Install it (for example with nvm), then come back to this page.".into());
                };
                if !self.helper_installed {
                    return (false, "This copy of Kairos has no scene helper. Reinstall it with apps/linux/scripts/install.sh.".into());
                }
                (true, format!("Ready: scenes go through Claude Code on your Claude subscription, with Node.js at {}. If Claude Code isn\u{2019}t logged in yet, run claude in a terminal, then /login.", node.display()))
            }
        }
    }
}
