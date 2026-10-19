//! Frame profile: Maximize Frame, the outer edge band, hidden panels and the frame menu items.

use std::{borrow::Cow, collections::HashMap, fmt::Debug, hash::Hash};

use egui::{Context, Pos2, Rect, pos2, vec2};

use super::{GroupKind, Renderer, floating::StackHit, group::TabGroup};
use crate::docking::{
    Action, Column, DockContent, DockEvent, Floating, Group, HidePanels, Layout, Node, Permissions, Profile, Zone,
    dock_model::{Place, node_at},
};

/// What the renderer draws when it is not the layout itself, and the targets that depend on it.
pub(in crate::docking) struct FrameState<P> {
    /// View path to layout path of each split, when the view is not the layout.
    paths: Option<HashMap<Vec<bool>, Vec<bool>>>,
    /// The view node whose rect is the outer edge area; none while a frame is maximized.
    edge_path: Option<Vec<bool>>,
    pub(super) edge: Option<Rect>,
    pub(super) maximized: Option<P>,
    /// Floating stacks as drawn, back to front.
    pub(super) stacks: Vec<StackHit<P>>,
}

/// The layout as shown: a maximized frame alone, and hidden panels left out.
pub(in crate::docking) fn view<'a, P: Clone + Eq + Hash>(
    layout: &'a Layout<P>,
    maximized: Option<&P>,
    hide: HidePanels<'_, P>,
    content: &dyn DockContent<P>,
) -> (Cow<'a, Layout<P>>, FrameState<P>) {
    let protected = |p: &P| content.permissions(p) == Permissions::PROTECTED;
    let hidden = |p: &P| match hide {
        HidePanels::None => false,
        HidePanels::All => !protected(p),
        HidePanels::Except(keep) => !protected(p) && !keep(p),
    };
    // Only a docked panel maximizes its frame.
    let maximized = maximized.and_then(|panel| match layout.place(panel)? {
        Place::Node(path) => Some((panel.clone(), path.clone(), layout.root.as_ref().and_then(|root| node_at(root, &path))?.clone())),
        place @ Place::Dock { .. } => {
            let group = layout.group(&place)?;
            Some((panel.clone(), path_of(&place), Node::Tabs { panels: group.panels.clone(), active: group.active }))
        }
        Place::Floating { .. } => None,
    });
    let mut state = FrameState { paths: None, edge_path: None, edge: None, maximized: None, stacks: Vec::new() };
    if maximized.is_none() && matches!(hide, HidePanels::None) {
        state.edge_path = Some(layout.edge_path());
        return (Cow::Borrowed(layout), state);
    }
    let mut paths = HashMap::new();
    let view = match maximized {
        Some((panel, path, node)) => {
            state.maximized = Some(panel);
            Layout { root: prune(&node, path, Vec::new(), &hidden, &mut paths), floating: Vec::new() }
        }
        None => {
            let root = layout.root.as_ref().and_then(|root| prune(root, Vec::new(), Vec::new(), &hidden, &mut paths));
            let floating = layout
                .floating
                .iter()
                .filter_map(|window| {
                    let groups: Vec<_> = window.groups.iter().filter_map(|g| prune_group(g, &hidden)).collect();
                    (!groups.is_empty()).then(|| Floating { groups, rect: window.rect, native: window.native })
                })
                .collect();
            Layout { root, floating }
        }
    };
    if state.maximized.is_none() {
        state.edge_path = Some(view.edge_path());
    }
    state.paths = Some(paths);
    (Cow::Owned(view), state)
}

fn path_of(place: &Place) -> Vec<bool> {
    match place {
        Place::Node(path) | Place::Dock { path, .. } => path.clone(),
        Place::Floating { .. } => Vec::new(),
    }
}

fn visible<P>(node: &Node<P>, hidden: &dyn Fn(&P) -> bool) -> bool {
    match node {
        Node::Split { first, second, .. } => visible(first, hidden) || visible(second, hidden),
        Node::Tabs { panels, .. } => panels.iter().any(|p| !hidden(p)),
        Node::Stack { entries } => entries.iter().any(|e| !hidden(&e.panel)),
        Node::Dock { columns, .. } => columns.iter().flat_map(|c| &c.groups).flat_map(|g| &g.panels).any(|p| !hidden(p)),
    }
}

/// `node` without hidden panels; a split with one visible side becomes that side. Records the
/// layout path of every split kept.
fn prune<P: Clone + PartialEq>(
    node: &Node<P>,
    path: Vec<bool>,
    at: Vec<bool>,
    hidden: &dyn Fn(&P) -> bool,
    paths: &mut HashMap<Vec<bool>, Vec<bool>>,
) -> Option<Node<P>> {
    let child = |path: &[bool], side: bool| [path, &[side]].concat();
    match node {
        Node::Split { axis, size, first, second } => match (visible(first, hidden), visible(second, hidden)) {
            (true, true) => {
                let first = prune(first, child(&path, false), child(&at, false), hidden, paths)?;
                let second = prune(second, child(&path, true), child(&at, true), hidden, paths)?;
                paths.insert(at, path);
                Some(Node::Split { axis: *axis, size: *size, first: Box::new(first), second: Box::new(second) })
            }
            (true, false) => prune(first, child(&path, false), at, hidden, paths),
            (false, true) => prune(second, child(&path, true), at, hidden, paths),
            (false, false) => None,
        },
        Node::Tabs { panels, active } => {
            let group = prune_group(&Group::new(panels.clone(), *active), hidden)?;
            Some(Node::Tabs { panels: group.panels, active: group.active })
        }
        Node::Stack { entries } => {
            let entries: Vec<_> = entries.iter().filter(|e| !hidden(&e.panel)).cloned().collect();
            (!entries.is_empty()).then_some(Node::Stack { entries })
        }
        Node::Dock { side, columns } => {
            let columns: Vec<_> = columns
                .iter()
                .filter_map(|c| {
                    let groups: Vec<_> = c.groups.iter().filter_map(|g| prune_group(g, hidden)).collect();
                    (!groups.is_empty()).then(|| Column { groups, ..c.clone() })
                })
                .collect();
            (!columns.is_empty()).then_some(Node::Dock { side: *side, columns })
        }
    }
}

/// A group without hidden panels; the selected panel stays selected when it is shown.
fn prune_group<P: Clone + PartialEq>(group: &Group<P>, hidden: &dyn Fn(&P) -> bool) -> Option<Group<P>> {
    let selected = group.active_panel();
    let panels: Vec<P> = group.panels.iter().filter(|p| !hidden(p)).cloned().collect();
    let active = selected.and_then(|s| panels.iter().position(|p| p == s)).unwrap_or(0);
    (!panels.is_empty()).then(|| Group { panels, active, ..group.clone() })
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(super) fn note_edge(&mut self, path: &[bool], rect: Rect) {
        if self.frame.edge_path.as_deref() == Some(path) {
            self.frame.edge = Some(rect);
        }
    }

    /// The layout path of a split drawn at view path `path`.
    pub(super) fn original_path(&self, path: &[bool]) -> Vec<bool> {
        self.frame.paths.as_ref().and_then(|paths| paths.get(path).cloned()).unwrap_or_else(|| path.to_vec())
    }

    /// The outer edge band under the pointer (P8): top and bottom in both profiles, left and
    /// right in the frame profile. Returns a docked anchor that is not moving, the zone and
    /// the preview of the new frame.
    pub(super) fn edge_proposal(&self, members: &[P], pointer: Pos2) -> Option<(P, Zone, Rect)> {
        let area = self.frame.edge.filter(|area| area.contains(pointer))?;
        let band = self.style.edge_band.min(area.width() / 4.0).min(area.height() / 4.0);
        let mut edges = vec![(pointer.y - area.top(), Zone::Top), (area.bottom() - pointer.y, Zone::Bottom)];
        if self.style.profile == Profile::Frame {
            edges.extend([(pointer.x - area.left(), Zone::Left), (area.right() - pointer.x, Zone::Right)]);
        }
        let (distance, zone) = edges.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))?;
        if distance > band {
            return None;
        }
        let anchor = self
            .layout
            .panels()
            .into_iter()
            .find(|p| !members.contains(p) && !matches!(self.layout.place(p), Some(Place::Floating { .. }) | None))?
            .clone();
        let size = area.size() * 0.25;
        let preview = match zone {
            Zone::Left => Rect::from_min_size(area.min, vec2(size.x, area.height())),
            Zone::Right => Rect::from_min_max(pos2(area.right() - size.x, area.top()), area.max),
            Zone::Top => Rect::from_min_size(area.min, vec2(area.width(), size.y)),
            Zone::Bottom | Zone::Center => Rect::from_min_max(pos2(area.left(), area.bottom() - size.y), area.max),
        };
        Some((anchor, zone, preview))
    }

    pub(super) fn float_label(&self) -> &str {
        if self.style.profile == Profile::Frame { &self.style.undock_label } else { &self.style.float_label }
    }

    pub(super) fn float_group_label(&self) -> &str {
        if self.style.profile == Profile::Frame { &self.style.undock_group_label } else { &self.style.float_group_label }
    }

    pub(super) fn close_group_label(&self) -> &str {
        if self.style.profile == Profile::Frame { &self.style.close_panel_group_label } else { &self.style.close_group_label }
    }

    /// Ctrl/Cmd held when a frame-profile tab drag starts: the drop floats (S9).
    pub(super) fn latch_undock(&self, ctx: &Context, kind: Option<GroupKind>) {
        let command = ctx.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.mac_cmd);
        if self.style.profile == Profile::Frame && kind == Some(GroupKind::Frame) && command {
            ctx.data_mut(|d| {
                if let Some(drag) = d.get_temp::<crate::docking::Drag<P>>(self.id.with("drag")) {
                    d.insert_temp(self.id.with("drag"), crate::docking::Drag { undock: true, ..drag });
                }
            });
        }
    }

    /// Frame items of the menu tail: Close Other Panels in Group and Maximize Frame (frame
    /// profile), and Move Panel to New Window where native windows are available.
    pub(super) fn frame_items(&mut self, ui: &mut egui::Ui, owner: &TabGroup<'_, P>, panel: &P) {
        if self.style.profile == Profile::Frame {
            let others = owner.panels.iter().filter(|p| *p != panel).all(|p| self.permissions(p).closable);
            if owner.panels.len() > 1 && others && self.menu_action(ui, &self.style.close_others_label.clone()) {
                self.output.actions.push(Action::CloseOthers { panel: panel.clone() });
            }
            if owner.kind == GroupKind::Frame {
                let restore = self.frame.maximized.as_ref().is_some_and(|m| owner.panels.contains(m));
                let label = if restore { self.style.restore_frame_label.clone() } else { self.style.maximize_label.clone() };
                if self.menu_action(ui, &label) {
                    let panel = if restore { self.frame.maximized.clone().unwrap_or_else(|| panel.clone()) } else { panel.clone() };
                    self.output.events.push(DockEvent::ToggleMaximize { panel });
                }
            }
        }
        #[cfg(feature = "native-windows")]
        self.new_window_item(ui, owner, panel);
    }
}
