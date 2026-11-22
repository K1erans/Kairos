//! The things the owner sets up once so Kairos can do everything it offers. Today lists them
//! until they are done or the owner hides the list.

use crate::models::{SceneBackend, VoiceProvider};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupStep {
    Shield,
    Read,
    JevKey,
    Scenes,
    VoiceKey,
    OpenAtLogin,
    DockReplacement,
}

#[derive(Clone, Debug, Default)]
pub struct SetupFacts {
    pub seen_clients: HashSet<String>,
    pub has_jev_key: bool,
    pub has_claude_key: bool,
    pub scene_backend: SceneBackend,
    pub has_node: bool,
    pub has_scene_helper: bool,
    pub voice_provider: VoiceProvider,
    pub has_voice_key: bool,
    pub open_at_login: bool,
    pub replaces_dock: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetupItem {
    pub step: SetupStep,
    pub done: bool,
    pub title: String,
    pub detail: String,
}

fn item(step: SetupStep, done: bool, title: &str, detail: &str) -> SetupItem {
    SetupItem { step, done, title: title.into(), detail: detail.into() }
}

pub struct SetupChecklist {
    pub items: Vec<SetupItem>,
}

impl SetupChecklist {
    pub fn new(f: &SetupFacts) -> Self {
        let mut items = vec![
            item(SetupStep::Shield, f.seen_clients.contains("shield"), "Load Kairos Shield",
                 "Blocks sites and hides page elements. Without it Kairos can\u{2019}t block any site. In chrome://extensions turn on Developer mode, choose \u{201C}Load unpacked\u{201D} and pick the shield folder."),
            item(SetupStep::Read, f.seen_clients.contains("read"), "Load Kairos Read (optional)",
                 "Reads articles aloud and builds 3D scenes from them. Load the read folder the same way."),
            item(SetupStep::JevKey, f.has_jev_key, "Add a TypeSafe key for Jev",
                 "Jev checks whether a page or app fits your current task. Without it only your block and allow lists apply."),
            Self::scenes(f),
        ];
        if f.voice_provider != VoiceProvider::System {
            let label = f.voice_provider.label();
            items.push(SetupItem {
                step: SetupStep::VoiceKey,
                done: f.has_voice_key,
                title: format!("Add your {label} key"),
                detail: format!("You chose {label} voices for Kairos Read. Until the key is saved it reads with built-in voices."),
            });
        }
        items.push(item(SetupStep::OpenAtLogin, f.open_at_login, "Open Kairos at login",
                        "So blocking and Kairos Bar are there from the moment you log in."));
        items.push(item(SetupStep::DockReplacement, f.replaces_dock, "Use Kairos Bar instead of the panel (optional)",
                        "Kairos hides the Cinnamon panel while Kairos Bar is on screen and puts it back when you turn this off or quit."));
        SetupChecklist { items }
    }

    fn scenes(f: &SetupFacts) -> SetupItem {
        if f.scene_backend != SceneBackend::Subscription {
            return item(SetupStep::Scenes, f.has_claude_key, "Add an Anthropic key (optional)",
                        "For 3D scenes in Kairos Read. Or choose \u{201C}Claude subscription\u{201D} in Settings > 3D scenes to use your Claude plan instead.");
        }
        if f.has_node && f.has_scene_helper {
            return item(SetupStep::Scenes, true, "Claude subscription ready",
                        "3D scenes in Kairos Read go through Claude Code, on your Claude subscription.");
        }
        let detail = if f.has_node {
            "This copy of Kairos has no scene helper. Reinstall it with apps/linux/scripts/install.sh, or use an Anthropic key in Settings > 3D scenes."
        } else {
            "Kairos Read\u{2019}s 3D scenes go through Claude Code on your Claude subscription, which needs Node.js. Install it (for example with nvm or apt install nodejs) and log in once in a terminal with claude, then /login."
        };
        item(SetupStep::Scenes, false, "Set up 3D scenes on your Claude subscription (optional)", detail)
    }

    pub fn done_count(&self) -> usize {
        self.items.iter().filter(|i| i.done).count()
    }
    pub fn is_complete(&self) -> bool {
        self.done_count() == self.items.len()
    }
}
