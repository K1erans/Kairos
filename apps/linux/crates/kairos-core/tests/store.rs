mod common;
use common::*;
use kairos_core::keys::{KeyStore, ScratchKeys};
use kairos_core::store::{self, DataStore};
use kairos_core::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn starter() -> AppData {
    AppData::starter(None, &|_| true)
}
fn temp_store(tag: &str) -> (PathBuf, DataStore) {
    let dir = temp_dir(tag);
    let s = DataStore::new(dir.join("kairos.json"));
    (dir, s)
}

// MARK: Store

#[test]
fn kairos_home_points_the_store_at_a_scratch_folder() {
    let home = scratch_home();
    assert_eq!(store::default_directory(), home);
    assert_eq!(DataStore::standard().path, home.join("kairos.json"));
    assert!(store::is_scratch());
    assert_eq!(KeyStore::current(), KeyStore::File(home.join("keys.json")));
}

#[test]
fn store_round_trip_and_corrupt_backup() {
    let (dir, store) = temp_store("store");
    let mut data = starter();
    data.tasks.push(TaskItem::new("t", Some(data.groups[0].id)));
    store.save(&data).unwrap();
    assert_eq!(store.load(starter).tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["t"]);
    assert_eq!(store::encode(&store.load(starter)), store::encode(&data), "everything survives a save and load (dates at whole seconds)");
    std::fs::write(&store.path, b"not json").unwrap();
    assert_eq!(store.load(starter).groups.len(), 3);
    let files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    assert!(files.iter().any(|f| f.starts_with("kairos.corrupt-")), "{files:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saved_json_has_sorted_keys_and_upper_case_ids() {
    let data = starter();
    let text = String::from_utf8(store::encode(&data)).unwrap();
    let id = data.groups[0].id.string();
    assert_eq!(id, id.to_uppercase());
    assert!(text.contains(&id));
    let v: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn older_files_without_new_fields_still_load() {
    let old = r#"{"groups":[{"name":"Work","blockSites":["x.com"]}],"tasks":[],"sessions":[],"blockEvents":[],"settings":{"iconSize":40},"focus":{"mode":"off"},"onboarded":true}"#;
    let data: AppData = serde_json::from_str(old).unwrap();
    assert_eq!(data.groups[0].block_sites, vec!["x.com"]);
    assert_eq!(data.groups[0].desktop, 0);
    assert_eq!(data.settings.icon_size, 40.0);
    assert_eq!(data.settings.voice_provider, VoiceProvider::System);
    assert_eq!(data.settings.presets.len(), 3);
}

#[test]
fn settings_added_later_default_on_for_existing_profiles() {
    let s = Settings::default();
    assert!(s.hides_in_full_screen && s.keeps_windows_clear);
    assert_eq!(s.bar_appearance, BarAppearance::Dark);
    assert_eq!(s.scene_backend, SceneBackend::Subscription);
    let saved: Settings = serde_json::from_str(r#"{"autoHideBar": false, "dockEdge": "bottom", "iconSize": 24}"#).unwrap();
    assert!(saved.hides_in_full_screen);
    assert!(saved.keeps_windows_clear, "a profile saved before the setting existed gets it on");
    assert_eq!(saved.bar_appearance, BarAppearance::Dark);
    assert_eq!(saved.scene_backend, SceneBackend::Subscription);
    assert_eq!(saved.dock_edge, DockEdge::Bottom);
    let glass: Settings = serde_json::from_str(r#"{"barAppearance": "glass"}"#).unwrap();
    assert_eq!(glass.bar_appearance, BarAppearance::Glass);
    assert_eq!(SceneBackend::ALL.map(|b| b.label()), ["Claude subscription", "Anthropic API key"]);
}

#[test]
fn an_existing_profile_keeps_its_group_keys() {
    let s: Settings = serde_json::from_str(r#"{"groupHotkeys": true}"#).unwrap();
    assert!(s.group_hotkeys);
    assert_eq!(s.quick_add_hotkey, Settings::default().quick_add_hotkey);
}

#[test]
fn seen_extensions_and_hiding_the_list_survive_a_relaunch() {
    let mut data = AppData::default();
    data.seen_clients = vec!["shield".into()];
    data.setup_hidden = true;
    let back: AppData = serde_json::from_slice(&store::encode(&data)).unwrap();
    assert_eq!(back.seen_clients, vec!["shield"]);
    assert!(back.setup_hidden);
}

// MARK: Settings behaviour

#[test]
fn changing_provider_forgets_the_old_providers_voice() {
    let mut s = Settings::default();
    s.set_voice_provider(VoiceProvider::Elevenlabs);
    s.voice_id = "21m00Tcm4TlvDq8ikWAM".into();
    s.set_voice_provider(VoiceProvider::Openai);
    assert_eq!(s.voice_id, "");
    s.voice_id = "nova".into();
    s.set_voice_provider(VoiceProvider::Openai);
    assert_eq!(s.voice_id, "nova", "picking the same provider keeps the voice");
}

#[test]
fn the_panel_is_hidden_only_while_the_bar_is_on_screen() {
    let mut s = Settings::default();
    assert!(!s.hides_system_dock());
    s.replaces_system_dock = true;
    assert!(s.hides_system_dock());
    s.dock_visible = false;
    assert!(!s.hides_system_dock(), "hiding Kairos Bar brings the panel back");
}

#[test]
fn using_kairos_bar_instead_of_the_panel_turns_on_auto_hide() {
    let mut s = Settings::default();
    assert!(!s.auto_hide_bar);
    s.set_replaces_system_dock(true);
    assert!(s.replaces_system_dock);
    assert!(s.auto_hide_bar);
    s.auto_hide_bar = false;
    s.set_replaces_system_dock(true);
    assert!(!s.auto_hide_bar, "only switching it on sets auto-hide; the owner's later choice stands");
    s.set_replaces_system_dock(false);
    assert!(!s.replaces_system_dock);
}

// MARK: LenientTests

fn tt() -> Date {
    t(1_800_000_000.0)
}

fn object<T: Serialize>(v: &T) -> Map<String, Value> {
    serde_json::to_value(v).unwrap().as_object().unwrap().clone()
}

struct Sample {
    name: &'static str,
    keys: &'static [&'static str],
    required: &'static [&'static str],
    full: Map<String, Value>,
    fallback: fn() -> Map<String, Value>,
    check: fn(&Value) -> Result<Map<String, Value>, String>,
}

fn decode<T: Serialize + DeserializeOwned>(v: &Value) -> Result<Map<String, Value>, String> {
    serde_json::from_value::<T>(v.clone()).map(|d| object(&d)).map_err(|e| e.to_string())
}

fn samples() -> Vec<Sample> {
    let mut group = AppGroup::with("Work", "book.fill", "#123456");
    group.dock_apps = vec!["a".into()];
    group.blocked_apps = vec!["b".into()];
    group.allow_sites = vec!["c.com".into()];
    group.block_sites = vec!["d.com".into()];
    group.hidden_elements = BTreeMap::from([("e.com".into(), vec!["#x".into()])]);
    group.jev_enabled = false;
    group.jev_threshold = 0.4;
    group.tempo_preset = "Sprint".into();
    group.desktop = 2;
    let mut task = TaskItem::new("Write", Some(Id::new()));
    task.notes = "n".into();
    task.priority = 2;
    task.due = Some(tt());
    task.estimate_min = Some(30);
    task.status = TaskStatus::Done;
    task.parent_id = Some(Id::new());
    task.created_at = tt();
    task.completed_at = Some(tt());
    task.listened_sec = 9;
    let mut session = Session::new(Some(Id::new()), Some(Id::new()), tt(), plus(tt(), 600.0), SessionKind::Rest);
    session.active_seconds = Some(540.0);
    session.interruptions = 2;
    session.completed = false;
    let event = BlockEvent::new(Some(Id::new()), "discord.desktop", Some("Discord".into()), "app", true, tt());
    let mut preset = TempoPreset::new("P", 40, 7, 20, true);
    preset.cycles_before_long = 3;
    let mut s = Settings::default();
    s.dock_edge = DockEdge::Bottom;
    s.dock_visible = false;
    s.icon_size = 50.0;
    s.switch_behavior = SwitchBehavior::Leave;
    s.presets = vec![preset.clone()];
    s.never_analyse = vec!["x.com".into()];
    s.jev_fail_closed = true;
    s.max_overrides_per_session = 1;
    s.override_minutes = 9;
    s.break_overlay = false;
    s.group_hotkeys = true;
    s.quick_add_hotkey = "super-space".into();
    s.timer_hotkey = "off".into();
    s.voice_provider = VoiceProvider::Openai;
    s.voice_id = "nova".into();
    s.auto_hide_bar = true;
    s.replaces_system_dock = true;
    s.notifies_when_due = false;
    s.shows_dock_folders = false;
    s.starts_focus_after_break = true;
    s.block_unshielded_browsers = false;
    s.keeps_windows_clear = false;
    s.hides_in_full_screen = false;
    s.bar_appearance = BarAppearance::Glass;
    s.scene_backend = SceneBackend::ApiKey;
    let mut focus = FocusState::default();
    focus.active_group_id = Some(Id::new());
    focus.current_task_id = Some(Id::new());
    focus.mode = Mode::Strict;
    focus.ends_at = Some(tt());
    focus.strict_until = Some(tt());
    focus.task_by_group = BTreeMap::from([(Id::new().string(), Id::new())]);
    let mut data = AppData::default();
    data.groups = vec![group.clone()];
    data.tasks = vec![task.clone()];
    data.sessions = vec![session.clone()];
    data.block_events = vec![event.clone()];
    data.settings = s.clone();
    data.focus = focus.clone();
    data.onboarded = true;
    data.seen_clients = vec!["read".into()];
    data.setup_hidden = true;
    data.open_session = Some(session.clone());
    data.unassigned_listened_sec = 5;

    vec![
        Sample { name: "AppGroup", keys: AppGroup::LENIENT_KEYS, required: &["name"], full: object(&group),
                 fallback: || object(&AppGroup::new("")), check: decode::<AppGroup> },
        Sample { name: "TaskItem", keys: TaskItem::LENIENT_KEYS, required: &["title"], full: object(&task),
                 fallback: || object(&TaskItem::new("", None)), check: decode::<TaskItem> },
        Sample { name: "Session", keys: Session::LENIENT_KEYS, required: &["start", "end"], full: object(&session),
                 fallback: || object(&Session::new(None, None, time::distant_past(), time::distant_past(), SessionKind::Focus)),
                 check: decode::<Session> },
        Sample { name: "BlockEvent", keys: BlockEvent::LENIENT_KEYS, required: &["target", "time"], full: object(&event),
                 fallback: || object(&BlockEvent::new(None, "", None, "rule", false, time::distant_past())), check: decode::<BlockEvent> },
        Sample { name: "TempoPreset", keys: TempoPreset::LENIENT_KEYS, required: &["name"], full: object(&preset),
                 fallback: || object(&TempoPreset::new("", 25, 5, 15, false)), check: decode::<TempoPreset> },
        Sample { name: "Settings", keys: Settings::LENIENT_KEYS, required: &[], full: object(&s),
                 fallback: || object(&Settings::default()), check: decode::<Settings> },
        Sample { name: "FocusState", keys: FocusState::LENIENT_KEYS, required: &[], full: object(&focus),
                 fallback: || object(&FocusState::default()), check: decode::<FocusState> },
        Sample { name: "AppData", keys: AppData::LENIENT_KEYS, required: &[], full: object(&data),
                 fallback: || object(&AppData::default()), check: decode::<AppData> },
    ]
}

#[test]
fn field_tables_name_every_stored_property() {
    for s in samples() {
        let mut listed: Vec<&str> = s.keys.to_vec();
        listed.sort();
        let mut stored: Vec<&str> = s.full.keys().map(|k| k.as_str()).collect();
        stored.sort();
        assert_eq!(listed, stored, "{}: the lenient table must list every stored field", s.name);
    }
}

#[test]
fn each_type_decodes_with_each_key_removed() {
    for s in samples() {
        let (fb1, fb2) = ((s.fallback)(), (s.fallback)());
        for key in s.keys {
            let mut cut = s.full.clone();
            cut.remove(*key);
            if s.required.contains(key) {
                assert!((s.check)(&Value::Object(cut.clone())).is_err(), "{} without {key}", s.name);
                let list = json!({"items": [s.full, cut, s.full]});
                let n = list["items"].as_array().unwrap().iter().filter(|v| (s.check)(v).is_ok()).count();
                assert_eq!(n, 2, "{}: an element without {key} drops alone", s.name);
                continue;
            }
            let decoded = (s.check)(&Value::Object(cut)).unwrap_or_else(|e| panic!("{} without {key}: {e}", s.name));
            for (k, v) in &s.full {
                if k != key {
                    assert_eq!(decoded.get(k), Some(v), "{} without {key} kept {k}", s.name);
                }
            }
            // Fresh ids and dates differ on every fallback; the rest must match the fallback.
            if fb1.get(*key) == fb2.get(*key) {
                assert_eq!(decoded.get(*key), fb1.get(*key), "{}.{key} falls back", s.name);
            }
        }
        // A wrong type falls back too, rather than failing the value.
        for key in s.keys.iter().filter(|k| !s.required.contains(k)) {
            let mut bad = s.full.clone();
            bad.insert(key.to_string(), json!({"unexpected": true}));
            assert!((s.check)(&Value::Object(bad)).is_ok(), "{}.{key} of the wrong type", s.name);
        }
    }
}

#[test]
fn a_list_keeps_every_element_that_decodes() {
    let map = json!({"items": [{"name": "A"}, {"focusMin": 1}, {"name": "B", "strict": true}]});
    let mut out: Vec<TempoPreset> = vec![];
    kairos_core::lenient::list(map.as_object().unwrap(), "items", &mut out).unwrap();
    assert_eq!(out.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
}

#[test]
fn one_bad_field_no_longer_discards_the_file() {
    let json = r#"{"groups":[{"name":"Work"}],"tasks":[{"id":"8C0C2D2E-5B0B-4C55-9A4A-2B8E0C1F6A11","title":"t","priority":0,"status":"todo","createdAt":"2026-11-09T10:00:00Z","listenedSec":0}],"sessions":[],"blockEvents":[],"settings":{"presets":[{"name":"A","focusMin":20,"shortBreakMin":5,"longBreakMin":15,"cyclesBeforeLong":4},{"name":"B","focusMin":30,"shortBreakMin":5,"longBreakMin":15,"cyclesBeforeLong":4,"strict":true},{"focusMin":1}]},"focus":{"mode":"off"},"onboarded":true}"#;
    let (dir, store) = temp_store("lenient");
    std::fs::write(&store.path, json).unwrap();
    let d = store.load(starter);
    assert_eq!(d.tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["t"], "a task without notes loads");
    assert_eq!(d.tasks[0].id.string(), "8C0C2D2E-5B0B-4C55-9A4A-2B8E0C1F6A11");
    assert_eq!(d.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Work"], "the file is not swapped for starter data");
    assert_eq!(d.settings.presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
    assert_eq!(d.settings.presets.iter().map(|p| p.strict).collect::<Vec<_>>(), vec![false, true]);
    let _ = std::fs::remove_dir_all(dir);
}

// MARK: ImportTests

const NOW: f64 = 2_000_000_000.0;

#[test]
fn something_that_is_not_an_export_is_refused() {
    assert!(AppData::from_export(b"{}").is_err());
    assert!(AppData::from_export(b"not json").is_err());
    assert!(AppData::from_export(&store::encode(&starter())).is_ok());
}

#[test]
fn import_takes_the_files_content_but_keeps_what_is_live_here() {
    let mut current = starter();
    current.focus.mode = Mode::Focus;
    current.seen_clients = vec!["shield".into()];
    current.onboarded = true;
    let mut file = starter();
    file.groups = vec![AppGroup::with("Imported", "pencil", "#6B7280")];
    file.tasks = vec![TaskItem::new("From the file", None)];
    file.focus.mode = Mode::Off;
    file.focus.active_group_id = Some(Id::new());
    file.focus.current_task_id = Some(Id::new());
    file.onboarded = false;
    let imported_id = file.groups[0].id;
    let next = current.importing(file, t(NOW));
    assert_eq!(next.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Imported"]);
    assert_eq!(next.tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["From the file"]);
    assert_eq!(next.focus.mode, Mode::Focus, "the mode is not the file's to change");
    assert_eq!(next.focus.active_group_id, Some(imported_id));
    assert_eq!(next.focus.current_task_id, None);
    assert_eq!(next.seen_clients, vec!["shield"]);
    assert!(next.onboarded);
}

#[test]
fn the_current_file_is_backed_up_beside_it_first() {
    let (dir, store) = temp_store("import");
    assert_eq!(store.backup(t(NOW)).unwrap(), None, "nothing saved yet, nothing to back up");
    let mut data = starter();
    data.tasks = vec![TaskItem::new("Keep me", None)];
    store.save(&data).unwrap();
    let first = store.backup(t(NOW)).unwrap().unwrap();
    let second = store.backup(t(NOW)).unwrap().unwrap();
    assert_ne!(first, second, "a second backup in the same second does not overwrite the first");
    assert_eq!(first.parent().unwrap(), dir.as_path());
    assert!(first.file_name().unwrap().to_string_lossy().starts_with("kairos-backup-"));
    let restored = AppData::from_export(&std::fs::read(&first).unwrap()).unwrap();
    assert_eq!(restored.tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["Keep me"]);
    let _ = std::fs::remove_dir_all(dir);
}

// MARK: KeyStoreTests

#[test]
fn the_real_profile_uses_the_keyring() {
    assert_eq!(KeyStore::at(None), KeyStore::Keyring);
    assert_eq!(KeyStore::at(Some("")), KeyStore::Keyring);
}

#[test]
fn a_scratch_copy_keeps_keys_in_its_scratch_folder() {
    assert_eq!(KeyStore::at(Some("/tmp/kairos-check-1")), KeyStore::File(PathBuf::from("/tmp/kairos-check-1/keys.json")));
}

#[test]
fn scratch_keys_round_trip() {
    let dir = temp_dir("keys");
    let keys = ScratchKeys { path: dir.join("keys.json") };
    assert_eq!(keys.get("anthropic"), None, "no file yet");
    assert!(keys.set("sk-test", "anthropic"));
    assert!(keys.set("ts-test", "typesafe"));
    assert_eq!(keys.get("anthropic").as_deref(), Some("sk-test"));
    assert!(keys.set("", "anthropic"));
    assert_eq!(keys.get("anthropic"), None, "an empty value removes it");
    assert_eq!(keys.get("typesafe").as_deref(), Some("ts-test"));
    let _ = std::fs::remove_dir_all(dir);
}
