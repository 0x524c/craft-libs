//! Fixes from the DesignCraft trial: group float size, tab label elision, the drag ghost and
//! grip, double-clicks on minimized groups, pointer focus, label keys and borrowed lookups.

use super::*;

/// Every shape, with nested `Shape::Vec`s flattened.
fn flat(shapes: &[egui::epaint::ClippedShape]) -> Vec<(Rect, egui::Shape)> {
    fn walk(clip: Rect, shape: &egui::Shape, out: &mut Vec<(Rect, egui::Shape)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(clip, s, out)),
            shape => out.push((clip, shape.clone())),
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(clipped.clip_rect, &clipped.shape, &mut out);
    }
    out
}

fn texts(d: &Driver) -> Vec<(Rect, egui::epaint::TextShape)> {
    flat(&d.shapes)
        .into_iter()
        .filter_map(|(clip, shape)| match shape {
            egui::Shape::Text(text) => Some((clip, text)),
            _ => None,
        })
        .collect()
}

fn rects(d: &Driver) -> Vec<egui::epaint::RectShape> {
    flat(&d.shapes)
        .into_iter()
        .filter_map(|(_, shape)| match shape {
            egui::Shape::Rect(rect) => Some(rect),
            _ => None,
        })
        .collect()
}

fn last_char(galley: &egui::Galley) -> Option<char> {
    galley.rows.last().and_then(|row| row.glyphs.last()).map(|glyph| glyph.chr)
}

#[test]
fn a_full_height_docked_group_floats_at_the_host_or_capped_height_inside_the_window() {
    for size in [None, Some(vec2(200.0, 150.0))] {
        let mut layout = docked();
        let Some(Node::Split { first, .. }) = &mut layout.root else { panic!() };
        **first = Node::Dock { side: Side::Start, columns: vec![Column::new(vec![Group::new(vec![2, 3], 1)], 240.0)] };
        let content = TestContent { protected: vec![0], floating_size: size, ..Default::default() };
        let mut d = Driver::with(layout, content, Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 600.0)));
        let bar = d.rect("drag-bar", 2);
        let out = d.drag(bar.center(), pos2(420.0, 300.0));
        let Some(Target::Float { rect: Some([x, y, width, height]) }) = moved_group(&out.actions) else { panic!("{:?}", out.actions) };
        match size {
            Some(size) => assert_eq!(*height, size.y),
            None => assert!(*height <= 320.0, "{height}"),
        }
        assert!(*width > 100.0 && *width <= 240.0, "{width}");
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        assert!(window.contains_rect(Rect::from_min_size(pos2(*x, *y), vec2(*width, *height))), "{x} {y} {width} {height}");
    }
}

/// Labels as a host's pre-laid-out galleys, which egui does not wrap again.
struct GalleyLabels;

impl DockContent<u32> for GalleyLabels {
    fn body(&mut self, _: &mut egui::Ui, _: &u32) {}
    fn logical_label(&self, panel: &u32) -> String {
        format!("Character Styles {panel}")
    }
    fn label(&mut self, ui: &egui::Ui, _: &u32, logical: &str, font: &egui::FontId, color: Color32) -> egui::WidgetText {
        egui::WidgetText::Galley(ui.painter().layout_no_wrap(logical.to_owned(), font.clone(), color))
    }
}

#[test]
fn shrunk_tabs_end_their_labels_with_an_ellipsis_inside_the_tab_active_or_not() {
    let panels = [1, 2, 3, 4, 5, 6];
    let layout = Layout { root: Some(tabs(&panels, 3)), floating: Vec::new() };
    let ctx = Context::default();
    let mut content = GalleyLabels;
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() };
        let mut full = ctx.run_ui(input, |ui| {
            let style = DockStyle::from_ui(ui);
            ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 400.0))), |ui| {
                DockArea::new(Id::new(AREA)).show_content(ui, &layout, &style, &mut content);
            });
        });
        full.textures_delta.clear();
        shapes = full.shapes;
    }
    let texts: Vec<_> = flat(&shapes)
        .into_iter()
        .filter_map(|(clip, shape)| match shape {
            egui::Shape::Text(text) => Some((clip, text)),
            _ => None,
        })
        .collect();
    for panel in panels {
        let tab = ctx.read_response(Id::new(AREA).with(("tab", &panel))).unwrap().rect;
        let Some((clip, text)) = texts.iter().find(|(_, t)| t.galley.text().ends_with(&format!(" {panel}")) && tab.contains(t.pos)) else {
            panic!("no label in tab {panel}")
        };
        assert_eq!(last_char(&text.galley), Some('…'), "tab {panel}");
        let drawn = Rect::from_min_size(text.pos, text.galley.size());
        assert!(drawn.right() <= tab.right() + 0.5 && drawn.left() >= tab.left(), "tab {panel}: {drawn:?} in {tab:?}");
        assert!(tab.contains_rect(clip.intersect(drawn)), "tab {panel}");
    }
}

#[test]
fn drag_bar_shows_a_grip_at_rest_and_a_group_drag_paints_a_filled_labelled_ghost() {
    let mut d = Driver::new(frames());
    let visuals = d.ctx.global_style().visuals.clone();
    let bar = d.rect("drag-bar", 2);
    let grip = flat(&d.shapes).into_iter().any(|(_, shape)| match shape {
        egui::Shape::LineSegment { points, stroke } => {
            bar.contains(points[0]) && bar.contains(points[1]) && stroke.color == visuals.weak_text_color()
        }
        _ => false,
    });
    assert!(grip, "no grip mark in the drag bar at rest");

    d.hold(bar.center(), pos2(150.0, 300.0));
    let fill = visuals.selection.stroke.color.gamma_multiply(0.15);
    let Some(ghost) = rects(&d).into_iter().find(|r| r.fill == fill && r.stroke.width == 2.0 && r.rect.contains(pos2(150.0, 300.0))) else {
        panic!("no filled ghost under the pointer")
    };
    for label in ["Panel 2", "Panel 3"] {
        assert!(texts(&d).iter().any(|(_, t)| t.galley.text() == label && ghost.rect.contains(t.pos)), "{label} missing from the ghost");
    }
    d.step(vec![key(egui::Key::Escape)]);
}

#[test]
fn double_click_on_a_tab_of_a_minimized_group_ends_expanded() {
    let layout = Layout { root: Some(tabs(&[1], 0)), floating: vec![Floating::single(vec![5, 6], 1, [80.0, 70.0, 300.0, 220.0])] };
    let mut d = Driver::new(layout);
    let minimized = |d: &Driver| d.layout.floating[0].groups[0].minimized;
    let active = d.tab(6).center();
    d.click(active);
    d.click(active);
    assert!(minimized(&d));
    d.wait();

    let other = d.tab(5).center();
    d.click(other);
    d.click(other);
    assert!(!minimized(&d));
    assert_eq!(d.layout.floating[0].groups[0].active_panel(), Some(&5));
}

#[test]
fn a_clicked_tab_shows_no_focus_outline_but_tab_key_focus_does() {
    let mut d = Driver::new(frames());
    let accent = d.ctx.global_style().visuals.selection.stroke.color;
    let id = Id::new(AREA).with(("tab", &3u32));
    let outline = d.tab(3).shrink(2.0);
    let outlined = |d: &Driver| rects(d).iter().any(|r| r.rect == outline && r.stroke.width > 0.0 && r.stroke.color == accent);

    d.click(d.tab(3).center());
    // A platform may focus the clicked widget, as an AccessKit focus request does.
    d.ctx.memory_mut(|m| m.request_focus(id));
    d.step(vec![]);
    assert_eq!(d.ctx.memory(|m| m.focused()), Some(id));
    assert!(!outlined(&d));

    d.ctx.memory_mut(|m| m.surrender_focus(id));
    for _ in 0..20 {
        if d.ctx.memory(|m| m.focused()) == Some(id) {
            break;
        }
        d.step(vec![key(egui::Key::Tab)]);
    }
    assert_eq!(d.ctx.memory(|m| m.focused()), Some(id));
    d.step(vec![]);
    assert!(outlined(&d));
}

#[test]
fn localized_covers_every_label_and_contains_takes_a_borrowed_panel() {
    let mut debug = String::new();
    let mut full = Context::default().run_ui(RawInput::default(), |ui| {
        debug = format!("{:?}", DockStyle::from_ui(ui).localized(|key| Some(key.to_owned())));
    });
    full.textures_delta.clear();
    let values: Vec<&str> = debug.split("_label: \"").skip(1).filter_map(|rest| rest.split('"').next()).collect();
    assert_eq!(values.len(), 24, "{debug}");
    assert!(values.iter().all(|v| v.starts_with("dock.")), "a label without a key: {values:?}");
    let unique: std::collections::HashSet<_> = values.iter().collect();
    assert_eq!(unique.len(), values.len());

    let window = Floating::single(vec!["swatches".to_owned()], 0, [0.0, 0.0, 200.0, 200.0]);
    assert!(window.contains("swatches"));
    assert!(!window.contains("layers"));
}
