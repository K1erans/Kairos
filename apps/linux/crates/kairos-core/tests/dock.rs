mod common;
use kairos_core::clearance::Rect;
use kairos_core::dock::*;
use kairos_core::{AppData, DockEdge, Id, SwitchBehavior};
use std::collections::HashSet;
use std::path::PathBuf;

fn starter() -> AppData {
    AppData::starter(None, &|_| true)
}
fn app(s: &str) -> Slot {
    Slot::App(s.into())
}
fn running(s: &str) -> Slot {
    Slot::Running(s.into())
}
fn geo(slots: Vec<Slot>, icon: f64) -> DockGeometry {
    DockGeometry::new(slots, icon, MAGNIFICATION)
}
fn stride(from: f64, to: f64, by: f64) -> Vec<f64> {
    let mut out = vec![];
    let mut x = from;
    while x <= to + 1e-9 {
        out.push(x);
        x += by;
    }
    out
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

// MARK: Slots

#[test]
fn open_apps_not_pinned_follow_a_separator() {
    let mut data = starter();
    data.settings.switch_behavior = SwitchBehavior::Leave;
    data.settings.shows_dock_folders = false;
    data.groups[0].dock_apps = vec!["a".into(), "b".into()];
    data.focus.active_group_id = Some(data.groups[0].id);
    let r = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let none = HashSet::new();
    let slots = DockGeometry::slots_for(&data, &r(&["b", "x", "a", "y", "x"]), &none);
    assert_eq!(slots[slots.len() - 5..].to_vec(), vec![app("a"), app("b"), Slot::Separator, running("x"), running("y")]);
    assert_eq!(DockGeometry::slots_for(&data, &r(&["a"]), &none).last(), Some(&app("b")), "no separator when every open app is pinned");
    assert!(!Slot::Separator.magnifies());
    data.groups[0].dock_apps.clear();
    let slots = DockGeometry::slots_for(&data, &r(&["x"]), &none);
    assert_eq!(slots[slots.len() - 3..].to_vec(), vec![Slot::AddApp, Slot::Separator, running("x")]);
}

#[test]
fn slots_offer_new_group_and_add_app_for_an_empty_group() {
    let mut data = starter();
    data.settings.shows_dock_folders = false;
    let none = HashSet::new();
    let slots = DockGeometry::slots_for(&data, &[], &none);
    assert_eq!(slots[..2].to_vec(), vec![Slot::Kairos, Slot::Separator], "Kairos opens from the bar's leading end");
    assert_eq!(slots.iter().filter(|s| **s == Slot::NewGroup).count(), 1);
    assert!(!slots.contains(&Slot::AddApp));
    data.groups[0].dock_apps.clear();
    let slots = DockGeometry::slots_for(&data, &[], &none);
    assert_eq!(slots[slots.len() - 2..].to_vec(), vec![Slot::Divider, Slot::AddApp]);
}

#[test]
fn downloads_and_trash_come_last_after_a_separator() {
    let mut data = starter();
    data.settings.switch_behavior = SwitchBehavior::Leave;
    data.groups[0].dock_apps = vec!["a".into()];
    data.focus.active_group_id = Some(data.groups[0].id);
    let none = HashSet::new();
    let slots = DockGeometry::slots_for(&data, &["x".into()], &none);
    assert_eq!(
        slots[slots.len() - 6..].to_vec(),
        vec![app("a"), Slot::Separator, running("x"), Slot::Separator, Slot::Folder(DockFolder::Downloads), Slot::Folder(DockFolder::Trash)]
    );
    data.settings.shows_dock_folders = false;
    assert!(!DockGeometry::slots_for(&data, &[], &none).contains(&Slot::Folder(DockFolder::Trash)));
}

#[test]
fn unpinned_open_apps_show_even_with_hide_on_switch_unless_hidden_by_switch() {
    let mut data = starter();
    data.groups[0].dock_apps = vec!["work".into(), "shared".into()];
    data.groups[1].dock_apps = vec!["study".into(), "shared".into()];
    data.settings.shows_dock_folders = false;
    let open: Vec<String> = ["work", "study", "shared", "other"].iter().map(|s| s.to_string()).collect();
    let n = data.groups.len() + 4;
    let none = HashSet::new();
    let work_open = DockGeometry::slots_for(&data, &open, &none);
    assert_eq!(work_open[n..].to_vec(), vec![app("work"), app("shared"), Slot::Separator, running("study"), running("other")]);

    let hidden_study: HashSet<String> = ["study".into()].into_iter().collect();
    let work_hidden = DockGeometry::slots_for(&data, &open, &hidden_study);
    assert_eq!(work_hidden[n..].to_vec(), vec![app("work"), app("shared"), Slot::Separator, running("other")]);

    data.focus = data.focus_after_switch(data.groups[1].id);
    let hidden_work: HashSet<String> = ["work".into(), "other".into()].into_iter().collect();
    let study_hidden = DockGeometry::slots_for(&data, &open, &hidden_work);
    assert_eq!(study_hidden[n..].to_vec(), vec![app("study"), app("shared")]);
}

#[test]
fn a_group_can_leave_other_apps_alone_when_settings_hide_them() {
    let mut data = starter();
    data.settings.switch_behavior = SwitchBehavior::Hide;
    data.settings.shows_dock_folders = false;
    data.groups[0].dock_apps = vec!["notes".into()];
    data.groups[1].dock_apps = vec!["mail".into()];
    data.groups[1].switch_behavior = Some(SwitchBehavior::Leave);
    let open: Vec<String> = vec!["game".into()];
    let hidden_game: HashSet<String> = ["game".into()].into_iter().collect();
    let none = HashSet::new();
    data.focus.active_group_id = Some(data.groups[0].id);
    assert_eq!(data.active_switch_behavior(), SwitchBehavior::Hide);
    assert!(!DockGeometry::slots_for(&data, &open, &hidden_game).contains(&running("game")));
    data.focus = data.focus_after_switch(data.groups[1].id);
    assert_eq!(data.active_switch_behavior(), SwitchBehavior::Leave);
    assert!(DockGeometry::slots_for(&data, &open, &none).contains(&running("game")));
}

#[test]
fn every_item_on_the_bar_has_its_own_key_even_with_two_separators() {
    let g = geo(vec![app("a"), Slot::Separator, running("x"), Slot::Separator, Slot::Folder(DockFolder::Trash)], 36.0);
    let layout = g.layout(None, 1.0);
    let keys: HashSet<(Slot, usize)> = keyed(&layout.items).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys.len(), 5);
}

#[test]
fn the_slot_under_the_pointer_is_the_one_that_grows_there() {
    let g = geo(vec![app("a"), Slot::Separator, Slot::Folder(DockFolder::Downloads), Slot::Folder(DockFolder::Trash)], 36.0);
    let rest = g.layout(None, 1.0).items;
    assert_eq!(g.slot_at(rest[3].center), Some(Slot::Folder(DockFolder::Trash)));
    assert_eq!(g.slot_at(rest[0].center), Some(app("a")));
    assert_eq!(g.slot_at(rest[1].center), None, "a separator takes no drops");
}

// MARK: DockGeometryTests

fn dock() -> DockGeometry {
    let mut slots: Vec<Slot> = (0..3).map(|_| Slot::Group(Id::new())).collect();
    slots.push(Slot::NewGroup);
    slots.push(Slot::Divider);
    slots.extend(["a", "b", "c", "d"].iter().map(|s| app(s)));
    geo(slots, 36.0)
}

#[test]
fn falloff_is_full_under_the_pointer_and_rest_at_range() {
    let d = dock();
    assert!(close(d.scale(0.0), d.max_scale));
    assert!(close(d.scale(d.range()), 1.0));
    assert!(close(d.scale(d.range() * 3.0), 1.0));
    let samples: Vec<f64> = stride(0.0, d.range(), 2.0).iter().map(|x| d.scale(*x)).collect();
    assert!(samples.windows(2).all(|w| w[0] >= w[1]), "shrinks steadily with distance");
    assert!(close(d.scale(-10.0), d.scale(10.0)));
}

#[test]
fn at_rest_the_bar_fits_its_slots_exactly() {
    let d = dock();
    let l = d.layout(None, 1.0);
    assert_eq!(l.start, 0.0);
    assert!(close(l.end, d.rest_length()));
    assert_eq!(l.hovered, None);
    assert!(l.items.iter().all(|i| i.scale == 1.0));
}

#[test]
fn magnified_icons_never_overlap_and_stay_inside_the_bar() {
    let d = dock();
    for p in stride(-20.0, d.rest_length() + 20.0, 0.5) {
        let l = d.layout(Some(p), 1.0);
        for w in l.items.windows(2) {
            assert!(w[0].center + w[0].size / 2.0 + d.gap() <= w[1].center - w[1].size / 2.0 + 1e-6, "overlap at pointer {p}");
        }
        assert!(l.start + d.padding() <= l.items[0].center - l.items[0].size / 2.0 + 1e-6);
        let last = l.items.last().unwrap();
        assert!(l.end - d.padding() >= last.center + last.size / 2.0 - 1e-6);
    }
}

#[test]
fn the_icon_under_the_pointer_stays_under_it_and_neighbours_grow_less() {
    let d = dock();
    let rest = d.layout(None, 1.0);
    let target = 5;
    let p = rest.items[target].center + 5.0;
    let l = d.layout(Some(p), 1.0);
    assert_eq!(l.hovered, Some(target));
    assert!((l.items[target].center - p).abs() < l.items[target].size / 2.0);
    assert!(l.items[target].scale > l.items[target + 1].scale);
    assert!(l.items[target + 1].scale > 1.0);
    assert_eq!(l.items[0].scale, 1.0, "far icons stay at rest size");
    assert_eq!(l.items[4].scale, 1.0, "the divider never grows");
}

fn spread(v: &[f64]) -> f64 {
    v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
}

#[test]
fn the_bars_ends_hold_still_while_the_pointer_sweeps_across_a_long_run_of_apps() {
    let mut slots = vec![Slot::NewGroup, Slot::Divider];
    slots.extend((0..14).map(|i| app(&i.to_string())));
    let d = geo(slots, 36.0);
    let rest = d.layout(None, 1.0).items;
    let from = rest[2].center + d.range() + d.icon;
    let to = rest[15].center - d.range() - d.icon;
    let layouts: Vec<Layout> = stride(from, to, 0.5).iter().map(|p| d.layout(Some(*p), 1.0)).collect();
    assert!(spread(&layouts.iter().map(|l| l.start).collect::<Vec<_>>()) < 0.01, "the leading end rocks");
    assert!(spread(&layouts.iter().map(|l| l.end).collect::<Vec<_>>()) < 0.01, "the trailing end rocks");
}

#[test]
fn the_bars_ends_hold_still_while_the_pointer_crosses_the_divider_and_a_separator() {
    let mut slots: Vec<Slot> = (0..5).map(|_| Slot::Group(Id::new())).collect();
    slots.push(Slot::Divider);
    slots.extend((0..5).map(|i| app(&i.to_string())));
    slots.push(Slot::Separator);
    slots.extend((0..5).map(|i| running(&i.to_string())));
    let d = geo(slots, 26.0);
    let rest = d.layout(None, 1.0).items;
    for thin in rest.iter().filter(|i| !i.slot.magnifies()) {
        let layouts: Vec<Layout> = stride(thin.center - d.icon, thin.center + d.icon, 0.5).iter().map(|p| d.layout(Some(*p), 1.0)).collect();
        assert!(spread(&layouts.iter().map(|l| l.start).collect::<Vec<_>>()) < 0.01, "the bar shrinks crossing {:?}", thin.slot);
        assert!(spread(&layouts.iter().map(|l| l.end).collect::<Vec<_>>()) < 0.01, "the bar shrinks crossing {:?}", thin.slot);
        assert!(
            layouts.iter().all(|l| l.items.iter().find(|i| i.slot == thin.slot).unwrap().size == DIVIDER_LENGTH),
            "{:?} keeps its size",
            thin.slot
        );
    }
}

#[test]
fn the_icon_centred_under_the_pointer_grows_to_the_full_magnification() {
    let d = dock();
    let target = d.layout(None, 1.0).items[6].clone();
    assert!(close(d.layout(Some(target.center), 1.0).items[6].size, d.icon * d.max_scale));
}

#[test]
fn easing_in_and_out_stays_inside_the_panel_and_starts_at_rest() {
    let d = dock();
    let (lead, trail) = d.overflow();
    let rest = d.layout(None, 1.0);
    let none = d.layout(Some(d.rest_length() / 2.0), 0.0);
    assert!(close(none.start, rest.start));
    assert!(close(none.end, rest.end));
    for (a, b) in none.items.iter().zip(&rest.items) {
        assert!(close(a.center, b.center));
    }
    for amount in stride(0.0, 1.0, 0.1) {
        for p in stride(-d.range(), d.rest_length() + d.range(), 1.0) {
            let l = d.layout(Some(p), amount);
            assert!(l.start >= -lead - 1e-6);
            assert!(l.end <= d.rest_length() + trail + 1e-6);
            for w in l.items.windows(2) {
                assert!(w[0].center + w[0].size / 2.0 + d.gap() <= w[1].center - w[1].size / 2.0 + 1e-6);
            }
        }
    }
}

#[test]
fn overflow_covers_every_pointer_position() {
    let d = dock();
    let (lead, trail) = d.overflow();
    assert!(lead + trail > 0.0);
    for p in stride(-d.range(), d.rest_length() + d.range(), 0.25) {
        let l = d.layout(Some(p), 1.0);
        assert!(l.start >= -lead - 1e-6);
        assert!(l.end <= d.rest_length() + trail + 1e-6);
    }
}

#[test]
fn hit_zone_engages_on_the_bar_and_holds_over_magnified_icons() {
    let d = dock();
    let inset = 4.0;
    let mid = d.rest_length() / 2.0;
    assert!(d.engages(mid, 0.0, inset, false), "pushing against the screen edge engages");
    assert!(d.engages(mid, inset + d.thickness() - 1.0, inset, false));
    let over_icon = inset + d.thickness() + d.headroom() / 2.0;
    assert!(!d.engages(mid, over_icon, inset, false), "a magnified icon's reach does not engage from outside");
    assert!(d.engages(mid, over_icon, inset, true), "but holds once engaged");
    assert!(!d.engages(mid, inset + d.thickness() + d.headroom() + 2.0, inset, true));
    assert!(!d.engages(-5.0, 10.0, inset, false));
}

#[test]
fn the_edge_strip_spans_the_bar() {
    let g = geo(vec![Slot::NewGroup, Slot::Divider, Slot::AddApp], 36.0);
    assert!(g.touches_edge(g.rest_length() / 2.0, 0.0));
    assert!(g.touches_edge(0.0, EDGE_STRIP));
    assert!(!g.touches_edge(g.rest_length() / 2.0, 6.0), "a pointer near the edge but not on it");
    assert!(!g.touches_edge(g.rest_length() + 30.0, 0.0), "the edge beyond the bar's ends");
}

// MARK: ReduceMotionTests

#[test]
fn with_reduce_motion_icons_keep_their_size_under_the_pointer() {
    let motion = DockMotion { reduced: true };
    let g = DockGeometry::new(vec![app("a"), app("b"), app("c"), app("d")], 36.0, motion.max_scale());
    let under = g.layout(Some(g.rest_length() / 2.0), 1.0).items;
    assert_eq!(under.iter().map(|i| i.size).collect::<Vec<_>>(), vec![36.0; 4]);
    assert_eq!(g.headroom(), 0.0);
}

#[test]
fn without_it_the_icon_under_the_pointer_grows() {
    let g = DockGeometry::new(vec![app("a"), app("b"), app("c"), app("d")], 36.0, DockMotion { reduced: false }.max_scale());
    let first = g.layout(None, 1.0).items[0].clone();
    assert!(g.layout(Some(first.center), 1.0).items[0].size > 36.0);
}

#[test]
fn auto_hide_fades_in_place_with_reduce_motion_and_slides_without() {
    assert_eq!(DockMotion { reduced: true }.appearance(0.25), (0.0, 0.25));
    assert_eq!(DockMotion { reduced: false }.appearance(0.25), (0.75, 1.0));
}

// MARK: BarAppearanceTests (proportions)

#[test]
fn the_owners_bar_is_about_one_point_six_times_its_icons() {
    let g = geo(vec![Slot::NewGroup], 24.0);
    assert_eq!(g.padding(), 7.0);
    assert_eq!(g.thickness(), 38.0);
    assert!((g.thickness() / g.icon - 1.6).abs() <= 0.05);
    assert_eq!(g.corner_radius(), 13.0);
}

#[test]
fn a_small_bar_matches_the_dock_screenshot() {
    let g = geo(vec![Slot::NewGroup], 16.0);
    assert_eq!(g.thickness(), 30.0);
    assert_eq!(g.corner_radius(), 10.0);
}

#[test]
fn larger_icons_keep_the_proportions() {
    for icon in stride(20.0, 64.0, 2.0) {
        let g = geo(vec![Slot::NewGroup], icon);
        assert!(g.thickness() / icon >= 1.38, "{icon}");
        assert!(g.thickness() / icon <= 1.7, "{icon}");
        assert_eq!(g.corner_radius(), (g.thickness() / 3.0).round());
    }
}

// MARK: DockDropTests

fn doc() -> PathBuf {
    PathBuf::from("/tmp/report.pdf")
}
fn pic() -> PathBuf {
    PathBuf::from("/tmp/photo.jpg")
}
fn notes() -> PathBuf {
    PathBuf::from("/usr/share/applications/xed.desktop")
}
fn id(p: &PathBuf) -> Option<String> {
    if p.extension().map(|e| e == "desktop").unwrap_or(false) {
        p.file_name().map(|n| n.to_string_lossy().to_string())
    } else {
        None
    }
}

#[test]
fn files_on_trash_go_to_the_trash() {
    assert_eq!(DockDrop::plan(&[doc(), pic()], Some(&Slot::Folder(DockFolder::Trash)), &id), DockDrop::MoveToTrash(vec![doc(), pic()]));
}

#[test]
fn documents_on_an_app_open_in_it() {
    assert_eq!(DockDrop::plan(&[doc()], Some(&app("xreader.desktop")), &id), DockDrop::Open(vec![doc()], "xreader.desktop".into()));
    assert_eq!(DockDrop::plan(&[doc(), notes()], Some(&running("xreader.desktop")), &id), DockDrop::Open(vec![doc()], "xreader.desktop".into()));
}

#[test]
fn an_app_dropped_anywhere_else_joins_the_group() {
    let want = DockDrop::AddApps(vec!["xed.desktop".into()]);
    assert_eq!(DockDrop::plan(&[notes()], Some(&Slot::Group(Id::new())), &id), want);
    assert_eq!(DockDrop::plan(&[notes()], None, &id), want);
    assert_eq!(DockDrop::plan(&[notes()], Some(&app("xreader.desktop")), &id), want);
}

#[test]
fn documents_anywhere_else_do_nothing() {
    assert_eq!(DockDrop::plan(&[doc()], Some(&Slot::Folder(DockFolder::Downloads)), &id), DockDrop::Nothing);
    assert_eq!(DockDrop::plan(&[doc()], None, &id), DockDrop::Nothing);
    assert_eq!(DockDrop::plan(&[], Some(&Slot::Folder(DockFolder::Trash)), &id), DockDrop::Nothing);
}

// MARK: DockLaunchTests

#[test]
fn an_app_pulses_until_it_is_running() {
    let mut l = DockLaunches::default();
    l.start("a", 0.0);
    l.update(&HashSet::new(), 2.0);
    assert_eq!(l.ids(), HashSet::from(["a".to_string()]));
    l.update(&HashSet::from(["a".to_string()]), 3.0);
    assert!(l.ids().is_empty());
}

#[test]
fn an_app_that_never_starts_stops_pulsing() {
    let mut l = DockLaunches::default();
    l.start("a", 0.0);
    l.update(&HashSet::new(), DockLaunches::TIMEOUT);
    assert!(l.ids().is_empty());
}

// MARK: MoveTests

#[test]
fn moving_stops_at_the_ends() {
    let apps = vec!["a", "b", "c"];
    assert_eq!(moving_by(&apps, &"b", -1), vec!["b", "a", "c"]);
    assert_eq!(moving_by(&apps, &"b", 1), vec!["a", "c", "b"]);
    assert_eq!(moving_by(&apps, &"a", -1), apps);
    assert_eq!(moving_by(&apps, &"c", 5), apps);
    assert_eq!(moving_by(&apps, &"z", 1), apps);
}

#[test]
fn dragged_icon_goes_to_nearest_peer_and_stays_there() {
    let centers = [10.0, 50.0, 90.0, 130.0];
    assert_eq!(reorder::index(&centers, -40.0), Some(0));
    assert_eq!(reorder::index(&centers, 69.0), Some(1));
    assert_eq!(reorder::index(&centers, 71.0), Some(2));
    assert_eq!(reorder::index(&centers, 400.0), Some(3));
    assert_eq!(reorder::index(&[], 0.0), None);
    assert_eq!(moving_to(&["a", "b", "c", "d"], &"a", 2), vec!["b", "c", "a", "d"]);
    assert_eq!(moving_to(&["a", "b", "c", "d"], &"d", 0), vec!["d", "a", "b", "c"]);
    assert_eq!(moving_to(&["a", "b"], &"b", 1), vec!["a", "b"]);
}

#[test]
fn dragged_app_changes_section_at_the_line_between_them() {
    use reorder::Section;
    let pinned = [100.0, 140.0, 180.0];
    let open = [240.0, 280.0];
    assert_eq!(reorder::section(60.0, &pinned, &open, 36.0), Section::Pinned(0));
    assert_eq!(reorder::section(150.0, &pinned, &open, 36.0), Section::Pinned(2));
    assert_eq!(reorder::section(205.0, &pinned, &open, 36.0), Section::Pinned(3));
    assert_eq!(reorder::section(215.0, &pinned, &open, 36.0), Section::Open);
    assert_eq!(reorder::section(200.0, &pinned, &[], 36.0), Section::Pinned(3));
    assert_eq!(reorder::section(210.0, &pinned, &[], 36.0), Section::Open);
}

// MARK: DockLabelTests

const WORK: f64 = 83.0;
const TRASH: f64 = 59.0;
const VS_CODE: f64 = 204.0;
const LONG_GROUP: f64 = 241.0;
const LONG_GROUP_BLOCKING: f64 = 325.0;

fn pill(c: f64, w: f64) -> (f64, f64) {
    (c - w / 2.0, c + w / 2.0)
}
fn assert_inside(p: (f64, f64), lo: f64, hi: f64) {
    assert!(p.0 >= lo + label::MARGIN - 1e-9, "clipped at the start: {p:?} in {lo}..{hi}");
    assert!(p.1 <= hi - label::MARGIN + 1e-9, "clipped at the end: {p:?} in {lo}..{hi}");
}

#[test]
fn the_first_slots_label_slides_inside_the_panel() {
    for w in [WORK, VS_CODE, LONG_GROUP, LONG_GROUP_BLOCKING] {
        let c = label::center(25.0, w, 0.0, 594.0);
        assert_inside(pill(c, w), 0.0, 594.0);
        let p = pill(c, w);
        assert!(p.0 <= 25.0 && 25.0 <= p.1, "still over the item");
    }
    assert_eq!(label::center(25.0, WORK, 0.0, 594.0), 4.0 + WORK / 2.0);
}

#[test]
fn the_last_slots_label_slides_inside_the_panel() {
    for w in [TRASH, VS_CODE, LONG_GROUP_BLOCKING] {
        let c = label::center(575.0, w, 0.0, 594.0);
        assert_inside(pill(c, w), 0.0, 594.0);
        let p = pill(c, w);
        assert!(p.0 <= 575.0 && 575.0 <= p.1);
    }
}

#[test]
fn a_middle_label_is_centred_over_its_item() {
    assert_eq!(label::center(300.0, WORK, 0.0, 594.0), 300.0);
    assert_eq!(label::center(300.0, LONG_GROUP, 0.0, 594.0), 300.0);
}

#[test]
fn a_label_stays_on_the_visible_screen() {
    let c = label::center(80.0, VS_CODE, 60.0, 594.0);
    assert_inside(pill(c, VS_CODE), 60.0, 594.0);
}

#[test]
fn a_short_bar_grows_its_panel_for_the_widest_label() {
    let grow = label::growth(150.0, LONG_GROUP);
    assert_eq!(grow, 50.0);
    let hi = 150.0 + 2.0 * grow;
    let c = label::center(grow + 20.0, LONG_GROUP, 0.0, hi);
    assert_inside(pill(c, LONG_GROUP), 0.0, hi);
    assert_eq!(label::growth(594.0, LONG_GROUP_BLOCKING), 0.0, "a long bar needs no more room");
}

#[test]
fn the_owners_bar_hovering_its_first_group() {
    let g = geo(
        vec![
            Slot::Group(Id::new()), Slot::Group(Id::new()), Slot::NewGroup, Slot::Divider, app("a"), app("b"), app("c"), app("d"),
            Slot::Separator, Slot::Folder(DockFolder::Downloads), Slot::Folder(DockFolder::Trash),
        ],
        24.0,
    );
    let (lead0, trail0) = g.overflow();
    let grow = label::growth(lead0 + g.rest_length() + trail0, LONG_GROUP_BLOCKING);
    let lead = lead0 + grow;
    let length = lead0 + g.rest_length() + trail0 + 2.0 * grow;
    let first = g.layout(None, 1.0).items[0].center;
    let hovered = g.layout(Some(first), 1.0);
    let item = &hovered.items[hovered.hovered.unwrap()];
    assert!(lead + item.center - item.size / 2.0 >= 0.0, "the magnified first icon is inside the panel");
    assert!(lead + hovered.start >= 0.0, "and so is the bar under it");
    for w in [WORK, LONG_GROUP_BLOCKING] {
        let c = label::center(lead + item.center, w, 0.0, length);
        assert_inside(pill(c, w), 0.0, length);
    }
    let last = g.layout(None, 1.0).items.last().unwrap().center;
    let at_end = g.layout(Some(last), 1.0);
    let trash = &at_end.items[at_end.hovered.unwrap()];
    assert!(lead + trash.center + trash.size / 2.0 <= length, "the magnified Trash is inside the panel");

    // DockFrame does the same sums for the bottom edge.
    let frame = DockFrame::new(g.slots.clone(), 24.0, DockEdge::Bottom, false, LONG_GROUP_BLOCKING);
    assert_eq!(frame.lead, lead);
    assert_eq!(frame.axis_length(), length);
}

#[test]
fn side_labels_at_either_end_stay_inside_the_panel() {
    for item in [12.0, 320.0, 632.0] {
        let c = label::center(item, 26.0, 0.0, 640.0);
        assert_inside(pill(c, 26.0), 0.0, 640.0);
    }
    assert_eq!(label::center(320.0, 26.0, 0.0, 640.0), 320.0);
}

#[test]
fn the_side_lane_fits_the_widest_label() {
    assert_eq!(label::side_lane(LONG_GROUP_BLOCKING), 339.0);
    assert!(label::side_lane(VS_CODE) >= VS_CODE + label::GAP);
    assert_eq!(label::side_lane(WORK), 97.0, "short names keep a narrow lane");
}

#[test]
fn a_label_wider_than_its_room_is_centred_on_it() {
    assert_eq!(label::center(10.0, 300.0, 0.0, 200.0), 100.0);
}

// MARK: DockPlacementTests (X11 coordinates: top-left origin, y down)

fn monitor() -> Rect {
    Rect::new(0.0, 0.0, 1470.0, 956.0)
}
fn work_area() -> Rect {
    Rect::new(0.0, 34.0, 1470.0, 922.0)
}

#[test]
fn the_bar_at_rest_is_centred_on_its_edge() {
    let (x, y) = panel_origin(DockEdge::Bottom, 594.0, 94.0, 280.0, monitor(), work_area());
    assert_eq!(x + 280.0, work_area().mid_x(), "the bar's middle, not the panel's, is centred");
    assert_eq!(y + 94.0, work_area().max_y());
    let (x, y) = panel_origin(DockEdge::Left, 94.0, 594.0, 280.0, monitor(), work_area());
    assert_eq!(x, 0.0);
    assert_eq!(y + 280.0, work_area().y + work_area().h / 2.0);
    let (x, _) = panel_origin(DockEdge::Right, 94.0, 594.0, 280.0, monitor(), work_area());
    assert_eq!(x + 94.0, 1470.0);
}

#[test]
fn a_wide_bar_stays_on_screen() {
    let (x, _) = panel_origin(DockEdge::Bottom, 1500.0, 94.0, 100.0, monitor(), work_area());
    assert_eq!(x + 1500.0, monitor().max_x(), "as on the Mac, a bar wider than the screen keeps its trailing end on screen");
    let (x, _) = panel_origin(DockEdge::Bottom, 594.0, 94.0, 1000.0, monitor(), work_area());
    assert_eq!(x, 0.0, "pushed back from the leading edge");
    let (_, y) = panel_origin(DockEdge::Left, 94.0, 1200.0, 100.0, monitor(), work_area());
    assert_eq!(y + 1200.0, monitor().max_y());
}

#[test]
fn a_panel_reserving_a_strip_is_respected() {
    // A Cinnamon panel at the bottom keeps its strip; the bar sits above it on the same edge.
    let with_panel = Rect::new(0.0, 0.0, 1470.0, 916.0);
    let (_, y) = panel_origin(DockEdge::Bottom, 594.0, 94.0, 280.0, monitor(), with_panel);
    assert_eq!(y + 94.0, 916.0);
}

#[test]
fn frame_maps_between_panel_and_bar_coordinates() {
    for edge in DockEdge::ALL {
        let f = DockFrame::new(vec![Slot::NewGroup, Slot::Divider, app("a")], 36.0, edge, false, 100.0);
        let (x, y) = f.point(20.0, 10.0);
        assert_eq!(f.bar(x, y), (20.0, 10.0), "{edge:?}");
    }
}
