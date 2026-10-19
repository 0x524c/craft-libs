//! Docks, columns, groups and floating stacks: their types and the structural edits behind
//! group, column and window moves.
//!
//! A group, column or floating window is named by a panel it contains. Edits run on a proposed
//! copy of the layout, so a helper that fails part-way leaves nothing behind.

use std::hash::Hash;

use super::model::{DockError, Floating, Layout, MAX_DEPTH, MAX_NODES, Node, Placement, Zone, valid_rect, valid_width};
use crate::layout::{SplitAxis, SplitSize};

/// Width of a new column when nothing better is known.
pub(super) const DEFAULT_COLUMN_WIDTH: f32 = 240.0;
/// Rect of a new floating window when the caller gives none and the source has no size.
const DEFAULT_FLOAT: [f32; 4] = [48.0, 48.0, 260.0, 320.0];

/// The window edge a [`Node::Dock`] sits on, in reading order so right-to-left interfaces
/// mirror. `Start` is the edge of a horizontal split's first child (left in a left-to-right
/// layout); the renderer maps it to a physical edge from the layout direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Side {
    Start,
    End,
}

/// Tabs that move, minimize and size together, in a dock column or a floating stack.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(bound(deserialize = "P: serde::Deserialize<'de>")))]
pub struct Group<P> {
    pub panels: Vec<P>,
    pub active: usize,
    /// Only the tab strip shows. `height` is kept for when the group expands again.
    #[cfg_attr(feature = "serde", serde(default))]
    pub minimized: bool,
    /// Preferred height in points; `None` shares the free height of the column or stack.
    #[cfg_attr(feature = "serde", serde(default))]
    pub height: Option<f32>,
}

impl<P> Group<P> {
    pub fn new(panels: Vec<P>, active: usize) -> Self {
        Self { panels, active, minimized: false, height: None }
    }

    pub fn active_panel(&self) -> Option<&P> {
        self.panels.get(self.active)
    }
}

/// One column of a dock: groups from top to bottom.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(bound(deserialize = "P: serde::Deserialize<'de>")))]
pub struct Column<P> {
    pub groups: Vec<Group<P>>,
    /// Expanded width in points.
    pub width: f32,
    /// Collapsed to one icon per panel.
    #[cfg_attr(feature = "serde", serde(default))]
    pub iconic: bool,
    /// Width while iconic; labels show beside the icons when it is wide enough.
    #[cfg_attr(feature = "serde", serde(default))]
    pub iconic_width: Option<f32>,
}

impl<P> Column<P> {
    pub fn new(groups: Vec<Group<P>>, width: f32) -> Self {
        Self { groups, width, iconic: false, iconic_width: None }
    }
}

/// Where a group, column or floating window goes.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Target<P> {
    /// Relative to the panel `anchor`, which must not be part of what moves.
    At { anchor: P, placement: Placement<P> },
    /// A new dock on an edge that has none (or an empty one). The root becomes the other side
    /// of a horizontal split.
    NewDock { side: Side },
    /// A new floating window. `None` keeps the source's size at a default position.
    Float { rect: Option<[f32; 4]> },
}

/// What the Window menu shows for a panel. The model does not track the z-order of floating
/// windows, so `Front` means "showing": the active tab of an expanded group in an expanded column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelState {
    Hidden,
    Open,
    Front,
}

/// The group a closed panel was in, when that group was in a dock or a floating stack.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupLocation {
    pub minimized: bool,
    pub height: Option<f32>,
    /// The column, when the group was docked.
    pub column: Option<ColumnLocation>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColumnLocation {
    pub side: Side,
    pub width: f32,
    pub iconic: bool,
    pub iconic_width: Option<f32>,
}

/// Where a panel is. Paths lead from the root: false is a split's first child, true its second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// A `Tabs` or `Stack` node.
    Node(Vec<bool>),
    Dock {
        path: Vec<bool>,
        column: usize,
        group: usize,
    },
    Floating {
        window: usize,
        group: usize,
    },
}

/// Which container a whole-container action names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit {
    Group,
    Column,
    Window,
}

/// Groups in transit, with the size and state of the container they came from.
pub(super) struct Moving<P> {
    pub groups: Vec<Group<P>>,
    pub width: Option<f32>,
    pub iconic: bool,
    pub iconic_width: Option<f32>,
    pub rect: Option<[f32; 4]>,
    pub native: Option<[f32; 4]>,
}

impl<P> Moving<P> {
    pub fn group(group: Group<P>) -> Self {
        Self { groups: vec![group], width: None, iconic: false, iconic_width: None, rect: None, native: None }
    }

    fn into_column(self, width: f32) -> Column<P> {
        Column { groups: self.groups, width: self.width.unwrap_or(width), iconic: self.iconic, iconic_width: self.iconic_width }
    }
}

pub(super) fn node_at<'a, P>(mut node: &'a Node<P>, path: &[bool]) -> Option<&'a Node<P>> {
    for second_child in path {
        let Node::Split { first, second, .. } = node else { return None };
        node = if *second_child { second } else { first };
    }
    Some(node)
}

pub(super) fn node_at_mut<'a, P>(mut node: &'a mut Node<P>, path: &[bool]) -> Option<&'a mut Node<P>> {
    for second_child in path {
        let Node::Split { first, second, .. } = node else { return None };
        node = if *second_child { second } else { first };
    }
    Some(node)
}

/// Insert `new` at `index` and select the member at `index + selected`.
fn splice<P>(panels: &mut Vec<P>, active: &mut usize, index: usize, new: Vec<P>, selected: usize) {
    let index = index.min(panels.len());
    let count = new.len();
    panels.splice(index..index, new);
    *active = index + selected.min(count.saturating_sub(1));
}

impl<P: Clone + Eq + Hash> Layout<P> {
    pub(super) fn place(&self, panel: &P) -> Option<Place> {
        for (w, window) in self.floating.iter().enumerate() {
            if let Some(g) = window.groups.iter().position(|group| group.panels.contains(panel)) {
                return Some(Place::Floating { window: w, group: g });
            }
        }
        let mut visit: Vec<(&Node<P>, Vec<bool>)> = self.root.iter().map(|node| (node, Vec::new())).collect();
        let mut visited = 0usize;
        while let Some((node, path)) = visit.pop() {
            visited = visited.saturating_add(1);
            if visited > MAX_NODES {
                return None;
            }
            match node {
                Node::Split { first, second, .. } => {
                    let mut second_path = path.clone();
                    second_path.push(true);
                    let mut first_path = path;
                    first_path.push(false);
                    visit.push((second, second_path));
                    visit.push((first, first_path));
                }
                Node::Tabs { panels, .. } if panels.contains(panel) => return Some(Place::Node(path)),
                Node::Stack { entries } if entries.iter().any(|e| &e.panel == panel) => return Some(Place::Node(path)),
                Node::Dock { columns, .. } => {
                    for (c, column) in columns.iter().enumerate() {
                        if let Some(g) = column.groups.iter().position(|group| group.panels.contains(panel)) {
                            return Some(Place::Dock { path, column: c, group: g });
                        }
                    }
                }
                Node::Tabs { .. } | Node::Stack { .. } => {}
            }
        }
        None
    }

    pub(super) fn dock_path(&self, side: Side) -> Option<Vec<bool>> {
        let mut visit: Vec<(&Node<P>, Vec<bool>)> = self.root.iter().map(|node| (node, Vec::new())).collect();
        let mut visited = 0usize;
        while let Some((node, path)) = visit.pop() {
            visited = visited.saturating_add(1);
            if visited > MAX_NODES {
                return None;
            }
            match node {
                Node::Split { first, second, .. } => {
                    let mut second_path = path.clone();
                    second_path.push(true);
                    let mut first_path = path;
                    first_path.push(false);
                    visit.push((second, second_path));
                    visit.push((first, first_path));
                }
                Node::Dock { side: s, .. } if *s == side => return Some(path),
                _ => {}
            }
        }
        None
    }

    /// The node an edge split wraps: the root, or the part between the docks.
    pub(in crate::docking) fn edge_path(&self) -> Vec<bool> {
        let mut path = Vec::new();
        let mut node = self.root.as_ref();
        while let Some(Node::Split { axis: SplitAxis::Horizontal, first, second, .. }) = node {
            if path.len() > MAX_DEPTH {
                break;
            }
            let second_side = match (&**first, &**second) {
                (Node::Dock { .. }, _) => true,
                (_, Node::Dock { .. }) => false,
                _ => break,
            };
            path.push(second_side);
            node = Some(if second_side { second } else { first });
        }
        path
    }

    pub(super) fn columns_mut(&mut self, path: &[bool]) -> Option<&mut Vec<Column<P>>> {
        match self.root.as_mut().and_then(|root| node_at_mut(root, path)) {
            Some(Node::Dock { columns, .. }) => Some(columns),
            _ => None,
        }
    }

    pub(super) fn column(&self, place: &Place) -> Option<&Column<P>> {
        let Place::Dock { path, column, .. } = place else { return None };
        match self.root.as_ref().and_then(|root| node_at(root, path)) {
            Some(Node::Dock { columns, .. }) => columns.get(*column),
            _ => None,
        }
    }

    pub(super) fn column_mut(&mut self, place: &Place) -> Option<&mut Column<P>> {
        let Place::Dock { path, column, .. } = place else { return None };
        self.columns_mut(path)?.get_mut(*column)
    }

    /// The dock or floating group at `place`; `None` for a `Tabs` or `Stack` node.
    pub(super) fn group(&self, place: &Place) -> Option<&Group<P>> {
        match place {
            Place::Node(_) => None,
            Place::Dock { group, .. } => self.column(place)?.groups.get(*group),
            Place::Floating { window, group } => self.floating.get(*window)?.groups.get(*group),
        }
    }

    pub(super) fn group_mut(&mut self, place: &Place) -> Option<&mut Group<P>> {
        match place {
            Place::Node(_) => None,
            Place::Dock { group, .. } => self.column_mut(place)?.groups.get_mut(*group),
            Place::Floating { window, group } => self.floating.get_mut(*window)?.groups.get_mut(*group),
        }
    }

    /// Remove the node at `path`; its parent split collapses into the sibling.
    pub(super) fn take_node(&mut self, path: &[bool]) -> Option<Node<P>> {
        let Some((second_child, parent_path)) = path.split_last() else { return self.root.take() };
        let parent = self.root.as_mut().and_then(|root| node_at_mut(root, parent_path))?;
        if !matches!(parent, Node::Split { .. }) {
            return None;
        }
        match std::mem::replace(parent, Node::Tabs { panels: Vec::new(), active: 0 }) {
            Node::Split { first, second, .. } => {
                let (taken, kept) = if *second_child { (second, first) } else { (first, second) };
                *parent = *kept;
                Some(*taken)
            }
            other => {
                *parent = other;
                None
            }
        }
    }

    /// The members of the group, column or floating window containing `panel`, in order.
    /// An accordion entry is a group of one.
    pub(super) fn members(&self, panel: &P, unit: Unit) -> Result<Vec<P>, DockError> {
        let place = self.place(panel).ok_or(DockError::MissingPanel)?;
        match (unit, &place) {
            (Unit::Group, Place::Node(path)) => match self.root.as_ref().and_then(|root| node_at(root, path)) {
                Some(Node::Tabs { panels, .. }) => Ok(panels.clone()),
                Some(Node::Stack { .. }) => Ok(vec![panel.clone()]),
                _ => Err(DockError::MissingPanel),
            },
            (Unit::Group, _) => self.group(&place).map(|g| g.panels.clone()).ok_or(DockError::MissingPanel),
            (Unit::Column, Place::Dock { .. }) => {
                let column = self.column(&place).ok_or(DockError::MissingPanel)?;
                Ok(column.groups.iter().flat_map(|g| g.panels.iter().cloned()).collect())
            }
            (Unit::Window, Place::Floating { window, .. }) => {
                let window = self.floating.get(*window).ok_or(DockError::MissingPanel)?;
                Ok(window.panels().cloned().collect())
            }
            (Unit::Column | Unit::Window, _) => Err(DockError::InvalidPlacement),
        }
    }

    pub(super) fn take_unit(&mut self, panel: &P, unit: Unit) -> Result<Moving<P>, DockError> {
        match unit {
            Unit::Group => self.take_group(panel),
            Unit::Column => self.take_column(panel),
            Unit::Window => self.take_window(panel),
        }
    }

    fn take_group(&mut self, panel: &P) -> Result<Moving<P>, DockError> {
        let place = self.place(panel).ok_or(DockError::MissingPanel)?;
        match place {
            Place::Node(path) => {
                if let Some(Node::Stack { entries }) = self.root.as_mut().and_then(|root| node_at_mut(root, &path))
                    && entries.len() > 1
                    && let Some(index) = entries.iter().position(|e| &e.panel == panel)
                {
                    let entry = entries.remove(index);
                    let group = Group { panels: vec![entry.panel], active: 0, minimized: !entry.open, height: entry.height };
                    return Ok(Moving::group(group));
                }
                match self.take_node(&path) {
                    Some(Node::Tabs { panels, active }) => Ok(Moving::group(Group::new(panels, active))),
                    Some(Node::Stack { mut entries }) if entries.len() == 1 => {
                        let entry = entries.remove(0);
                        Ok(Moving::group(Group { panels: vec![entry.panel], active: 0, minimized: !entry.open, height: entry.height }))
                    }
                    _ => Err(DockError::MissingPanel),
                }
            }
            Place::Dock { path, column, group } => {
                let columns = self.columns_mut(&path).ok_or(DockError::MissingPanel)?;
                let source = columns.get_mut(column).ok_or(DockError::MissingPanel)?;
                if group >= source.groups.len() {
                    return Err(DockError::MissingPanel);
                }
                let taken = source.groups.remove(group);
                let width = source.width;
                if source.groups.is_empty() {
                    columns.remove(column);
                }
                if columns.is_empty() {
                    self.take_node(&path);
                }
                Ok(Moving { width: Some(width), ..Moving::group(taken) })
            }
            Place::Floating { window, group } => {
                let source = self.floating.get_mut(window).ok_or(DockError::MissingPanel)?;
                if group >= source.groups.len() {
                    return Err(DockError::MissingPanel);
                }
                let taken = source.groups.remove(group);
                let rect = source.rect;
                let native = source.native;
                let whole = source.groups.is_empty();
                if whole {
                    self.floating.remove(window);
                }
                let width = rect.get(2).copied();
                Ok(Moving { width, rect: whole.then_some(rect), native: native.filter(|_| whole), ..Moving::group(taken) })
            }
        }
    }

    fn take_column(&mut self, panel: &P) -> Result<Moving<P>, DockError> {
        let Some(Place::Dock { path, column, .. }) = self.place(panel) else { return Err(DockError::InvalidPlacement) };
        let columns = self.columns_mut(&path).ok_or(DockError::MissingPanel)?;
        if column >= columns.len() {
            return Err(DockError::MissingPanel);
        }
        let taken = columns.remove(column);
        if columns.is_empty() {
            self.take_node(&path);
        }
        Ok(Moving {
            groups: taken.groups,
            width: Some(taken.width),
            iconic: taken.iconic,
            iconic_width: taken.iconic_width,
            rect: None,
            native: None,
        })
    }

    fn take_window(&mut self, panel: &P) -> Result<Moving<P>, DockError> {
        let Some(Place::Floating { window, .. }) = self.place(panel) else { return Err(DockError::InvalidPlacement) };
        if window >= self.floating.len() {
            return Err(DockError::MissingPanel);
        }
        let taken = self.floating.remove(window);
        Ok(Moving {
            groups: taken.groups,
            width: taken.rect.get(2).copied(),
            iconic: false,
            iconic_width: None,
            rect: Some(taken.rect),
            native: taken.native,
        })
    }

    /// Insert groups taken by [`Self::take_unit`] (or a new single-panel group).
    pub(super) fn insert_moving(&mut self, moving: Moving<P>, to: Target<P>) -> Result<(), DockError> {
        if moving.groups.is_empty() || moving.groups.iter().any(|g| g.panels.is_empty()) {
            return Err(DockError::MissingPanel);
        }
        match to {
            Target::Float { rect } => {
                let native = if rect.is_none() { moving.native } else { None };
                let rect = rect.or(moving.rect).unwrap_or_else(|| {
                    let [x, y, width, height] = DEFAULT_FLOAT;
                    [x, y, moving.width.unwrap_or(width), height]
                });
                if !valid_rect(rect) {
                    return Err(DockError::InvalidGeometry);
                }
                self.floating.push(Floating { groups: moving.groups, rect, native });
                Ok(())
            }
            Target::NewDock { side } => {
                if let Some(path) = self.dock_path(side) {
                    let columns = self.columns_mut(&path).ok_or(DockError::MissingTarget)?;
                    if !columns.is_empty() {
                        return Err(DockError::InvalidPlacement);
                    }
                    columns.push(moving.into_column(DEFAULT_COLUMN_WIDTH));
                    return Ok(());
                }
                let column = moving.into_column(DEFAULT_COLUMN_WIDTH);
                self.add_dock(side, column);
                Ok(())
            }
            Target::At { anchor, placement } => {
                let place = self.place(&anchor).ok_or(DockError::MissingTarget)?;
                match placement {
                    Placement::Tab { before } => {
                        let index = match &before {
                            Some(before) => {
                                if self.place(before).as_ref() != Some(&place) {
                                    return Err(DockError::MissingTarget);
                                }
                                self.index_in(&place, before)
                            }
                            None => None,
                        };
                        self.merge(&place, index, moving)
                    }
                    Placement::Split(Zone::Center) => {
                        let index = self.index_in(&place, &anchor).map(|i| i + 1);
                        self.merge(&place, index, moving)
                    }
                    Placement::Split(zone) => {
                        let Place::Node(path) = place else { return Err(DockError::InvalidPlacement) };
                        let node = self.root.as_mut().and_then(|root| node_at_mut(root, &path)).ok_or(DockError::MissingTarget)?;
                        let (panels, active) = flatten(moving.groups);
                        let new = Node::Tabs { panels, active };
                        let old = std::mem::replace(node, Node::Tabs { panels: Vec::new(), active: 0 });
                        let axis = if matches!(zone, Zone::Top | Zone::Bottom) { SplitAxis::Vertical } else { SplitAxis::Horizontal };
                        let (first, second) = if matches!(zone, Zone::Top | Zone::Left) { (new, old) } else { (old, new) };
                        *node = Node::Split { axis, size: SplitSize::Ratio(0.5), first: Box::new(first), second: Box::new(second) };
                        Ok(())
                    }
                    Placement::Group { below } => {
                        let (groups, index) = match &place {
                            Place::Dock { group, .. } => (&mut self.column_mut(&place).ok_or(DockError::MissingTarget)?.groups, *group),
                            Place::Floating { window, group } => {
                                (&mut self.floating.get_mut(*window).ok_or(DockError::MissingTarget)?.groups, *group)
                            }
                            Place::Node(_) => return Err(DockError::InvalidPlacement),
                        };
                        let index = (index + usize::from(below)).min(groups.len());
                        groups.splice(index..index, moving.groups);
                        Ok(())
                    }
                    Placement::Edge(zone) => {
                        if zone == Zone::Center || matches!(place, Place::Floating { .. }) {
                            return Err(DockError::InvalidPlacement);
                        }
                        let path = self.edge_path();
                        let node = self.root.as_mut().and_then(|root| node_at_mut(root, &path)).ok_or(DockError::MissingTarget)?;
                        if matches!(node, Node::Dock { .. }) {
                            return Err(DockError::InvalidPlacement);
                        }
                        let (panels, active) = flatten(moving.groups);
                        let new = Node::Tabs { panels, active };
                        let old = std::mem::replace(node, Node::Tabs { panels: Vec::new(), active: 0 });
                        let axis = if matches!(zone, Zone::Top | Zone::Bottom) { SplitAxis::Vertical } else { SplitAxis::Horizontal };
                        let (first, second, size) = if matches!(zone, Zone::Top | Zone::Left) { (new, old, 0.25) } else { (old, new, 0.75) };
                        *node = Node::Split { axis, size: SplitSize::Ratio(size), first: Box::new(first), second: Box::new(second) };
                        Ok(())
                    }
                    Placement::Column { after } => {
                        let Place::Dock { path, column, .. } = &place else { return Err(DockError::InvalidPlacement) };
                        let columns = self.columns_mut(path).ok_or(DockError::MissingTarget)?;
                        let width = columns.get(*column).map_or(DEFAULT_COLUMN_WIDTH, |c| c.width);
                        let index = (column + usize::from(after)).min(columns.len());
                        columns.insert(index, moving.into_column(width));
                        Ok(())
                    }
                }
            }
        }
    }

    /// Wrap the root in a horizontal split with a new dock on `side`. The split's fixed size is
    /// the column width.
    pub(super) fn add_dock(&mut self, side: Side, column: Column<P>) {
        let width = column.width;
        let dock = Node::Dock { side, columns: vec![column] };
        self.root = Some(match self.root.take() {
            None => dock,
            Some(root) => {
                let (first, second, size) = match side {
                    Side::Start => (dock, root, SplitSize::FixedFirst(width)),
                    Side::End => (root, dock, SplitSize::FixedSecond(width)),
                };
                Node::Split { axis: SplitAxis::Horizontal, size, first: Box::new(first), second: Box::new(second) }
            }
        });
    }

    fn index_in(&self, place: &Place, panel: &P) -> Option<usize> {
        match place {
            Place::Node(path) => match self.root.as_ref().and_then(|root| node_at(root, path))? {
                Node::Tabs { panels, .. } => panels.iter().position(|p| p == panel),
                Node::Stack { entries } => entries.iter().position(|e| &e.panel == panel),
                _ => None,
            },
            _ => self.group(place)?.panels.iter().position(|p| p == panel),
        }
    }

    /// Join every moving panel to the group at `place`, before `index` or at the end. The first
    /// moving group's active panel becomes active.
    fn merge(&mut self, place: &Place, index: Option<usize>, moving: Moving<P>) -> Result<(), DockError> {
        let (panels, selected) = flatten(moving.groups);
        match place {
            Place::Node(path) => match self.root.as_mut().and_then(|root| node_at_mut(root, path)) {
                Some(Node::Tabs { panels: target, active }) => {
                    splice(target, active, index.unwrap_or(target.len()), panels, selected);
                    Ok(())
                }
                Some(Node::Stack { entries }) => {
                    let index = index.unwrap_or(entries.len()).min(entries.len());
                    entries.splice(index..index, panels.into_iter().map(|panel| super::StackEntry { panel, open: true, height: None }));
                    Ok(())
                }
                _ => Err(DockError::MissingTarget),
            },
            _ => {
                let group = self.group_mut(place).ok_or(DockError::MissingTarget)?;
                let end = group.panels.len();
                splice(&mut group.panels, &mut group.active, index.unwrap_or(end), panels, selected);
                Ok(())
            }
        }
    }
}

/// All panels in order, and the index of the first group's active panel among them.
fn flatten<P>(groups: Vec<Group<P>>) -> (Vec<P>, usize) {
    let selected = groups.first().map_or(0, |g| g.active.min(g.panels.len().saturating_sub(1)));
    (groups.into_iter().flat_map(|g| g.panels).collect(), selected)
}

pub(super) fn valid_column<P>(column: &Column<P>) -> bool {
    valid_width(column.width) && column.iconic_width.is_none_or(valid_width)
}
