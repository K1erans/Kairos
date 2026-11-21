//! Writes 3D scenes for Kairos Read. The prompts are the same whichever backend sends them.

use crate::claude::{Cancel, ClaudeBackend, ClaudeError, ClaudePrompt};
use serde::{Deserialize, Serialize};

pub const THREE_VERSION: &str = "0.170.0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCandidate {
    pub name: String,
    pub kind: String,
    pub why: String,
}

pub fn import_map() -> String {
    format!(
        "<script type=\"importmap\">\n{{\"imports\":{{\"three\":\"https://unpkg.com/three@{v}/build/three.module.js\",\"three/addons/\":\"https://unpkg.com/three@{v}/examples/jsm/\"}}}}\n</script>",
        v = THREE_VERSION
    )
}

pub fn scene_system() -> String {
    format!(
        "You write interactive 3D scenes that help a reader understand what an article describes. The \
reference for quality is a physically simulated record player in the browser: the behaviour \
comes from simulating the real thing, not from playing back a canned animation.

Return one complete, standalone HTML document and nothing else: no commentary and no Markdown \
fences. It runs inside a narrow browser side panel (about 400px wide, any height) and is also \
saved as a file people open directly, so it must work with no build step and no network \
requests other than three.js.

Technical contract:
- Load three.js only through this import map, copied exactly, placed before your module script:
{}
- Import only `three` and, for camera control, `three/addons/controls/OrbitControls.js`. No \
other libraries, no external textures, models, fonts or images. Build geometry in code.
- Fill the window, handle resize, and cap devicePixelRatio at 2.
- Give the important parts short visible labels (HTML overlays positioned from projected \
coordinates work well) so the reader learns the names.
- When the article explains how something works, simulate it: step the state each frame from \
the quantities that matter and expose two to four of them as labelled sliders. When the \
subject makes sound and it helps understanding, synthesise it with Web Audio, started only \
after a click on a clearly labelled button.
- Narration sync: listen for `window.addEventListener(\"message\", ...)` events whose \
`event.data` is `{{type: \"kairos:sentence\", text}}`. When the sentence mentions one of your \
parts (match on a few keywords per part), highlight that part and ease the camera toward it; \
clear the highlight when a later sentence mentions none.
- Use a dark neutral background, readable 13px system-font UI, and keep controls in a compact \
panel that does not cover the model.
- Be accurate to the article. Where the article does not give a number, use a physically \
plausible one rather than inventing a precise-looking figure in a label.",
        import_map()
    )
}

pub const CANDIDATE_SYSTEM: &str = "You pick what in an article is worth showing as an interactive 3D scene. Choose up to three \
things the article actually describes that a reader would understand better by seeing and \
manipulating them: a physical object, a mechanism, or a process. Skip abstract ideas, people \
and anything the article only mentions in passing. If nothing qualifies, return an empty array.

Reply with only a JSON array, no prose and no code fences. Each item is an object with \
\"name\" (short noun phrase), \"kind\" (one of \"object\", \"mechanism\", \"process\") and \"why\" (one \
sentence on what the scene would show).";

/// Keeps the article within a sensible prompt size without silently dropping the middle.
pub fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let head: String = text.chars().take(limit).collect();
    format!("{head}\n\n[The article continues beyond this point; it was cut at {limit} characters.]")
}

pub fn parse_candidates(raw: &str) -> Vec<SceneCandidate> {
    let (Some(start), Some(end)) = (raw.find('['), raw.rfind(']')) else { return vec![] };
    if start >= end {
        return vec![];
    }
    serde_json::from_str::<Vec<SceneCandidate>>(&raw[start..=end]).map(|l| l.into_iter().take(3).collect()).unwrap_or_default()
}

/// Strips Markdown fences or stray prose around the document, if the model added any.
pub fn extract_html(raw: &str) -> Result<String, ClaudeError> {
    let mut s = raw.to_string();
    if let Some(i) = s.find("```") {
        let after = s[i + 3..].to_string();
        s = match after.find('\n') {
            Some(nl) => after[nl + 1..].to_string(),
            None => after,
        };
        if let Some(close) = s.rfind("```") {
            s.truncate(close);
        }
    }
    let lower = s.to_lowercase();
    let start = lower.find("<!doctype html").or_else(|| lower.find("<html")).ok_or(ClaudeError::Empty)?;
    let end = lower.rfind("</html>").ok_or(ClaudeError::Empty)? + "</html>".len();
    if end <= start {
        return Err(ClaudeError::Empty);
    }
    Ok(s[start..end].to_string())
}

pub fn generate_prompt(title: &str, text: &str, subject: &str) -> ClaudePrompt {
    let user = format!(
        "Build a scene of: {subject}\n\nIt should show what this article says about it, so someone listening to the article can \
follow along.\n\nArticle title: {title}\n\n<article>\n{}\n</article>",
        clip(text, 60_000)
    );
    ClaudePrompt { system: scene_system(), user, effort: "high".into(), max_tokens: 64000 }
}

pub struct SceneWriter {
    pub backend: Box<dyn ClaudeBackend>,
}

impl SceneWriter {
    pub fn candidates(&self, title: &str, text: &str, cancel: &Cancel) -> Result<Vec<SceneCandidate>, ClaudeError> {
        let user = format!("Article title: {title}\n\n<article>\n{}\n</article>", clip(text, 60_000));
        let raw = self.backend.complete(
            &ClaudePrompt { system: CANDIDATE_SYSTEM.into(), user, effort: "low".into(), max_tokens: 4000 },
            cancel,
        )?;
        Ok(parse_candidates(&raw))
    }

    pub fn generate(&self, title: &str, text: &str, subject: &str, cancel: &Cancel) -> Result<String, ClaudeError> {
        extract_html(&self.backend.complete(&generate_prompt(title, text, subject), cancel)?)
    }

    pub fn edit(&self, html: &str, instruction: &str, cancel: &Cancel) -> Result<String, ClaudeError> {
        let user = format!(
            "Here is a scene you wrote earlier. Change it as asked and return the complete updated \
HTML document, keeping everything that was not asked to change.\n\nRequested change: {instruction}\n\n<scene>\n{html}\n</scene>"
        );
        let raw = self.backend.complete(&ClaudePrompt { system: scene_system(), user, effort: "high".into(), max_tokens: 64000 }, cancel)?;
        extract_html(&raw)
    }
}
