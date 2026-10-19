//! Where each docking gesture starts, for control protocols and tests.

use std::{collections::HashMap, fmt::Debug, hash::Hash};

use egui::{Context, Id, Rect};

use super::{DockArea, Layout, Node, render::RESIZE_HANDLES};

/// A rendered docking control that starts a gesture. Groups, columns and windows are named by
/// their first panel.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Hit<P> {
    /// A tab: click activates, drag moves the panel (S1).
    Tab(P),
    /// A group's drag bar: click opens the group menu, drag moves the group (S2).
    DragBar(P),
    /// The top border of a group's strip: drag moves the group (S3).
    Border(P),
    /// The "…" button of a group whose tabs overflow; named by the active panel.
    Overflow(P),
    /// [D] A column bar: drag moves the column (S6).
    ColumnBar(P),
    /// [D] The »/« button of a column bar (M2).
    Collapse(P),
    /// An accordion entry's −/+ button.
    StackToggle(P),
    /// A floating window's title bar (S5).
    Title(P),
    /// A floating window's × button.
    Close(P),
    /// [D] A panel's icon in an iconic column (S7, M5).
    Icon(P),
    /// [D] The grip above an icon group (S8).
    IconGrip(P),
    /// A floating window's resize handle.
    Resize(P, ResizeEdge),
    Divider(Divider<P>),
}

/// The edge or corner a floating window's resize handle moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResizeEdge {
    Left,
    Right,
    Bottom,
    BottomLeft,
    BottomRight,
}

/// A divider that resizes what is on either side of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Divider<P> {
    /// A split, by its path from the root (see [`super::Action::ResizeSplit`]).
    Split(Vec<bool>),
    /// Below the accordion entry of this panel.
    Stack(P),
    /// [D] The inner edge of the column holding this panel first.
    Column(P),
    /// Below the dock or floating group holding this panel first.
    Group(P),
}

fn key(area: Id) -> Id {
    area.with("hit-rects")
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> DockArea<'_, P> {
    /// Every gesture source the area drew in its last frame, with its rect in points. Hosts
    /// return these from a `ui.docking` control method beside the layout and `focused`; tests
    /// drag from their centres.
    pub fn hit_rects(ctx: &Context, id: Id) -> Vec<(Hit<P>, Rect)> {
        ctx.data(|d| d.get_temp::<Vec<(Hit<P>, Rect)>>(key(id))).unwrap_or_default()
    }
}

/// Name the controls the renderer registered this frame and store their rects.
pub(super) fn record<P: Clone + Eq + Hash + Debug + Send + Sync + 'static>(ctx: &Context, area: Id, layout: &Layout<P>, owned: &[Id]) {
    let mut names: HashMap<Id, Hit<P>> = HashMap::new();
    for panel in layout.panels() {
        let p = || panel.clone();
        let some = Some(panel);
        names.insert(area.with(("tab", panel)), Hit::Tab(p()));
        names.insert(area.with(("icon", panel)), Hit::Icon(p()));
        names.insert(area.with(("overflow", panel)), Hit::Overflow(p()));
        names.insert(area.with(("collapse", panel)), Hit::StackToggle(p()));
        names.insert(area.with(("stack-resize", panel)), Hit::Divider(Divider::Stack(p())));
        names.insert(area.with(("drag-bar", some)), Hit::DragBar(p()));
        names.insert(area.with(("drag-border", some)), Hit::Border(p()));
        names.insert(area.with(("icon-grip", some)), Hit::IconGrip(p()));
        names.insert(area.with(("column-bar", some)), Hit::ColumnBar(p()));
        names.insert(area.with(("column-collapse", some)), Hit::Collapse(p()));
        names.insert(area.with(("column-resize", some)), Hit::Divider(Divider::Column(p())));
        names.insert(area.with(("group-resize", some)), Hit::Divider(Divider::Group(p())));
        names.insert(area.with(("floating-divider", some)), Hit::Divider(Divider::Group(p())));
        let title = area.with(("move-group", some));
        names.insert(title, Hit::Title(p()));
        names.insert(title.with("close"), Hit::Close(p()));
        let window = area.with(("floating", panel));
        let edges = [ResizeEdge::BottomRight, ResizeEdge::Left, ResizeEdge::Right, ResizeEdge::Bottom, ResizeEdge::BottomLeft];
        for (name, edge) in RESIZE_HANDLES.iter().zip(edges) {
            names.insert(window.with(*name), Hit::Resize(p(), edge));
        }
    }
    let mut nodes = layout.root.as_ref().map(|root| vec![(root, Vec::<bool>::new())]).unwrap_or_default();
    while let Some((node, path)) = nodes.pop() {
        if let Node::Split { first, second, .. } = node {
            names.insert(area.with(("split", &path)), Hit::Divider(Divider::Split(path.clone())));
            let mut second_path = path.clone();
            second_path.push(true);
            let mut first_path = path;
            first_path.push(false);
            nodes.push((second, second_path));
            nodes.push((first, first_path));
        }
    }
    let hits: Vec<(Hit<P>, Rect)> =
        owned.iter().filter_map(|id| Some((names.remove(id)?, ctx.read_response(*id)?.rect))).filter(|(_, rect)| rect.is_positive()).collect();
    ctx.data_mut(|d| d.insert_temp(key(area), hits));
}
