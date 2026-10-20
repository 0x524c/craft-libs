//! Where each docking gesture starts, for control protocols and tests.

use std::{collections::HashMap, fmt::Debug, hash::Hash};

use egui::{Context, Id, LayerId, Order, Rect};

use super::{DockArea, Floating, Layout, Node, render::RESIZE_HANDLES};

/// A rendered docking control that starts a gesture. Groups, columns and windows are named by
/// their first panel.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Hit<P> {
    /// A tab: click activates, drag moves the panel (S1).
    Tab(P),
    /// A group's drag bar: drag moves the group; a right click, Enter or Space opens the group
    /// menu (S2).
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
    /// drag from their centers.
    pub fn hit_rects(ctx: &Context, id: Id) -> Vec<(Hit<P>, Rect)> {
        ctx.data(|d| d.get_temp::<Vec<(Hit<P>, Rect)>>(key(id))).unwrap_or_default()
    }

    /// The egui widget id of the control `hit` names in the area `id`, for tests and
    /// automation that focus a control or read its response.
    pub fn hit_id(id: Id, hit: &Hit<P>) -> Id {
        match hit {
            Hit::Tab(p) => id.with(("tab", p)),
            Hit::Icon(p) => id.with(("icon", p)),
            Hit::Overflow(p) => id.with(("overflow", p)),
            Hit::StackToggle(p) => id.with(("collapse", p)),
            Hit::Divider(Divider::Stack(p)) => id.with(("stack-resize", p)),
            Hit::DragBar(p) => id.with(("drag-bar", Some(p))),
            Hit::Border(p) => id.with(("drag-border", Some(p))),
            Hit::IconGrip(p) => id.with(("icon-grip", Some(p))),
            Hit::ColumnBar(p) => id.with(("column-bar", Some(p))),
            Hit::Collapse(p) => id.with(("column-collapse", Some(p))),
            Hit::Divider(Divider::Column(p)) => id.with(("column-resize", Some(p))),
            Hit::Divider(Divider::Group(p)) => id.with(("group-resize", Some(p))),
            Hit::Divider(Divider::Split(path)) => id.with(("split", path)),
            Hit::Title(p) => id.with(("move-group", Some(p))),
            Hit::Close(p) => id.with(("move-group", Some(p))).with("close"),
            Hit::Resize(p, edge) => {
                let name = match edge {
                    ResizeEdge::BottomRight => RESIZE_HANDLES[0],
                    ResizeEdge::Left => RESIZE_HANDLES[1],
                    ResizeEdge::Right => RESIZE_HANDLES[2],
                    ResizeEdge::Bottom => RESIZE_HANDLES[3],
                    ResizeEdge::BottomLeft => RESIZE_HANDLES[4],
                };
                id.with(("floating", p)).with(name)
            }
        }
    }

    /// The egui layer of a floating window in the area `id`; `None` for an empty window. Of two
    /// layers, the later in `ctx.memory(|m| m.layer_ids())` is drawn on top, and
    /// `ctx.memory(|m| m.area_rect(layer.id))` is the window's rect.
    pub fn floating_layer(id: Id, window: &Floating<P>) -> Option<LayerId> {
        window.panels().next().map(|first| LayerId::new(Order::Middle, id.with(("floating", first))))
    }
}

/// Name the controls the renderer registered this frame and store their rects.
pub(super) fn record<P: Clone + Eq + Hash + Debug + Send + Sync + 'static>(ctx: &Context, area: Id, layout: &Layout<P>, owned: &[Id]) {
    let mut names: HashMap<Id, Hit<P>> = HashMap::new();
    let mut name = |hit: Hit<P>| {
        names.insert(DockArea::<P>::hit_id(area, &hit), hit);
    };
    for panel in layout.panels() {
        let p = || panel.clone();
        for hit in [
            Hit::Tab(p()),
            Hit::Icon(p()),
            Hit::Overflow(p()),
            Hit::StackToggle(p()),
            Hit::Divider(Divider::Stack(p())),
            Hit::DragBar(p()),
            Hit::Border(p()),
            Hit::IconGrip(p()),
            Hit::ColumnBar(p()),
            Hit::Collapse(p()),
            Hit::Divider(Divider::Column(p())),
            Hit::Divider(Divider::Group(p())),
            Hit::Title(p()),
            Hit::Close(p()),
        ] {
            name(hit);
        }
        for edge in [ResizeEdge::BottomRight, ResizeEdge::Left, ResizeEdge::Right, ResizeEdge::Bottom, ResizeEdge::BottomLeft] {
            name(Hit::Resize(p(), edge));
        }
    }
    let mut nodes = layout.root.as_ref().map(|root| vec![(root, Vec::<bool>::new())]).unwrap_or_default();
    while let Some((node, path)) = nodes.pop() {
        if let Node::Split { first, second, .. } = node {
            name(Hit::Divider(Divider::Split(path.clone())));
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
