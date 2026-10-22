//! A split tree of tab groups in an editing layout like FilmCraft's: Source | Program |
//! Properties over Project | Tools | Timeline | Audio Meters, every group an ordinary
//! resizable one. One test per docking rule, the stuck cases of the FilmCraft map, and a
//! property test over random drops.

use std::collections::HashMap;

use super::floating_tests::frame_style;
use super::render::{Drawn, Sizer};
use super::testing::DockDriver;
use super::*;
use crate::layout::{SplitAxis, SplitSize};
use egui::{Modifiers, Pos2, Rect, Vec2, pos2, vec2};

const SOURCE: [u32; 4] = [1, 11, 12, 13];
const PROGRAM: u32 = 2;
const PROPERTIES: [u32; 3] = [3, 31, 32];
const PROJECT: [u32; 7] = [4, 41, 42, 43, 44, 45, 46];
const TOOLS: u32 = 5;
const TIMELINE: u32 = 6;
const METERS: u32 = 7;
const INFO: u32 = 9;

const SCREEN: Vec2 = Vec2::new(1440.0, 860.0);

/// Panel bodies by their rects; `caps` are maximum widths a host asks for.
#[derive(Default)]
struct Film {
    bodies: HashMap<u32, Rect>,
    caps: HashMap<u32, f32>,
}

impl DockContent<u32> for Film {
    fn body(&mut self, ui: &mut egui::Ui, panel: &u32) {
        self.bodies.insert(*panel, ui.max_rect());
    }
    fn logical_label(&self, panel: &u32) -> String {
        format!("Panel {panel}")
    }
    fn limits(&self, panel: &u32) -> PanelLimits {
        match self.caps.get(panel) {
            Some(width) => PanelLimits { min: Vec2::ZERO, max: vec2(*width, 1_000_000.0) },
            None => PanelLimits::default(),
        }
    }
}

type Driver = DockDriver<u32, Film>;

fn tabs(panels: &[u32]) -> Node<u32> {
    Node::Tabs { panels: panels.to_vec(), active: 0 }
}

fn split(axis: SplitAxis, ratio: f32, first: Node<u32>, second: Node<u32>) -> Node<u32> {
    Node::Split { axis, size: SplitSize::Ratio(ratio), first: Box::new(first), second: Box::new(second) }
}

fn hsplit(ratio: f32, first: Node<u32>, second: Node<u32>) -> Node<u32> {
    split(SplitAxis::Horizontal, ratio, first, second)
}

/// The Editing workspace, with Info floating over Properties.
fn editing() -> Layout<u32> {
    let top = hsplit(0.69, hsplit(0.5, tabs(&SOURCE), tabs(&[PROGRAM])), tabs(&PROPERTIES));
    let row = hsplit(0.06, tabs(&[TOOLS]), hsplit(0.88, tabs(&[TIMELINE]), tabs(&[METERS])));
    let bottom = hsplit(0.29, tabs(&PROJECT), row);
    Layout {
        root: Some(split(SplitAxis::Vertical, 0.57, top, bottom)),
        floating: vec![Floating::single(vec![INFO], 0, [1120.0, 60.0, 280.0, 200.0])],
    }
}

fn driver(layout: Layout<u32>, content: Film) -> Driver {
    let mut d = DockDriver::new(layout, content).with_style(frame_style);
    d.screen = Rect::from_min_size(Pos2::ZERO, SCREEN);
    d.area = d.screen;
    d.settle();
    d
}

fn film() -> Driver {
    driver(editing(), Film::default())
}

/// A group's rect: its active body and its tab strip.
fn group(d: &Driver, panel: u32) -> Rect {
    let body = d.content.bodies.get(&panel).copied().unwrap_or_else(|| panic!("panel {panel} not drawn"));
    Rect::from_min_max(pos2(body.left(), body.top() - 26.0), body.max)
}

fn tab(d: &Driver, panel: u32) -> Pos2 {
    d.hit(&Hit::Tab(panel)).unwrap_or_else(|| panic!("tab {panel}")).center()
}

/// Drag a divider by `delta`, hovering it first as a pointer does.
fn drag_hit(d: &mut Driver, hit: Hit<u32>, delta: Vec2) {
    let at = d.hit(&hit).unwrap_or_else(|| panic!("{hit:?} in {:?}", d.hits())).center();
    d.move_to(at);
    d.drag(at, at + delta);
    d.settle();
}

fn divider(path: &[bool]) -> Hit<u32> {
    Hit::Divider(Divider::Split(path.to_vec()))
}

fn sizes(d: &Driver, panels: &[u32]) -> Vec<Vec2> {
    panels.iter().map(|p| group(d, *p).size()).collect()
}

fn close(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 1.0 && (a.max - b.max).length() < 1.0
}

#[test]
fn a_group_is_a_picture_frame_whose_edges_split_it_in_half_and_whose_middle_joins_it() {
    let base = film();
    let timeline = group(&base, TIMELINE);
    let others = [PROJECT[0], TOOLS, METERS, SOURCE[0], PROGRAM, PROPERTIES[0]];
    let inset = |zone| {
        let (w, h) = (timeline.width() * 0.08, (timeline.height() - 26.0) * 0.08);
        match zone {
            Zone::Left => pos2(timeline.left() + w, timeline.center().y + 13.0),
            Zone::Right => pos2(timeline.right() - w, timeline.center().y + 13.0),
            Zone::Top => pos2(timeline.center().x, timeline.top() + 26.0 + h),
            Zone::Bottom => pos2(timeline.center().x, timeline.bottom() - h),
            Zone::Center => pos2(timeline.center().x, timeline.center().y + 13.0),
        }
    };
    for zone in [Zone::Left, Zone::Right, Zone::Top, Zone::Bottom] {
        let mut d = film();
        let before = sizes(&d, &others);
        let at = inset(zone);
        d.hold(tab(&d, INFO), at);
        let half = zone_rect(timeline, zone).unwrap();
        assert!(d.filled(half.shrink(1.0)), "{zone:?}: the preview is that half of the group");
        let out = d.release(at);
        assert_eq!(out.actions, vec![Action::Move { panel: INFO, anchor: TIMELINE, placement: Placement::Split(zone) }], "{zone:?}");
        d.settle();
        let (info, rest) = (group(&d, INFO), group(&d, TIMELINE));
        assert!(close(info.union(rest), timeline), "{zone:?}: the two halves fill the group: {info:?} {rest:?} in {timeline:?}");
        assert!((info.area() - rest.area()).abs() < info.area() * 0.05, "{zone:?}: about halves");
        assert_eq!(sizes(&d, &others), before, "{zone:?}: no other group changes");
    }
    // The middle and the tab row join the group.
    let mut d = film();
    let out = d.drag(tab(&d, INFO), inset(Zone::Center));
    assert_eq!(out.actions, vec![Action::Move { panel: INFO, anchor: TIMELINE, placement: Placement::Split(Zone::Center) }]);
    let mut d = film();
    let out = d.drag(tab(&d, INFO), pos2(timeline.center().x, timeline.top() + 10.0));
    assert!(matches!(&out.actions[..], [Action::Move { panel: INFO, placement: Placement::Tab { .. }, .. }]), "{:?}", out.actions);
    // The gap between two groups belongs to neither: the drag cancels.
    let mut d = film();
    let tools = group(&d, TOOLS);
    let gap = pos2(tools.right() + 2.0, tools.center().y);
    let out = d.drag(tab(&d, INFO), gap);
    assert!(out.actions.is_empty(), "{:?}", out.actions);
}

#[test]
fn window_edges_add_a_full_column_or_row_without_taking_the_groups_edges() {
    let mut d = film();
    let project = group(&d, PROJECT[0]);
    let out = d.drag(tab(&d, INFO), pos2(3.0, project.center().y));
    assert_eq!(out.actions, vec![Action::Move { panel: INFO, anchor: SOURCE[0], placement: Placement::Edge(Zone::Left) }]);
    d.settle();
    let info = group(&d, INFO);
    assert_eq!((info.top(), info.bottom(), info.left()), (0.0, SCREEN.y, 0.0), "a full-height column at the left");
    assert!((info.width() - SCREEN.x * 0.25).abs() < 4.0, "{info:?}");

    for (at, zone) in [(pos2(SCREEN.x - 3.0, 300.0), Zone::Right), (pos2(700.0, 3.0), Zone::Top), (pos2(700.0, SCREEN.y - 3.0), Zone::Bottom)] {
        let mut d = film();
        let out = d.drag(tab(&d, INFO), at);
        assert!(matches!(&out.actions[..], [Action::Move { placement: Placement::Edge(z), .. }] if *z == zone), "{zone:?}: {:?}", out.actions);
    }
    // Past the window band, the group's own edge band splits that group.
    let mut d = film();
    let out = d.drag(tab(&d, INFO), pos2(project.left() + 20.0, project.center().y + 13.0));
    assert_eq!(out.actions, vec![Action::Move { panel: INFO, anchor: PROJECT[0], placement: Placement::Split(Zone::Left) }]);
}

#[test]
fn groups_have_one_small_minimum_and_no_maximum() {
    let mut d = film();
    let tools = group(&d, TOOLS).width();
    drag_hit(&mut d, divider(&[true, true]), vec2(400.0, 0.0));
    assert!((group(&d, TOOLS).width() - (tools + 400.0)).abs() < 1.0, "Tools grows as wide as it is dragged");
    drag_hit(&mut d, divider(&[true, true]), vec2(-2000.0, 0.0));
    assert_eq!(group(&d, TOOLS).width(), 40.0, "and shrinks to the minimum, no further");

    // A group too narrow for two halves takes a drop on its edge as a tab.
    let tools = group(&d, TOOLS);
    let out = d.drag(tab(&d, INFO), pos2(tools.right() - 4.0, tools.center().y + 30.0));
    assert!(
        matches!(&out.actions[..], [Action::Move { panel: INFO, anchor: TOOLS, placement: Placement::Split(Zone::Center) | Placement::Tab { .. } }]),
        "{:?}",
        out.actions
    );
}

#[test]
fn a_divider_resizes_only_the_groups_that_share_it() {
    // Timeline | Meters: Project and Tools keep their widths.
    let mut d = film();
    let (project, tools, timeline, meters) = (group(&d, PROJECT[0]), group(&d, TOOLS), group(&d, TIMELINE), group(&d, METERS));
    drag_hit(&mut d, divider(&[true, true, true]), vec2(-100.0, 0.0));
    assert_eq!((group(&d, PROJECT[0]), group(&d, TOOLS)), (project, tools));
    assert!((group(&d, TIMELINE).width() - (timeline.width() - 100.0)).abs() < 1.0);
    assert!((group(&d, METERS).width() - (meters.width() + 100.0)).abs() < 1.0);

    // Project | the rest: only Tools, on the other side, gives way; Timeline and Meters stay.
    let mut d = film();
    drag_hit(&mut d, divider(&[true]), vec2(15.0, 0.0));
    assert!((group(&d, PROJECT[0]).width() - (project.width() + 15.0)).abs() < 1.0);
    assert!((group(&d, TOOLS).width() - (tools.width() - 15.0)).abs() < 1.0);
    assert!(close(group(&d, TIMELINE), timeline) && close(group(&d, METERS), meters), "{:?}", d.layout);

    // Top row: Source | Program leaves Properties alone.
    let mut d = film();
    let properties = group(&d, PROPERTIES[0]);
    drag_hit(&mut d, divider(&[false, false]), vec2(80.0, 0.0));
    assert_eq!(group(&d, PROPERTIES[0]), properties);
}

#[test]
fn an_intersection_moves_both_dividers() {
    let mut d = film();
    let (project, tools, source) = (group(&d, PROJECT[0]), group(&d, TOOLS), group(&d, SOURCE[0]));
    let hit = Hit::Divider(Divider::Intersection(vec![], vec![true]));
    let at = d.hit(&hit).unwrap_or_else(|| panic!("{:?}", d.hits()));
    assert!(at.contains(pos2(project.right() + 2.0, project.top() - 2.0)), "{at:?} at Project's top right corner");
    drag_hit(&mut d, hit, vec2(15.0, -60.0));
    assert!((group(&d, PROJECT[0]).width() - (project.width() + 15.0)).abs() < 1.0);
    assert!((group(&d, PROJECT[0]).height() - (project.height() + 60.0)).abs() < 1.0);
    assert!((group(&d, TOOLS).width() - (tools.width() - 15.0)).abs() < 1.0);
    assert!((group(&d, SOURCE[0]).height() - (source.height() - 60.0)).abs() < 1.0);
    assert_eq!(group(&d, SOURCE[0]).width(), source.width());
}

#[test]
fn closing_a_group_gives_its_space_to_its_neighbours() {
    let mut d = film();
    let (tools, meters) = (group(&d, TOOLS), group(&d, METERS));
    d.layout.apply(Action::CloseGroup { panel: TIMELINE }).unwrap();
    d.settle();
    let meters_now = group(&d, METERS);
    assert_eq!(group(&d, TOOLS), tools);
    assert_eq!((meters_now.right(), meters_now.top(), meters_now.bottom()), (meters.right(), meters.top(), meters.bottom()));
    assert!((meters_now.left() - (tools.right() + 4.0)).abs() < 1.0, "the row has no hole: {meters_now:?}");
}

// The stuck cases of the FilmCraft map (map.md section 3).

#[test]
fn info_dropped_onto_a_capped_group_lifts_the_cap() {
    let content = Film { caps: HashMap::from([(METERS, 116.0)]), ..Film::default() };
    let mut d = driver(editing(), content);
    let meters = group(&d, METERS);
    d.drag(tab(&d, INFO), pos2(meters.center().x, meters.center().y + 13.0));
    d.settle();
    assert!(d.layout.location(&INFO).is_ok_and(|l| l.anchor == Some(METERS)), "{:?}", d.layout);
    // Info is the group's shown panel.
    let width = group(&d, INFO).width();
    drag_hit(&mut d, divider(&[true, true, true]), vec2(-200.0, 0.0));
    assert!((group(&d, INFO).width() - (width + 200.0)).abs() < 1.0, "the group grows past the cap of one of its panels");
    drag_hit(&mut d, divider(&[true, true, true]), vec2(150.0, 0.0));
    assert!((group(&d, INFO).width() - (width + 50.0)).abs() < 1.0, "and shrinks again");
}

#[test]
fn a_drop_beside_a_narrow_group_splits_that_group_only() {
    let mut d = film();
    drag_hit(&mut d, divider(&[true, true]), vec2(80.0, 0.0));
    let (tools, project, timeline, meters) = (group(&d, TOOLS), group(&d, PROJECT[0]), group(&d, TIMELINE), group(&d, METERS));
    assert!(tools.width() > 120.0, "{tools:?}");
    let at = pos2(tools.right() - 8.0, tools.center().y + 13.0);
    d.hold(tab(&d, INFO), at);
    assert!(d.filled(zone_rect(tools, Zone::Right).unwrap().shrink(1.0)), "the preview is Tools' right half");
    let out = d.release(at);
    assert_eq!(out.actions, vec![Action::Move { panel: INFO, anchor: TOOLS, placement: Placement::Split(Zone::Right) }]);
    d.settle();
    assert!(close(group(&d, TOOLS).union(group(&d, INFO)), tools));
    assert_eq!((group(&d, PROJECT[0]), group(&d, TIMELINE), group(&d, METERS)), (project, timeline, meters));
}

#[test]
fn every_divider_travels_after_drops() {
    let mut d = film();
    let tools = group(&d, TOOLS);
    d.drag(tab(&d, INFO), pos2(tools.right() - 8.0, tools.center().y + 13.0));
    d.settle();
    let meters = group(&d, METERS);
    d.drag(tab(&d, PROGRAM), pos2(meters.center().x, meters.center().y + 13.0));
    d.settle();
    let timeline = group(&d, TIMELINE);
    d.drag(tab(&d, PROPERTIES[0]), pos2(timeline.center().x, timeline.bottom() - 10.0));
    d.settle();
    assert_dividers_travel(&mut d);
}

/// Each split divider moves both ways when dragged, from the layout as it is.
fn assert_dividers_travel(d: &mut Driver) {
    let start = d.layout.clone();
    let paths: Vec<Vec<bool>> = d
        .hits()
        .into_iter()
        .filter_map(|(hit, _)| match hit {
            Hit::Divider(Divider::Split(path)) => Some(path),
            _ => None,
        })
        .collect();
    assert!(!paths.is_empty());
    for path in paths {
        for step in [12.0, -24.0] {
            let Some(rect) = d.hit(&divider(&path)) else { continue };
            let delta = if rect.width() < rect.height() { vec2(step, 0.0) } else { vec2(0.0, step) };
            let before = d.layout.clone();
            drag_hit(d, divider(&path), delta);
            assert_ne!(d.layout, before, "divider {path:?} does not move by {step}");
        }
        d.layout = start.clone();
        d.settle();
    }
}

/// Every group at or above its minimum, and every divider with room both ways.
fn assert_sizes(d: &Driver, context: &str) {
    let mut style = DockStyle::default();
    frame_style(&mut style);
    let Some(root) = &d.layout.root else { return };
    let sizer = Sizer { style: &style, content: &d.content };
    let mut groups: Vec<Drawn<'_, u32>> = Vec::new();
    sizer.groups(root, d.area, &mut groups);
    for g in &groups {
        let room = g.rect.size() - g.limits.min;
        assert!(room.x >= -0.01 && room.y >= -0.01, "{context}: {:?} is below its minimum {:?}", g.rect, g.limits.min);
    }
    let mut nodes = vec![(root, d.area)];
    while let Some((node, rect)) = nodes.pop() {
        let Some(children) = sizer.children(node, rect) else { continue };
        let (lo, hi) = sizer.range(node, rect);
        assert!(lo < -0.5 && hi > 0.5, "{context}: a divider is stuck ({lo}, {hi}) at {rect:?} in {:?}", d.layout);
        nodes.extend(children);
    }
}

/// A small deterministic generator, so a failing sequence repeats.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 1000) as f32 / 1000.0
    }
}

#[test]
fn random_drops_leave_every_group_its_minimum_and_every_divider_travel() {
    for seed in 1..=6u64 {
        let mut rng = Lcg(seed);
        let mut d = film();
        for step in 0..14 {
            // Only the bodies drawn now.
            d.content.bodies.clear();
            d.settle();
            let panels: Vec<u32> = d.layout.panels().into_iter().copied().collect();
            let panel = panels[rng.below(panels.len())];
            let Some(from) = d.hit(&Hit::Tab(panel)).map(|r| r.center()) else { continue };
            let target = panels[rng.below(panels.len())];
            let Some(rect) = d.content.bodies.get(&target).copied() else { continue };
            let to = match rng.below(8) {
                // A window edge.
                0 => {
                    let edges = [
                        pos2(3.0, rect.center().y),
                        pos2(SCREEN.x - 3.0, rect.center().y),
                        pos2(rect.center().x, 3.0),
                        pos2(rect.center().x, SCREEN.y - 3.0),
                    ];
                    edges[rng.below(4)]
                }
                // The tab row.
                1 => pos2(rect.center().x, rect.top() - 13.0),
                // Anywhere in the body: an edge band or the middle.
                _ => rect.lerp_inside(vec2(0.02 + rng.unit() * 0.96, 0.02 + rng.unit() * 0.96)),
            };
            d.drag(from, to);
            d.settle();
            if d.layout.floating.iter().any(|w| w.contains(&panel)) && panel != INFO {
                // Back into the tree: this test is about the docked layout.
                d.layout.apply(Action::Move { panel, anchor: target, placement: Placement::Split(Zone::Center) }).ok();
                d.settle();
            }
            assert_sizes(&d, &format!("seed {seed}, step {step}"));
        }
        assert_dividers_travel(&mut d);
    }
}

#[test]
fn ctrl_or_cmd_floats_from_a_tab_or_a_drag_bar_and_the_preview_says_so() {
    const HINT: &str = "Hold Ctrl to undock";
    let floated = |actions: &[Action<u32>], panel: u32| match actions {
        [Action::Float { panel: p, .. }] => *p == panel,
        [Action::MoveGroup { panel: p, to: Target::Float { .. } }] => *p == panel,
        _ => false,
    };
    fn texts(d: &Driver) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        d.shapes().iter().for_each(|clipped| walk(&clipped.shape, &mut out));
        out
    }
    let d = film();
    let over = group(&d, TIMELINE).center() + vec2(0.0, 13.0);
    let from_tab = tab(&d, PROJECT[0]);
    let from_bar = d.hit(&Hit::DragBar(PROJECT[0])).unwrap().center();
    for from in [from_tab, from_bar] {
        // Without the modifier, a release over a group docks there; the preview names the modifier.
        let mut d = film();
        d.hold(from, over);
        assert!(texts(&d).iter().any(|t| t == HINT), "{from:?}: {:?}", texts(&d));
        let out = d.release(over);
        assert!(!out.actions.is_empty() && !floated(&out.actions, PROJECT[0]), "{from:?}: {:?}", out.actions);
        // Held, it floats, and the hint is gone.
        let mut d = film();
        d.modifiers = Modifiers::COMMAND;
        d.hold(from, over);
        assert!(!texts(&d).iter().any(|t| t == HINT), "{from:?}");
        let out = d.release(over);
        assert!(floated(&out.actions, PROJECT[0]), "{from:?}: {:?}", out.actions);
    }
}

/// The Editing workspace under a 24 pt menu bar and above a 20 pt status bar.
fn film_in_window() -> Driver {
    let mut d = DockDriver::new(editing(), Film::default()).with_style(frame_style);
    d.screen = Rect::from_min_size(Pos2::ZERO, SCREEN);
    d.area = Rect::from_min_max(pos2(0.0, 24.0), pos2(SCREEN.x, SCREEN.y - 20.0));
    d.settle();
    d
}

#[test]
fn a_release_over_no_target_inside_the_window_cancels_the_drag() {
    let d = film_in_window();
    let (source, program) = (group(&d, SOURCE[0]), group(&d, PROGRAM));
    let gap = pos2((source.right() + program.left()) * 0.5, source.center().y);
    let from_tab = tab(&d, PROJECT[0]);
    let from_bar = d.hit(&Hit::DragBar(PROJECT[0])).unwrap().center();
    for from in [from_tab, from_bar] {
        for to in [pos2(600.0, 10.0), pos2(600.0, SCREEN.y - 8.0), gap] {
            let mut d = film_in_window();
            let before = d.layout.clone();
            let out = d.drag(from, to);
            d.settle();
            assert!(out.actions.is_empty(), "{from:?} to {to:?}: {:?}", out.actions);
            assert_eq!(d.layout, before, "{from:?} to {to:?}");
        }
        // Ctrl/Cmd held floats it there.
        let mut d = film_in_window();
        d.modifiers = Modifiers::COMMAND;
        d.drag(from, pos2(600.0, 10.0));
        d.settle();
        assert!(d.layout.floating.iter().any(|w| w.contains(&PROJECT[0])), "{from:?}: {:?}", d.layout.floating);
    }
}

#[test]
fn a_release_outside_the_window_floats() {
    let d = film_in_window();
    let from_tab = tab(&d, PROJECT[0]);
    let from_bar = d.hit(&Hit::DragBar(PROJECT[0])).unwrap().center();
    for from in [from_tab, from_bar] {
        let mut d = film_in_window();
        d.hold(from, pos2(600.0, 10.0));
        d.leave();
        d.settle();
        let window = d.layout.floating.iter().find(|w| w.contains(&PROJECT[0]));
        assert!(window.is_some(), "{from:?}: {:?}", d.layout);
        // A tab takes one panel; a drag bar takes the group.
        assert_eq!(window.unwrap().contains(&PROJECT[1]), from == from_bar, "{from:?}");
    }
}

#[test]
fn the_title_bar_moves_a_floating_window_without_docking() {
    let mut d = film();
    let title = d.hit(&Hit::Title(INFO)).unwrap().center();
    let timeline = group(&d, TIMELINE);
    d.drag(title, timeline.center());
    d.settle();
    let window = d.layout.floating.iter().find(|w| w.contains(&INFO)).expect("still floating");
    assert!(window.rect[0] < 1000.0 && window.rect[1] > 400.0, "moved towards the drop: {:?}", window.rect);
    assert_eq!(d.layout.root, editing().root);
}

fn panel_set(layout: &Layout<u32>) -> Vec<u32> {
    let mut panels: Vec<u32> = layout.panels().into_iter().copied().collect();
    panels.sort_unstable();
    panels
}

fn window_of(layout: &Layout<u32>, panel: u32) -> &Floating<u32> {
    layout.floating.iter().find(|w| w.contains(&panel)).unwrap_or_else(|| panic!("{panel} does not float: {layout:?}"))
}

#[test]
fn tabs_and_drag_bars_dock_into_a_floating_window_and_out_of_it() {
    let mut d = film();
    let all = panel_set(&d.layout);
    let info = group(&d, INFO);
    let out = d.drag(tab(&d, PROGRAM), pos2(info.left() + 10.0, info.center().y + 13.0));
    assert_eq!(out.actions, vec![Action::Move { panel: PROGRAM, anchor: INFO, placement: Placement::Split(Zone::Left) }]);
    d.settle();
    assert!(matches!(window_of(&d.layout, INFO).root, Node::Split { axis: SplitAxis::Horizontal, .. }), "{:?}", d.layout.floating);
    let (program, info) = (group(&d, PROGRAM), group(&d, INFO));
    assert!(program.right() < info.left() && (program.width() - info.width()).abs() < 2.0, "{program:?} {info:?}");

    // A drag bar inside the window moves its group to a group of the main area.
    let timeline = group(&d, TIMELINE);
    let bar = d.hit(&Hit::DragBar(INFO)).unwrap().center();
    let out = d.drag(bar, timeline.center() + vec2(0.0, 13.0));
    assert_eq!(out.actions, vec![Action::MoveGroup { panel: INFO, to: Target::At { anchor: TIMELINE, placement: Placement::Split(Zone::Center) } }]);
    d.settle();
    assert_eq!(window_of(&d.layout, PROGRAM).root, tabs(&[PROGRAM]));
    assert_eq!(panel_set(&d.layout), all);
}

#[test]
fn a_floating_window_tree_resizes_by_its_dividers_and_intersections() {
    let mut layout = editing();
    let tree = hsplit(0.5, tabs(&[INFO]), split(SplitAxis::Vertical, 0.5, tabs(&[50]), tabs(&[51])));
    layout.floating = vec![Floating::new(tree, [700.0, 100.0, 600.0, 500.0])];
    let mut d = driver(layout, Film::default());
    let ratios = |d: &Driver| match &d.layout.floating[0].root {
        Node::Split { size: SplitSize::Ratio(outer), second, .. } => match &**second {
            Node::Split { size: SplitSize::Ratio(inner), .. } => (*outer, *inner),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    let outer = Hit::Divider(Divider::WindowSplit(INFO, Vec::new()));
    let inner = Hit::Divider(Divider::WindowSplit(INFO, vec![true]));
    for (hit, delta) in [(&outer, vec2(30.0, 0.0)), (&outer, vec2(-60.0, 0.0)), (&inner, vec2(0.0, 30.0)), (&inner, vec2(0.0, -60.0))] {
        let before = ratios(&d);
        drag_hit(&mut d, hit.clone(), delta);
        let after = ratios(&d);
        let moved = if hit == &outer { (after.0 - before.0) * delta.x } else { (after.1 - before.1) * delta.y };
        assert!(moved > 0.0, "{hit:?} by {delta:?}: {before:?} to {after:?}");
        assert_eq!(d.layout.root, editing().root, "the main area keeps its sizes");
    }
    // The four-way handle where the inner divider meets the outer one moves both.
    let before = ratios(&d);
    drag_hit(&mut d, Hit::Divider(Divider::WindowIntersection(INFO, Vec::new(), vec![true])), vec2(25.0, 25.0));
    let after = ratios(&d);
    assert!(after.0 > before.0 && after.1 > before.1, "{before:?} to {after:?}");
}

#[test]
fn moves_between_windows_are_one_action_and_lose_no_panel() {
    let mut layout = editing();
    layout.floating.push(Floating::single(vec![50, 51], 0, [200.0, 520.0, 320.0, 260.0]));
    let mut d = driver(layout, Film::default());
    let all = panel_set(&d.layout);
    // Floating to floating: a whole group, by its drag bar, beside Info.
    let info = group(&d, INFO);
    let bar = d.hit(&Hit::DragBar(50)).unwrap().center();
    let out = d.drag(bar, pos2(info.right() - 10.0, info.center().y + 13.0));
    assert_eq!(out.actions, vec![Action::MoveGroup { panel: 50, to: Target::At { anchor: INFO, placement: Placement::Split(Zone::Right) } }]);
    d.settle();
    assert_eq!(d.layout.floating.len(), 1);
    assert_eq!(panel_set(&d.layout), all);
    // Main to floating: a tab joins the window's group.
    let fifty = group(&d, 50);
    let out = d.drag(tab(&d, SOURCE[1]), fifty.center() + vec2(0.0, 13.0));
    assert_eq!(out.actions, vec![Action::Move { panel: SOURCE[1], anchor: 50, placement: Placement::Split(Zone::Center) }]);
    d.settle();
    assert!(window_of(&d.layout, 50).contains(&SOURCE[1]));
    assert_eq!(panel_set(&d.layout), all);
    assert_eq!(d.layout.validate(), Ok(()));
}

#[test]
fn a_closed_floating_window_closes_its_panels_and_each_reopens_where_it_was() {
    let mut layout = editing();
    let tree = hsplit(0.4, tabs(&[INFO]), tabs(&[50]));
    layout.floating = vec![Floating::new(tree.clone(), [700.0, 100.0, 600.0, 400.0])];
    let mut d = driver(layout, Film::default());
    let locations: Vec<_> = [INFO, 50].into_iter().map(|p| (p, d.layout.location(&p).unwrap())).collect();
    let close = d.hit(&Hit::Close(INFO)).unwrap().center();
    let out = d.click(close);
    assert_eq!(out.actions, vec![Action::CloseWindow { panel: INFO }]);
    assert!(d.layout.floating.is_empty() && !d.layout.contains(&50));
    for (panel, location) in &locations {
        d.layout.restore(*panel, location).unwrap();
    }
    let window = &d.layout.floating[0];
    assert_eq!((&window.root, window.rect), (&tree, [700.0, 100.0, 600.0, 400.0]));
}
