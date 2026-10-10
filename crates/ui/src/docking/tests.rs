use super::*;
use crate::layout::{SplitAxis, SplitSize};
use egui::{Context, Event, Id, Modifiers, PointerButton, RawInput, Rect, pos2, vec2};

fn tabs(panels: &[u32], active: usize) -> Node<u32> {
    Node::Tabs { panels: panels.to_vec(), active }
}

fn sample() -> Layout<u32> {
    Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedSecond(240.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(tabs(&[2, 3, 4], 1)),
        }),
        floating: vec![Floating { panels: vec![5, 6], active: 1, rect: [80.0, 70.0, 300.0, 220.0] }],
    }
}

fn move_to(panel: u32, anchor: u32, zone: Zone) -> Action<u32> {
    Action::Move { panel, anchor, placement: Placement::Split(zone) }
}

#[test]
fn floating_brush_panel_joins_main_tabs_without_losing_other_panels() {
    let mut layout = sample();
    layout.apply(Action::Move { panel: 5, anchor: 3, placement: Placement::Tab { before: Some(4) } }).unwrap();
    let Node::Split { size, second, .. } = layout.root.as_ref().unwrap() else { panic!() };
    assert_eq!(*size, SplitSize::FixedSecond(240.0));
    assert_eq!(**second, tabs(&[2, 3, 5, 4], 2));
    assert_eq!(layout.floating[0].panels, vec![6]);
    assert_eq!(layout.floating[0].active, 0);
    layout.validate().unwrap();
    assert_eq!(layout.panels().len(), 6);
}

#[test]
fn detach_redock_merge_and_collapse_empty_source_containers() {
    let mut layout = sample();
    layout.apply(Action::Float { panel: 1, rect: [30.0, 40.0, 250.0, 200.0] }).unwrap();
    assert_eq!(layout.root, Some(tabs(&[2, 3, 4], 1)));
    layout.apply(move_to(1, 2, Zone::Left)).unwrap();
    assert_eq!(layout.floating.len(), 1);
    layout.apply(move_to(5, 6, Zone::Center)).unwrap();
    assert_eq!(layout.floating[0].panels, vec![6, 5]);
    assert_eq!(layout.floating[0].active, 1);
    layout.apply(move_to(6, 3, Zone::Center)).unwrap();
    layout.apply(move_to(5, 3, Zone::Center)).unwrap();
    assert!(layout.floating.is_empty());
    assert_eq!(layout.panels().len(), 6);
}

#[test]
fn invalid_moves_are_atomic_including_after_a_source_would_have_been_removed() {
    let layout = sample();
    for action in [
        move_to(5, 99, Zone::Center),
        move_to(99, 2, Zone::Center),
        move_to(5, 6, Zone::Left),
        move_to(5, 5, Zone::Center),
        Action::Move { panel: 5, anchor: 2, placement: Placement::Tab { before: Some(6) } },
        Action::Float { panel: 3, rect: [f32::NAN, 0.0, 100.0, 100.0] },
        Action::Float { panel: 3, rect: [0.0, 0.0, 0.0, 100.0] },
        Action::ResizeSplit { path: vec![false], size: SplitSize::Ratio(0.3) },
    ] {
        let mut changed = layout.clone();
        assert!(changed.apply(action).is_err());
        assert_eq!(changed, layout);
    }
}

#[test]
fn removing_an_earlier_tab_preserves_selected_identity() {
    let mut layout = Layout { root: Some(tabs(&[1, 2, 3, 4], 2)), floating: Vec::new() };
    layout.apply(Action::Close { panel: 1 }).unwrap();
    assert_eq!(layout.root, Some(tabs(&[2, 3, 4], 1)));
    layout.apply(Action::Close { panel: 3 }).unwrap();
    assert_eq!(layout.root, Some(tabs(&[2, 4], 1)));
}

#[test]
fn reordering_tabs_uses_destination_identity_after_removal() {
    let mut layout = Layout { root: Some(tabs(&[1, 2, 3], 0)), floating: Vec::new() };
    layout.apply(Action::Move { panel: 1, anchor: 2, placement: Placement::Tab { before: None } }).unwrap();
    assert_eq!(layout.root, Some(tabs(&[2, 3, 1], 2)));
    layout.apply(Action::Move { panel: 1, anchor: 2, placement: Placement::Tab { before: Some(2) } }).unwrap();
    assert_eq!(layout.root, Some(tabs(&[1, 2, 3], 0)));
    let before = layout.clone();
    layout.apply(Action::Move { panel: 1, anchor: 2, placement: Placement::Tab { before: Some(1) } }).unwrap();
    assert_eq!(layout, before);
}

#[test]
fn accordion_open_state_and_custom_height_survive_reorder() {
    let mut layout = Layout {
        root: Some(Node::Stack {
            entries: vec![StackEntry { panel: 1, open: false, height: Some(123.0) }, StackEntry { panel: 2, open: true, height: None }],
        }),
        floating: Vec::new(),
    };
    layout.apply(Action::Move { panel: 1, anchor: 2, placement: Placement::Tab { before: None } }).unwrap();
    let Node::Stack { entries } = layout.root.as_ref().unwrap() else { panic!() };
    assert_eq!(entries[1], StackEntry { panel: 1, open: false, height: Some(123.0) });
    let location = layout.location(&1).unwrap();
    layout.apply(Action::Close { panel: 1 }).unwrap();
    layout.restore(1, &location).unwrap();
    let Node::Stack { entries } = layout.root.as_ref().unwrap() else { panic!() };
    assert_eq!(entries[1], StackEntry { panel: 1, open: false, height: Some(123.0) });
}

#[test]
fn protected_canvas_remains_present_and_cannot_be_grouped_with_panels() {
    let permissions = |panel: &u32| if *panel == 1 { Permissions::PROTECTED } else { Permissions::default() };
    let layout = sample();
    for action in [
        Action::Close { panel: 1 },
        Action::Float { panel: 1, rect: [10.0, 10.0, 200.0, 200.0] },
        move_to(1, 3, Zone::Center),
        move_to(3, 1, Zone::Center),
        Action::OpenAt { panel: 1, anchor: 3, placement: Placement::Tab { before: None } },
    ] {
        let mut next = layout.clone();
        assert_eq!(next.apply_with_permissions(action, permissions), Err(DockError::NotAllowed));
        assert_eq!(next, layout);
    }
    let mut next = layout;
    next.apply_with_permissions(Action::OpenAt { panel: 7, anchor: 1, placement: Placement::Split(Zone::Left) }, permissions).unwrap();
    assert!(next.contains(&1) && next.contains(&7));
}

#[test]
fn close_and_restore_preserve_fixed_side_and_floating_geometry() {
    let mut layout = Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedFirst(180.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(tabs(&[2], 0)),
        }),
        floating: vec![Floating { panels: vec![3], active: 0, rect: [41.0, 52.0, 233.0, 244.0] }],
    };
    let saved = layout.clone();
    let first = layout.location(&1).unwrap();
    let float = layout.location(&3).unwrap();
    layout.apply(Action::Close { panel: 1 }).unwrap();
    layout.apply(Action::Close { panel: 3 }).unwrap();
    layout.restore(1, &first).unwrap();
    layout.restore(3, &float).unwrap();
    assert_eq!(layout, saved);
    let invalid = Location { anchor: Some(99), ..first };
    layout.apply(Action::Close { panel: 1 }).unwrap();
    let before = layout.clone();
    assert_eq!(layout.restore(1, &invalid), Err(DockError::MissingTarget));
    assert_eq!(layout, before);
}

#[test]
fn closing_and_reopening_an_entire_accordion_restores_its_kind_and_heights() {
    let mut layout = Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedSecond(220.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(Node::Stack {
                entries: vec![StackEntry { panel: 2, open: false, height: Some(90.0) }, StackEntry { panel: 3, open: true, height: Some(180.0) }],
            }),
        }),
        floating: Vec::new(),
    };
    let before = layout.clone();
    let second = layout.location(&2).unwrap();
    layout.apply(Action::Close { panel: 2 }).unwrap();
    let third = layout.location(&3).unwrap();
    layout.apply(Action::Close { panel: 3 }).unwrap();
    layout.restore(3, &third).unwrap();
    layout.restore(2, &second).unwrap();
    assert_eq!(layout, before);
}

#[test]
fn invalid_saved_layouts_reject_duplicates_selection_depth_and_nonfinite_geometry() {
    let mut duplicate = sample();
    duplicate.floating[0].panels.push(1);
    assert_eq!(duplicate.validate(), Err(DockError::DuplicatePanel));
    let mut invalid = sample();
    invalid.floating[0].active = usize::MAX;
    assert_eq!(invalid.validate(), Err(DockError::InvalidSelection));
    invalid.floating[0].active = 0;
    invalid.floating[0].rect[3] = f32::INFINITY;
    assert_eq!(invalid.validate(), Err(DockError::InvalidGeometry));
    let mut deep = tabs(&[0], 0);
    for panel in 1..=MAX_DEPTH as u32 {
        deep = Node::Split { axis: SplitAxis::Horizontal, size: SplitSize::Ratio(0.5), first: Box::new(deep), second: Box::new(tabs(&[panel], 0)) };
    }
    let mut deep = Layout { root: Some(deep), floating: Vec::new() };
    assert_eq!(deep.validate(), Err(DockError::LimitExceeded));
    assert_eq!(deep.apply(Action::Activate { panel: 0 }), Err(DockError::LimitExceeded));
    let oversized = Layout { root: Some(Node::Tabs { panels: (0..=MAX_PANELS as u32).collect(), active: 0 }), floating: Vec::new() };
    assert_eq!(oversized.validate(), Err(DockError::LimitExceeded));
}

fn target(anchor: u32, members: &[u32], rect: Rect) -> DropTarget<u32> {
    let strip = Rect::from_min_size(rect.min, vec2(rect.width(), 24.0));
    DropTarget {
        anchor,
        rect,
        tab_strip: Some(strip),
        tabs: members
            .iter()
            .enumerate()
            .map(|(i, panel)| (*panel, Rect::from_min_size(rect.min + vec2(i as f32 * 60.0, 0.0), vec2(60.0, 24.0))))
            .collect(),
        allow_splits: true,
        accepts_tabs: true,
    }
}

#[test]
fn pointer_targets_distinguish_reorder_center_split_and_occlusion() {
    let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 200.0));
    let group = target(2, &[2, 3], rect);
    let placement = resolve_drop(&5, pos2(90.0, 30.0), std::slice::from_ref(&group)).unwrap();
    assert_eq!(placement.placement, Placement::Tab { before: Some(3) });
    assert!(placement.insertion.unwrap().is_finite());
    assert_eq!(resolve_drop(&5, rect.center(), std::slice::from_ref(&group)).unwrap().placement, Placement::Split(Zone::Center));
    assert_eq!(resolve_drop(&5, pos2(11.0, 90.0), std::slice::from_ref(&group)).unwrap().placement, Placement::Split(Zone::Left));
    assert!(resolve_drop(&3, rect.center(), std::slice::from_ref(&group)).is_none());
    assert!(resolve_drop(&3, pos2(11.0, 90.0), std::slice::from_ref(&group)).is_some());
    let mut overlay = target(5, &[5], rect);
    overlay.accepts_tabs = false;
    overlay.allow_splits = false;
    assert!(resolve_drop(&3, rect.center(), &[group, overlay]).is_none());
    assert!(drop_zone(Rect::NOTHING, 24.0, pos2(0.0, 0.0)).is_none());
    assert!(drop_zone(rect, 24.0, pos2(f32::NAN, 0.0)).is_none());
}

#[test]
fn long_action_sequence_never_loses_or_duplicates_a_panel() {
    let mut layout = sample();
    let mut seed = 72u64;
    for step in 0..800 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let panel = (seed % 6 + 1) as u32;
        let anchor = ((seed >> 16) % 6 + 1) as u32;
        let action = if step % 7 == 0 {
            Action::Float { panel, rect: [10.0, 20.0, 200.0, 220.0] }
        } else {
            move_to(
                panel,
                anchor,
                match seed % 5 {
                    0 => Zone::Left,
                    1 => Zone::Right,
                    2 => Zone::Top,
                    3 => Zone::Bottom,
                    _ => Zone::Center,
                },
            )
        };
        let before = layout.clone();
        if layout.apply(action).is_err() {
            assert_eq!(layout, before);
        }
        layout.validate().unwrap();
        let mut panels: Vec<u32> = layout.panels().into_iter().copied().collect();
        panels.sort_unstable();
        assert_eq!(panels, vec![1, 2, 3, 4, 5, 6]);
    }
}

#[cfg(feature = "serde")]
#[test]
fn saved_layout_and_hidden_location_round_trip() {
    let layout = sample();
    let value = serde_json::to_string(&layout).unwrap();
    let restored: Layout<u32> = serde_json::from_str(&value).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, layout);
    let location = layout.location(&3).unwrap();
    let encoded = serde_json::to_string(&location).unwrap();
    assert_eq!(serde_json::from_str::<Location<u32>>(&encoded).unwrap(), location);
}

#[cfg(feature = "serde")]
#[test]
fn old_hidden_locations_accept_panel_ids_without_a_default_identity() {
    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Panel {
        Layers,
        Brushes,
    }
    let location: Location<Panel> = serde_json::from_value(serde_json::json!({
        "floating": null, "anchor": "Layers", "placement": {"Tab": {"before": null}},
        "size": null, "stack": null
    }))
    .unwrap();
    assert!(location.sibling.is_empty());
    assert_eq!(location.anchor, Some(Panel::Layers));
}

fn frame(ctx: &Context, layout: &Layout<u32>, events: Vec<Event>, focused: bool) -> (DockOutput<u32>, Vec<Id>) {
    let mut output = DockOutput::default();
    let mut ids = Vec::new();
    let input = RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), events, focused, ..Default::default() };
    ctx.run_ui(input, |ui| {
        let style = DockStyle::from_ui(ui);
        output = DockArea::new(Id::new("test-area")).show(
            ui,
            layout,
            &style,
            |p| format!("Panel {p}"),
            |_| Permissions::default(),
            |ui, _| {
                ids.push(ui.id());
                ui.label("Actual panel content");
            },
        );
    })
    .textures_delta
    .clear();
    (output, ids)
}

#[test]
fn renderer_keeps_content_identity_when_panel_moves_into_a_float() {
    let ctx = Context::default();
    let mut layout = Layout { root: Some(tabs(&[1], 0)), floating: Vec::new() };
    let (output, before) = frame(&ctx, &layout, Vec::new(), true);
    assert!(output.error.is_none());
    layout.apply(Action::Float { panel: 1, rect: [100.0, 100.0, 320.0, 260.0] }).unwrap();
    let (output, after) = frame(&ctx, &layout, Vec::new(), true);
    assert!(output.error.is_none());
    assert_eq!(before, after);
}

#[test]
fn cancelling_or_losing_focus_during_a_drag_does_not_move_panels() {
    let ctx = Context::default();
    let layout = sample();
    let source = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 200.0));
    let press = Event::PointerButton { pos: pos2(30.0, 30.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE };
    frame(&ctx, &layout, vec![Event::PointerMoved(pos2(30.0, 30.0)), press], true);
    assert!(begin_drag(&ctx, Id::new("test-area"), 2u32, source));
    let (out, _) = frame(
        &ctx,
        &layout,
        vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
        true,
    );
    assert!(out.actions.is_empty());
    assert!(begin_drag(&ctx, Id::new("test-area"), 2u32, source));
    let (out, _) = frame(&ctx, &layout, Vec::new(), false);
    assert!(out.actions.is_empty());
    let (out, _) = frame(
        &ctx,
        &layout,
        vec![Event::PointerButton { pos: pos2(700.0, 400.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        true,
    );
    assert!(out.actions.is_empty());
}

fn step(ctx: &Context, layout: &mut Layout<u32>, events: Vec<Event>) -> Vec<Action<u32>> {
    let (out, _) = frame(ctx, layout, events, true);
    assert!(out.error.is_none());
    for action in &out.actions {
        layout.apply(action.clone()).unwrap();
    }
    out.actions
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

#[test]
fn actual_pointer_drag_docks_a_floating_panel_into_the_main_tab_strip() {
    let ctx = Context::default();
    let mut layout = sample();
    for _ in 0..3 {
        step(&ctx, &mut layout, Vec::new());
    }
    let source = ctx.read_response(Id::new("test-area").with(("tab", &5u32))).unwrap().rect.center();
    let target = ctx.read_response(Id::new("test-area").with(("tab", &3u32))).unwrap().rect.center();
    step(&ctx, &mut layout, vec![Event::PointerMoved(source), pointer_button(source, true)]);
    step(&ctx, &mut layout, vec![Event::PointerMoved(source + vec2(8.0, 0.0))]);
    step(&ctx, &mut layout, vec![Event::PointerMoved(target)]);
    let actions = step(&ctx, &mut layout, vec![pointer_button(target, false)]);
    assert!(actions.iter().any(|a| matches!(a, Action::Move { panel: 5, .. })), "{actions:?}");
    assert!(!layout.floating.iter().any(|g| g.panels.contains(&5)));
    assert_eq!(layout.panels().len(), 6);
    let Node::Split { second, .. } = layout.root.as_ref().unwrap() else { panic!() };
    let Node::Tabs { panels, active } = second.as_ref() else { panic!() };
    assert_eq!(panels.get(*active), Some(&5));
}

#[test]
fn panel_size_limits_clamp_geometry_without_overwriting_preferred_widths() {
    let layout = Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedFirst(450.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(Node::Split {
                axis: SplitAxis::Horizontal,
                size: SplitSize::FixedSecond(500.0),
                first: Box::new(tabs(&[0], 0)),
                second: Box::new(tabs(&[2], 0)),
            }),
        }),
        floating: Vec::new(),
    };
    let unchanged = layout.clone();
    let ctx = Context::default();
    let mut widths = Vec::new();
    for width in [1400.0, 900.0, 300.0, 1400.0] {
        let mut rects = std::collections::BTreeMap::new();
        let mut out = DockOutput::default();
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 600.0))), ..Default::default() }, |ui| {
            out = DockArea::new(Id::new("bounds")).show_with_limits(
                ui,
                &layout,
                &DockStyle::from_ui(ui),
                |p| p.to_string(),
                |p| if *p == 0 { Permissions::PROTECTED } else { Permissions::default() },
                |p| match *p {
                    1 => PanelLimits { min: vec2(200.0, 40.0), max: vec2(480.0, 1_000_000.0) },
                    2 => PanelLimits { min: vec2(250.0, 40.0), max: vec2(520.0, 1_000_000.0) },
                    _ => PanelLimits { min: vec2(360.0, 40.0), ..Default::default() },
                },
                |ui, p| {
                    rects.insert(*p, ui.max_rect());
                },
            );
        })
        .textures_delta
        .clear();
        assert!(out.error.is_none());
        assert!(out.actions.is_empty());
        for rect in rects.values() {
            assert!(rect.is_finite() && rect.is_positive());
            assert!(rect.left() >= 0.0 && rect.right() <= width);
        }
        if width >= 900.0 {
            assert!((200.0..=480.0).contains(&rects[&1].width()));
            assert!((250.0..=520.0).contains(&rects[&2].width()));
            assert!(rects[&0].width() >= 360.0);
        }
        widths.push(rects[&1].width());
    }
    assert_eq!(widths.first(), widths.last());
    assert_eq!(layout, unchanged);
}

#[test]
fn focused_floating_resize_grip_accepts_both_arrow_axes_and_releases_tab() {
    let ctx = Context::default();
    let mut layout = sample();
    for _ in 0..3 {
        step(&ctx, &mut layout, Vec::new());
    }
    let id = Id::new("test-area").with(("floating", &5u32)).with("resize");
    ctx.memory_mut(|memory| memory.request_focus(id));
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    step(&ctx, &mut layout, vec![key(egui::Key::ArrowRight), key(egui::Key::ArrowDown)]);
    assert_eq!(layout.floating[0].rect, [80.0, 70.0, 308.0, 228.0]);
    step(&ctx, &mut layout, vec![key(egui::Key::Tab)]);
    assert_ne!(ctx.memory(|memory| memory.focused()), Some(id));
}

#[test]
fn stack_dividers_resize_adjacent_open_bodies_by_pointer_and_keyboard() {
    let ctx = Context::default();
    let mut layout = Layout {
        root: Some(Node::Stack {
            entries: vec![
                StackEntry { panel: 1, open: true, height: Some(150.0) },
                StackEntry { panel: 2, open: false, height: Some(91.0) },
                StackEntry { panel: 3, open: true, height: Some(150.0) },
                StackEntry { panel: 4, open: true, height: Some(120.0) },
            ],
        }),
        floating: Vec::new(),
    };
    for _ in 0..3 {
        step(&ctx, &mut layout, Vec::new());
    }
    let id = Id::new("test-area").with(("stack-resize", &1u32));
    let start = ctx.read_response(id).unwrap().rect.center();
    step(&ctx, &mut layout, vec![Event::PointerMoved(start), pointer_button(start, true)]);
    step(&ctx, &mut layout, vec![Event::PointerMoved(start + vec2(0.0, 24.0))]);
    step(&ctx, &mut layout, vec![pointer_button(start + vec2(0.0, 24.0), false)]);
    let Node::Stack { entries } = layout.root.as_ref().unwrap() else { panic!() };
    let after_pointer = entries[0].height.unwrap();
    assert!(after_pointer > 150.0);
    assert!((entries[0].height.unwrap() + entries[2].height.unwrap() - 300.0).abs() < 0.001);
    assert_eq!(entries[1].height, Some(91.0));
    assert!(!entries[1].open);
    assert_eq!(entries[3].height, Some(120.0));
    ctx.memory_mut(|memory| memory.request_focus(id));
    step(
        &ctx,
        &mut layout,
        vec![Event::Key { key: egui::Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    );
    let Node::Stack { entries } = layout.root.as_ref().unwrap() else { panic!() };
    assert!(entries[0].height.unwrap() > after_pointer);
    assert!((entries[0].height.unwrap() + entries[2].height.unwrap() - 300.0).abs() < 0.001);
}

#[test]
fn split_inner_edge_remains_draggable_beside_a_scrollbar() {
    let ctx = Context::default();
    let mut layout = Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedFirst(220.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(tabs(&[2], 0)),
        }),
        floating: Vec::new(),
    };
    let frame = |layout: &mut Layout<u32>, events| {
        let mut output = DockOutput::default();
        let mut sidebar = Rect::NOTHING;
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), events, ..Default::default() }, |ui| {
            output = DockArea::new(Id::new("scrolling-dock")).show(
                ui,
                layout,
                &DockStyle::from_ui(ui),
                |p| format!("Panel {p}"),
                |_| Permissions::default(),
                |ui, panel| {
                    if *panel == 1 {
                        sidebar = ui.max_rect();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                            .show(ui, |ui| {
                                for row in 0..100 {
                                    ui.label(format!("Library item {row}"));
                                }
                            });
                    } else {
                        ui.label("Document");
                    }
                },
            );
        })
        .textures_delta
        .clear();
        assert!(output.error.is_none());
        for action in output.actions {
            layout.apply(action).unwrap();
        }
        sidebar
    };
    for _ in 0..3 {
        frame(&mut layout, Vec::new());
    }
    let sidebar = frame(&mut layout, Vec::new());
    let start = pos2(sidebar.right() - 2.0, sidebar.center().y);
    frame(&mut layout, vec![Event::PointerMoved(start), pointer_button(start, true)]);
    frame(&mut layout, vec![Event::PointerMoved(start + vec2(40.0, 0.0))]);
    frame(&mut layout, vec![pointer_button(start + vec2(40.0, 0.0), false)]);
    let Node::Split { size: SplitSize::FixedFirst(width), .. } = layout.root.unwrap() else { panic!() };
    assert!((width - 260.0).abs() < 0.001, "sidebar width after inner-edge drag: {width}");
}

#[test]
fn restoring_a_panel_beside_a_nested_subtree_preserves_the_complete_workspace() {
    let mut layout = Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Vertical,
            size: SplitSize::FixedSecond(330.0),
            first: Box::new(tabs(&[1], 0)),
            second: Box::new(Node::Split {
                axis: SplitAxis::Horizontal,
                size: SplitSize::FixedFirst(220.0),
                first: Box::new(tabs(&[2], 0)),
                second: Box::new(tabs(&[3], 0)),
            }),
        }),
        floating: Vec::new(),
    };
    let expected = layout.clone();
    let location = layout.location(&1).unwrap();
    layout.apply(Action::Close { panel: 1 }).unwrap();
    layout.restore(1, &location).unwrap();
    assert_eq!(layout, expected);
}

#[test]
fn floating_only_surface_does_not_allocate_the_callers_document_region() {
    let ctx = Context::default();
    let layout = Layout { root: None, floating: vec![Floating { panels: vec![1u32], active: 0, rect: [30.0, 40.0, 200.0, 200.0] }] };
    ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() }, |ui| {
        let before = ui.available_rect_before_wrap();
        let out = DockArea::new(Id::new("float-only")).show(
            ui,
            &layout,
            &DockStyle::from_ui(ui),
            |p| p.to_string(),
            |_| Permissions::default(),
            |ui, _| {
                ui.label("Panel");
            },
        );
        assert!(out.error.is_none());
        assert_eq!(ui.available_rect_before_wrap(), before);
    })
    .textures_delta
    .clear();
}
