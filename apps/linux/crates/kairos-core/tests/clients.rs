mod common;
use common::*;
use kairos_core::claude::{api_request_body, Cancel, ClaudeApi, ClaudeBackend, ClaudeError, ClaudePrompt, ClaudeSubscription, StreamParser, MODEL};
use kairos_core::jev::{self, JevReadiness, JevVerdict, Subject};
use kairos_core::node;
use kairos_core::privacy;
use kairos_core::scenes::{self, SceneCandidate, SceneWriter};
use kairos_core::setup::{SetupChecklist, SetupFacts, SetupStep};
use kairos_core::voice::{self, CloudVoice};
use kairos_core::{SceneBackend, VoiceProvider};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// MARK: Jev

#[test]
fn jev_parse_and_threshold() {
    let json = r#"{"model":"jev-1.13.0","answers":{"focus":{"choice":"distracting","confidence":0.93,"probabilities":{"relevant":0.03,"distracting":0.93,"unsure":0.04}}},"usage":{"input_tokens":700,"output_tokens":0}}"#;
    let v = jev::parse(json.as_bytes()).unwrap();
    assert_eq!(v.choice, "distracting");
    assert!(v.should_block(0.7));
    assert!(!v.should_block(0.95));
    assert!(!JevVerdict::unsure().should_block(0.7));
    assert!(jev::parse(b"{}").is_err());
}

#[test]
fn the_documented_choice_answer_parses() {
    let json = r#"{"model":"jev-latest","answers":{"focus":{"type":"choice","choice":"relevant","confidence":0.7,"probabilities":{"relevant":0.8,"distracting":0.15,"unsure":0.05}}},"usage":{"input_tokens":120,"output_tokens":12}}"#;
    let v = jev::parse(json.as_bytes()).unwrap();
    assert_eq!(v.choice, "relevant");
    assert!(!v.should_block(0.7));
}

#[test]
fn every_question_is_a_documented_choice_question() {
    let mut page = Subject::new("page");
    page.url = "https://x.com".into();
    page.title = "X".into();
    page.excerpt = "hi".into();
    let mut app = Subject::new("app");
    app.app = "Notes".into();
    app.title = "Draft".into();
    for subject in [page, app] {
        let b = jev::request_body("Write report", "Work", &subject);
        assert_eq!(b["model"], "jev-latest");
        assert!(b["state"].is_object());
        let questions = b["questions"].as_object().unwrap();
        assert!(!questions.is_empty());
        for q in questions.values() {
            assert_eq!(q["type"], "choice", "ChoiceQuestion.type is required");
            let criteria = q["criteria"].as_object().unwrap();
            assert!((1..=255).contains(&criteria.len()));
            let mut keys: Vec<&String> = criteria.keys().collect();
            keys.sort();
            assert_eq!(keys, vec!["distracting", "relevant", "unsure"]);
        }
    }
}

#[test]
fn jev_that_is_on_but_cannot_run_says_why() {
    assert_eq!(JevReadiness::new(true, false, true, None), JevReadiness::NoKey);
    assert_eq!(JevReadiness::new(true, true, false, None), JevReadiness::NoTask);
    assert_eq!(JevReadiness::new(true, false, false, None), JevReadiness::NoKey, "the key comes first");
    assert_eq!(JevReadiness::new(true, true, true, Some("HTTP 422")), JevReadiness::Failing("HTTP 422".into()));
    for state in [JevReadiness::NoKey, JevReadiness::NoTask, JevReadiness::Failing("HTTP 422".into())] {
        assert!(state.problem().is_some());
    }
}

#[test]
fn nothing_to_say_when_jev_runs_or_is_off() {
    assert_eq!(JevReadiness::new(true, true, true, None), JevReadiness::Ready);
    assert_eq!(JevReadiness::new(false, false, false, Some("x")), JevReadiness::Off);
    assert_eq!(JevReadiness::Ready.problem(), None);
    assert_eq!(JevReadiness::Off.problem(), None);
}

// MARK: Claude stream

#[test]
fn stream_parser() {
    let mut p = StreamParser::default();
    p.consume("event: content_block_delta").unwrap();
    p.consume(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hm"}}"#).unwrap();
    p.consume(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"<!doctype html><html>"}}"#).unwrap();
    p.consume(r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"</html>"}}"#).unwrap();
    p.consume(r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}"#).unwrap();
    assert_eq!(p.result().unwrap(), "<!doctype html><html></html>");

    let mut refused = StreamParser::default();
    refused.consume(r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}}}"#).unwrap();
    assert!(refused.result().is_err());
}

fn parse_events(events: &[&str]) -> StreamParser {
    let mut p = StreamParser::default();
    for data in events {
        let v: Value = serde_json::from_str(data).unwrap();
        p.consume(&format!("event: {}", v["type"].as_str().unwrap())).unwrap();
        p.consume(&format!("data: {data}")).unwrap();
        p.consume("").unwrap();
    }
    p
}

#[test]
fn a_mid_stream_fallback_continues_the_partial_scene() {
    let p = parse_events(&[
        r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<!doctype html><html><head><title>Heart"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
        r#"{"type":"content_block_stop","index":1}"#,
        r#"{"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"</title></head><body></body></html>"}}"#,
        r#"{"type":"content_block_stop","index":2}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":40}}"#,
        r#"{"type":"message_stop"}"#,
    ]);
    assert_eq!(p.result().unwrap(), "<!doctype html><html><head><title>Heart</title></head><body></body></html>");
}

#[test]
fn a_fallback_before_any_output_is_seamless() {
    let p = parse_events(&[
        r#"{"type":"message_start","message":{"id":"msg_2","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"stop_reason":null}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"<html></html>"}}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
    ]);
    assert_eq!(p.result().unwrap(), "<html></html>");
}

#[test]
fn a_refusal_after_every_fallback_drops_the_partial() {
    let p = parse_events(&[
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<html>"}}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}}}"#,
    ]);
    assert_eq!(p.result(), Err(ClaudeError::Refused("cyber".into())));
}

// MARK: Scenes

#[test]
fn extract_html_and_candidates() {
    let wrapped = "Here you go:\n```html\n<!DOCTYPE html>\n<html><body>x</body></html>\n```\nEnjoy";
    assert_eq!(scenes::extract_html(wrapped).unwrap(), "<!DOCTYPE html>\n<html><body>x</body></html>");
    assert!(scenes::extract_html("sorry, no").is_err());
    let c = scenes::parse_candidates(r#"Sure: [{"name":"Jet engine","kind":"mechanism","why":"Shows airflow."}]"#);
    assert_eq!(c, vec![SceneCandidate { name: "Jet engine".into(), kind: "mechanism".into(), why: "Shows airflow.".into() }]);
    assert_eq!(scenes::parse_candidates("none"), vec![]);
    let body = api_request_body(&ClaudePrompt { system: "s".into(), user: "u".into(), effort: "low".into(), max_tokens: 10 });
    assert!(serde_json::to_string(&body).is_ok());
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["max_tokens"], 10);
}

// MARK: Scene backends

fn prompt() -> ClaudePrompt {
    ClaudePrompt { system: "sys".into(), user: "hello".into(), effort: "low".into(), max_tokens: 10 }
}

/// A fake `scene-agent.mjs`: `body` runs with HERE set to its folder, with /bin/sh as node.
fn helper(dir: &Path, body: &str) -> ClaudeSubscription {
    let url = dir.join("scene-agent.sh");
    std::fs::write(&url, format!("HERE=$(dirname \"$0\")\n{body}\n")).unwrap();
    ClaudeSubscription::new(Some(PathBuf::from("/bin/sh")), Some(url))
}

#[test]
fn the_helper_gets_the_prompt_on_stdin_and_its_answer_comes_back() {
    // The test process has a key in its environment; the helper must not see it.
    std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-pass");
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"cat > "$HERE/input.json"; env > "$HERE/env.txt"; printf 'An answer'"#);
    assert_eq!(backend.complete(&prompt(), &Cancel::default()).unwrap(), "An answer");
    let input: Map<String, Value> = serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(Value::Object(input), json!({"system": "sys", "user": "hello", "effort": "low", "model": "claude-opus-5-5"}));
    let env = std::fs::read_to_string(dir.join("env.txt")).unwrap();
    assert!(env.lines().any(|l| l.starts_with("PATH=/bin:")), "node's folder comes first, for anything it starts");
    assert!(!env.contains("ANTHROPIC_API_KEY"), "an API key would be billed instead of the subscription");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_generated_scene_is_the_document_in_the_answer() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"cat > "$HERE/input.json"; printf 'Here it is:\n```html\n<!doctype html><html><body>clock</body></html>\n```\n'"#);
    let writer = SceneWriter { backend: Box::new(backend) };
    let html = writer.generate("Clocks", "A pendulum swings.", "Pendulum clock", &Cancel::default()).unwrap();
    assert_eq!(html, "<!doctype html><html><body>clock</body></html>");
    let input: Value = serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(input["system"], scenes::scene_system());
    assert_eq!(input["effort"], "high");
    assert!(input["user"].as_str().unwrap().contains("Build a scene of: Pendulum clock"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_large_scene_does_not_stall_the_pipes() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"cat > /dev/null; printf '<!doctype html><html><body>'; head -c 400000 /dev/zero | tr '\0' 'a'; printf '</body></html>'"#);
    let writer = SceneWriter { backend: Box::new(backend) };
    let html = writer.generate("t", &"word ".repeat(20_000), "s", &Cancel::default()).unwrap();
    assert_eq!(html.len(), 400_000 + "<!doctype html><html><body></body></html>".len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn claude_code_not_logged_in_says_how_to_log_in() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"cat > /dev/null; echo 'Invalid API key · Please run /login' >&2; exit 1"#);
    let failure = backend.complete(&prompt(), &Cancel::default()).unwrap_err();
    assert_eq!(failure, ClaudeError::NotLoggedIn);
    assert!(failure.to_string().contains("/login"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn any_other_failure_gives_the_helpers_line() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"cat > /dev/null; echo 'starting' >&2; echo 'Claude stopped early (error_max_turns).' >&2; exit 1"#);
    assert_eq!(backend.complete(&prompt(), &Cancel::default()), Err(ClaudeError::Helper("Claude stopped early (error_max_turns).".into())));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_node_or_helper_say_what_to_do() {
    let dir = temp_dir("scene-helper");
    let no_node = ClaudeSubscription::new(None, Some(dir.join("scene-agent.mjs")));
    let a = no_node.complete(&prompt(), &Cancel::default()).unwrap_err();
    assert_eq!(a, ClaudeError::NoNode);
    assert!(a.to_string().contains("Node.js"));
    let no_helper = ClaudeSubscription::new(Some(PathBuf::from("/bin/sh")), Some(dir.join("missing.mjs")));
    assert_eq!(no_helper.complete(&prompt(), &Cancel::default()), Err(ClaudeError::NoHelper));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_helper_without_its_packages_counts_as_missing() {
    let stderr = "node:internal/modules/esm/resolve:839
  throw new ERR_MODULE_NOT_FOUND(packageName, fileURLToPath(base), null);
Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@anthropic-ai/claude-agent-sdk' imported from /x/scene-agent.mjs
    at packageResolve (node:internal/modules/esm/resolve:839:9)
Node.js v22.11.0";
    assert_eq!(ClaudeSubscription::failure(stderr), ClaudeError::NoHelper);
    assert_eq!(ClaudeSubscription::failure(""), ClaudeError::Helper("the scene helper stopped without saying why.".into()));
}

#[test]
fn cancelling_stops_the_helper_process() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"echo $$ > "$HERE/pid"; exec sleep 30"#);
    let cancel = Cancel::default();
    let started = Instant::now();
    let c = cancel.clone();
    let task = std::thread::spawn(move || backend.complete(&prompt(), &c));
    let pid_file = dir.join("pid");
    while !pid_file.exists() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let failure = task.join().unwrap().unwrap_err();
    assert_eq!(failure, ClaudeError::Cancelled);
    assert_eq!(failure.to_string(), "The scene build was cancelled.");
    assert!(started.elapsed() < Duration::from_secs(5), "it did not wait for the helper to finish");
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the helper is gone");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancelling_before_the_helper_starts_never_starts_it() {
    let dir = temp_dir("scene-helper");
    let backend = helper(&dir, r#"touch "$HERE/ran""#);
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(backend.complete(&prompt(), &cancel), Err(ClaudeError::Cancelled));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!dir.join("ran").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_api_backend_still_needs_a_key() {
    assert_eq!(ClaudeApi { api_key: None }.complete(&prompt(), &Cancel::default()), Err(ClaudeError::NoKey));
}

#[test]
fn a_cancelled_api_request_is_never_sent() {
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(ClaudeApi { api_key: Some("sk-test".into()) }.complete(&prompt(), &cancel), Err(ClaudeError::Cancelled));
}

// MARK: Finding node and the helper

#[test]
fn node_is_looked_for_where_linux_keeps_it() {
    let versions: Vec<String> = ["v9.11.2", "v22.11.0", "v20.3.1"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        node::candidates("/home/o", &versions),
        vec![
            "/home/o/.volta/bin/node", "/usr/local/bin/node", "/usr/bin/node",
            "/home/o/.nvm/versions/node/v22.11.0/bin/node", "/home/o/.nvm/versions/node/v20.3.1/bin/node",
            "/home/o/.nvm/versions/node/v9.11.2/bin/node",
        ]
    );
}

#[test]
fn the_first_node_found_wins_and_the_login_shell_is_the_fallback() {
    let asked = std::cell::Cell::new(false);
    let shell = || {
        asked.set(true);
        Some("Welcome!\n/home/o/.asdf/shims/node\n".to_string())
    };
    assert_eq!(node::find("/home/o", &[], &|p| p == "/usr/local/bin/node", &shell).as_deref(), Some("/usr/local/bin/node"));
    assert!(!asked.get(), "the shell is only asked when nothing else is there");
    assert_eq!(node::find("/home/o", &[], &|p| p.starts_with("/home/o/.asdf"), &shell).as_deref(), Some("/home/o/.asdf/shims/node"));
    assert_eq!(node::find("/home/o", &[], &|_| false, &shell), None);
    assert_eq!(node::find("/home/o", &[], &|p| !p.starts_with('/'), &|| Some("node not found".into())), None, "an answer that is no path is not taken");
}

#[test]
fn the_helper_is_installed_with_kairos_unless_pointed_elsewhere() {
    let resources = Path::new("/usr/share/kairos");
    std::env::remove_var("KAIROS_SCENE_AGENT");
    assert_eq!(node::helper_url(Some(resources)), Some(PathBuf::from("/usr/share/kairos/scene-agent/scene-agent.mjs")));
    std::env::set_var("KAIROS_SCENE_AGENT", "/src/scene-agent/scene-agent.mjs");
    assert_eq!(node::helper_url(Some(resources)), Some(PathBuf::from("/src/scene-agent/scene-agent.mjs")));
    std::env::remove_var("KAIROS_SCENE_AGENT");
    let dir = temp_dir("scene-helper");
    assert!(!node::helper_installed(Some(&dir.join("scene-agent.mjs"))));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_helper_counts_as_installed_with_its_sdk() {
    let dir = temp_dir("scene-helper");
    let helper = dir.join("scene-agent.mjs");
    std::fs::write(&helper, "").unwrap();
    assert!(!node::helper_installed(Some(&helper)), "npm ci has not run");
    std::fs::create_dir_all(dir.join("node_modules/@anthropic-ai/claude-agent-sdk")).unwrap();
    assert!(node::helper_installed(Some(&helper)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn settings_says_what_is_missing() {
    let none = node::SceneSetup::default();
    assert!(!none.status(SceneBackend::Subscription, true).0);
    assert!(none.status(SceneBackend::Subscription, true).1.contains("Node.js"));
    let mut no_helper = node::SceneSetup { node: Some(PathBuf::from("/usr/bin/node")), ..Default::default() };
    assert!(no_helper.status(SceneBackend::Subscription, false).1.contains("install.sh"));
    no_helper.helper_installed = true;
    let ready = no_helper.status(SceneBackend::Subscription, false);
    assert!(ready.0);
    assert!(ready.1.contains("/usr/bin/node"));
    assert!(!none.status(SceneBackend::ApiKey, false).0);
    assert!(none.status(SceneBackend::ApiKey, true).0);
}

#[test]
fn the_checklist_asks_for_node_and_the_helper_not_a_key() {
    let mut facts = SetupFacts::default();
    let item = |f: &SetupFacts| SetupChecklist::new(f).items.into_iter().find(|i| i.step == SetupStep::Scenes).unwrap();
    assert!(!item(&facts).done);
    assert!(item(&facts).detail.contains("Node.js"));
    assert!(!item(&facts).title.contains("Anthropic"), "no API key is demanded");
    facts.has_node = true;
    assert!(item(&facts).detail.contains("scene helper"));
    facts.has_scene_helper = true;
    assert!(item(&facts).done);
    assert_eq!(item(&facts).title, "Claude subscription ready");
    facts.scene_backend = SceneBackend::ApiKey;
    assert!(!item(&facts).done);
    assert_eq!(item(&facts).title, "Add an Anthropic key (optional)");
    facts.has_claude_key = true;
    assert!(item(&facts).done);
}

// MARK: Voice

#[test]
fn voice_requests() {
    let (url, headers, _) = voice::request(VoiceProvider::Elevenlabs, "k", "", "Hi").unwrap();
    assert_eq!(url, "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM?output_format=mp3_44100_128");
    assert!(headers.contains(&("xi-api-key".to_string(), "k".to_string())));
    let (_, _, body) = voice::request(VoiceProvider::Openai, "k", "nova", "Hi").unwrap();
    assert_eq!(body["voice"], "nova");
    assert_eq!(body["input"], "Hi");
    assert!(voice::request(VoiceProvider::System, "k", "", "Hi").is_none());
    let voices = voice::parse_elevenlabs_voices(br#"{"voices":[{"voice_id":"a1","name":"Rachel"},{"name":"broken"}]}"#);
    assert_eq!(voices, vec![CloudVoice { id: "a1".into(), name: "Rachel".into() }]);
}

fn chosen_voice(provider: VoiceProvider, id: &str) -> String {
    let (url, _, body) = voice::request(provider, "k", id, "Hi.").unwrap();
    if provider == VoiceProvider::Elevenlabs {
        url.split('?').next().unwrap().rsplit('/').next().unwrap().to_string()
    } else {
        body["voice"].as_str().unwrap().to_string()
    }
}

#[test]
fn a_voice_saved_for_the_other_provider_falls_back_to_the_default() {
    assert_eq!(chosen_voice(VoiceProvider::Openai, "21m00Tcm4TlvDq8ikWAM"), "alloy");
    assert_eq!(chosen_voice(VoiceProvider::Openai, "nova"), "nova");
    assert_eq!(chosen_voice(VoiceProvider::Elevenlabs, "nova"), voice::ELEVENLABS_DEFAULT_VOICE);
    assert_eq!(chosen_voice(VoiceProvider::Elevenlabs, "pNInz6obpgDQGcFmaJgB"), "pNInz6obpgDQGcFmaJgB");
}

// MARK: Privacy

fn msg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}
fn list() -> Vec<String> {
    vec!["mail.google.com".into(), "mybank.com".into()]
}

#[test]
fn voice_and_scenes_from_a_listed_page_are_refused() {
    for kind in ["tts.speak", "tts.prefetch", "scene.candidates", "scene.generate", "scene.edit"] {
        let m = msg(json!({"type": kind, "id": "1", "text": "Your statement is ready.", "url": "https://www.mybank.com/statements"}));
        let refusal = privacy::refusal(&m, &list());
        assert!(refusal.is_some(), "{kind}");
        assert!(refusal.unwrap().contains("mybank.com"), "the message names the site");
    }
    let mail = msg(json!({"type": "tts.speak", "text": "Hi", "url": "https://mail.google.com/mail/u/0/#inbox"}));
    assert!(privacy::refusal(&mail, &list()).is_some());
}

#[test]
fn other_pages_and_other_requests_go_through() {
    assert_eq!(privacy::refusal(&msg(json!({"type": "tts.speak", "text": "Hi", "url": "https://en.wikipedia.org/wiki/Kairos"})), &list()), None);
    assert_eq!(
        privacy::refusal(&msg(json!({"type": "tts.speak", "text": "Hi", "url": "https://notmybank.com/"})), &list()),
        None,
        "a host only matches the entry or its subdomains"
    );
    assert_eq!(privacy::refusal(&msg(json!({"type": "read.log", "url": "https://mybank.com/"})), &list()), None, "nothing leaves the machine");
    assert_eq!(privacy::refusal(&msg(json!({"type": "tts.speak", "text": "Hi"})), &list()), None, "no url to check");
}

#[test]
fn jev_is_not_told_a_browsers_page_title() {
    let chrome = privacy::app_subject("Google Chrome", "google-chrome.desktop", "Inbox (3) - me@gmail.com - Gmail");
    assert_eq!(chrome.app, "Google Chrome");
    assert_eq!(chrome.title, "");
    assert_eq!(privacy::app_subject("Firefox", "firefox.desktop", "My Bank").title, "");
    assert_eq!(privacy::app_subject("Text Editor", "xed.desktop", "Draft").title, "Draft");
}

#[test]
fn extract_html_keeps_byte_positions_with_text_that_grows_when_lower_cased() {
    // `İ` is two bytes but lower-cases to three, which used to shift the slice and panic.
    let raw = "İİİİ<!DOCTYPE html><html><body>x</body></HTML> İ";
    assert_eq!(scenes::extract_html(raw).unwrap(), "<!DOCTYPE html><html><body>x</body></HTML>");
    assert_eq!(scenes::extract_html("ẞİ <HTML>İ</html>").unwrap(), "<HTML>İ</html>");
    assert!(scenes::extract_html("İİİİ").is_err());
}
