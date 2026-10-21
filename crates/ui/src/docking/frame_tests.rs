//! Frame profile in a Premiere-style editing layout, as FilmCraft draws it: a bottom row of
//! Project | Tools | Timeline | Audio Meters, where Tools and the meters are narrow fixed-width
//! frames without a tab strip that a grip on top moves.

use std::collections::HashMap;

use super::testing::DockDriver;
use super::*;
use crate::layout::{SplitAxis, SplitSize};
use egui::{Id, Pos2, Rect, Sense, pos2, vec2};

const SOURCE: [u32; 4] = [1, 11, 12, 13];
const PROGRAM: u32 = 2;
const PROPERTIES: [u32; 3] = [3, 31, 32];
const PROJECT: [u32; 7] = [4, 41, 42, 43, 44, 45, 46];
const TOOLS: u32 = 5;
const TIMELINE: u32 = 6;
const METERS: u32 = 7;

const TOOLS_WIDTH: f32 = 64.0;
const METERS_WIDTH: f32 = 116.0;

/// Panel bodies by their rects, with a FilmCraft-style 8 pt grip on the bare frames.
#[derive(Default)]
struct Film {
    bodies: HashMap<u32, Rect>,
}

impl DockContent<u32> for Film {
    fn body(&mut self, ui: &mut egui::Ui, panel: &u32) {
        let frame = ui.max_rect();
        self.bodies.insert(*panel, frame);
        if self.bare(panel) {
            let grip = Rect::from_min_size(frame.min, vec2(frame.width(), 8.0_f32.min(frame.height())));
            if ui.interact(grip, Id::new(("grip", *panel)), Sense::drag()).drag_started() {
                begin_drag(ui.ctx(), Id::new("dock-driver"), *panel, frame);
            }
        }
        ui.allocate_rect(frame, Sense::hover());
    }
    fn logical_label(&self, panel: &u32) -> String {
        format!("Panel {panel}")
    }
    fn bare(&self, panel: &u32) -> bool {
        matches!(*panel, TOOLS | METERS)
    }
}

type Driver = DockDriver<u32, Film>;

fn tabs(panels: &[u32]) -> Node<u32> {
    Node::Tabs { panels: panels.to_vec(), active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<u32>, second: Node<u32>) -> Node<u32> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

fn hsplit(size: SplitSize, first: Node<u32>, second: Node<u32>) -> Node<u32> {
    split(SplitAxis::Horizontal, size, first, second)
}

/// Tools | Timeline | Audio Meters.
fn tools_row() -> Node<u32> {
    hsplit(SplitSize::FixedFirst(TOOLS_WIDTH), tabs(&[TOOLS]), hsplit(SplitSize::FixedSecond(METERS_WIDTH), tabs(&[TIMELINE]), tabs(&[METERS])))
}

/// FilmCraft's Editing workspace: Source | Program | Properties over Project | Tools | Timeline | Meters.
fn editing() -> Layout<u32> {
    let top = hsplit(SplitSize::FixedSecond(450.0), hsplit(SplitSize::Ratio(0.5), tabs(&SOURCE), tabs(&[PROGRAM])), tabs(&PROPERTIES));
    let bottom = hsplit(SplitSize::Ratio(0.29), tabs(&PROJECT), tools_row());
    Layout { root: Some(split(SplitAxis::Vertical, SplitSize::Ratio(0.57), top, bottom)), floating: Vec::new() }
}

fn driver(layout: Layout<u32>) -> Driver {
    let mut d = DockDriver::new(layout, Film::default()).with_style(|style| style.profile = Profile::Frame);
    d.screen = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 860.0));
    d.area = d.screen;
    d.settle();
    d
}

fn body(d: &Driver, panel: u32) -> Rect {
    d.content.bodies.get(&panel).copied().unwrap_or_else(|| panic!("panel {panel} not drawn"))
}

fn floated_project() -> Driver {
    let mut layout = editing();
    layout.apply(Action::MoveGroup { panel: PROJECT[0], to: Target::Float { rect: Some([600.0, 120.0, 400.0, 300.0]) } }).unwrap();
    driver(layout)
}

fn split_at(anchor: u32, zone: Zone) -> Target<u32> {
    Target::At { anchor, placement: Placement::Split(zone) }
}

#[test]
fn a_narrow_frame_offers_both_side_targets_and_a_seam_splits_beside_its_frames() {
    let d = floated_project();
    let tools = body(&d, TOOLS);
    assert_eq!((tools.left(), tools.width()), (0.0, TOOLS_WIDTH), "Tools starts the bottom row");
    // Left: past the 8 pt outer edge band.
    for (at, zone) in [(pos2(20.0, tools.center().y), Zone::Left), (pos2(tools.right() - 12.0, tools.center().y), Zone::Right)] {
        let mut d = floated_project();
        let bar = d.hit(&Hit::DragBar(PROJECT[0])).unwrap();
        let out = d.drag(bar.center(), at);
        assert_eq!(out.actions, vec![Action::MoveGroup { panel: PROJECT[0], to: split_at(TOOLS, zone) }], "{zone:?}");
    }
    // The gap between Tools and Timeline: a tab dropped there splits instead of floating.
    let mut d = floated_project();
    let timeline = body(&d, TIMELINE);
    let seam = pos2((tools.right() + timeline.left()) * 0.5, timeline.center().y);
    assert!(!tools.contains(seam) && !timeline.contains(seam), "the seam is outside both frames");
    let out = d.drag(d.hit(&Hit::Tab(PROGRAM)).unwrap().center(), seam);
    assert!(
        matches!(&out.actions[..], [Action::Move { panel: PROGRAM, anchor: TOOLS | TIMELINE, placement: Placement::Split(Zone::Left | Zone::Right) }]),
        "{:?}",
        out.actions
    );
}


fn bottom(layout: &Layout<u32>) -> &Node<u32> {
    let Some(Node::Split { second, .. }) = &layout.root else { panic!("{:?}", layout.root) };
    second
}

/// Drag a split divider by `dx`, hovering it first as a pointer does.
fn drag_divider(d: &mut Driver, path: &[bool], dx: f32) {
    let divider = d.hit(&Hit::Divider(Divider::Split(path.to_vec()))).unwrap_or_else(|| panic!("{path:?} in {:?}", d.hits()));
    d.move_to(divider.center());
    d.drag(divider.center(), divider.center() + vec2(dx, 0.0));
    d.settle();
}

#[test]
fn a_split_beside_a_fixed_width_frame_keeps_its_width_and_gets_its_own_divider() {
    // Left of Tools: the new frame takes half the row, beside Tools at its own width.
    let mut d = floated_project();
    let tools = body(&d, TOOLS);
    let bar = d.hit(&Hit::DragBar(PROJECT[0])).unwrap();
    let at = pos2(20.0, tools.center().y);
    d.hold(bar.center(), at);
    let row = Rect::from_x_y_ranges(0.0..=1440.0, tools.y_range());
    assert!(d.filled(Rect::from_min_max(row.min, pos2(row.center().x, row.bottom())).shrink(1.0)), "the preview shows the new frame");
    d.release(at);
    assert_eq!(bottom(&d.layout), &hsplit(SplitSize::Ratio(0.5), tabs(&PROJECT), tools_row()));
    d.settle();
    assert_eq!(body(&d, TOOLS).width(), TOOLS_WIDTH);
    // Each divider resizes its own two sides; Tools keeps its width.
    let project = body(&d, PROJECT[0]).width();
    drag_divider(&mut d, &[true], -200.0);
    let widths = |d: &Driver| (body(d, PROJECT[0]).width().round(), body(d, TOOLS).width().round());
    assert_eq!(widths(&d), ((project - 200.0).round(), TOOLS_WIDTH));
    drag_divider(&mut d, &[true, true], 30.0);
    assert_eq!(widths(&d), ((project - 200.0).round(), TOOLS_WIDTH + 30.0));

    // Right of Tools: the new frame takes half of what lies right of Tools.
    let mut d = driver(editing());
    let tools = body(&d, TOOLS);
    let tab = d.hit(&Hit::Tab(PROGRAM)).unwrap();
    d.drag(tab.center(), pos2(tools.right() - 12.0, tools.center().y));
    let rest = hsplit(SplitSize::FixedSecond(METERS_WIDTH), tabs(&[TIMELINE]), tabs(&[METERS]));
    let right = hsplit(SplitSize::FixedFirst(TOOLS_WIDTH), tabs(&[TOOLS]), hsplit(SplitSize::Ratio(0.5), tabs(&[PROGRAM]), rest));
    assert_eq!(bottom(&d.layout), &hsplit(SplitSize::Ratio(0.29), tabs(&PROJECT), right));
    d.settle();
    assert_eq!(body(&d, TOOLS).width(), TOOLS_WIDTH);
}
