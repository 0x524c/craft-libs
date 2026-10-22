//! Split-tree geometry: where each split draws its children, how far a divider can travel,
//! and the sizes that move a divider while every group away from it keeps its size.

use egui::Rect;

use super::{dock::dock_rects, node_limits, split_bounds};
use crate::docking::{DockContent, DockStyle, Node, PanelLimits};
use crate::layout::{SplitAxis, SplitLayout, SplitSize, split_rect};

/// Room a group keeps above its minimum after a drop, so each of its dividers can still
/// travel both ways.
pub(in crate::docking) const DROP_MARGIN: f32 = 1.0;

pub(in crate::docking) struct Sizer<'a, P> {
    pub(in crate::docking) style: &'a DockStyle,
    pub(in crate::docking) content: &'a dyn DockContent<P>,
}

/// One group as drawn: its node, rect and limits.
pub(in crate::docking) struct Drawn<'a, P> {
    pub(in crate::docking) node: &'a Node<P>,
    pub(in crate::docking) rect: Rect,
    pub(in crate::docking) limits: PanelLimits,
}

fn extent(rect: Rect, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Horizontal => rect.width(),
        SplitAxis::Vertical => rect.height(),
    }
}

fn along(v: egui::Vec2, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Horizontal => v.x,
        SplitAxis::Vertical => v.y,
    }
}

impl<P> Sizer<'_, P> {
    fn limits(&self, node: &Node<P>) -> PanelLimits {
        node_limits(node, self.style, self.content)
    }

    /// The split as drawn: its stored size, clamped so both children fit their limits.
    pub(in crate::docking) fn split(&self, axis: SplitAxis, size: SplitSize, first: &Node<P>, second: &Node<P>, rect: Rect) -> Option<SplitLayout> {
        let wanted = split_rect(rect, axis, size, self.style.gap, 0.0)?;
        let (lo, hi) = split_bounds(wanted.available(), axis, self.limits(first), self.limits(second));
        split_rect(rect, axis, SplitSize::FixedFirst(wanted.first_size().clamp(lo, hi)), self.style.gap, 0.0)
    }

    /// The children of a split drawn in `rect`, with their rects; a dock beside its neighbour
    /// is drawn by the dock renderer.
    pub(in crate::docking) fn children<'n>(&self, node: &'n Node<P>, rect: Rect) -> Option<[(&'n Node<P>, Rect); 2]> {
        let Node::Split { axis, size, first, second } = node else { return None };
        match (&**first, &**second) {
            (Node::Dock { side, columns }, other) if *axis == SplitAxis::Horizontal => {
                let (dock, rest, _) = dock_rects(*side, columns, other, rect, self.style, self.content);
                Some([(first, dock), (second, rest)])
            }
            (other, Node::Dock { side, columns }) if *axis == SplitAxis::Horizontal => {
                let (dock, rest, _) = dock_rects(*side, columns, other, rect, self.style, self.content);
                Some([(first, rest), (second, dock)])
            }
            _ => {
                let split = self.split(*axis, *size, first, second, rect)?;
                Some([(first, split.first), (second, split.second)])
            }
        }
    }

    /// Every group of `node` drawn in `rect`; a dock counts as one.
    pub(in crate::docking) fn groups<'n>(&self, node: &'n Node<P>, rect: Rect, out: &mut Vec<Drawn<'n, P>>) {
        match self.children(node, rect) {
            Some(children) => children.into_iter().for_each(|(child, rect)| self.groups(child, rect, out)),
            None => out.push(Drawn { node, rect, limits: self.limits(node) }),
        }
    }

    /// How far the end edge (`end`) or start edge of `node` drawn in `rect` can move along
    /// `axis`: inward while the groups touching it stay at their minimums, and outward up to
    /// their maximums.
    pub(in crate::docking) fn travel(&self, node: &Node<P>, rect: Rect, axis: SplitAxis, end: bool) -> (f32, f32) {
        let docked =
            matches!(node, Node::Split { first, second, .. } if matches!(**first, Node::Dock { .. }) || matches!(**second, Node::Dock { .. }));
        match (node, self.children(node, rect)) {
            (Node::Split { first, .. }, Some([a, b])) if docked => {
                let (other, rect) = if matches!(**first, Node::Dock { .. }) { b } else { a };
                self.travel(other, rect, axis, end)
            }
            (Node::Split { axis: split_axis, .. }, Some([a, b])) if *split_axis == axis => {
                let (child, rect) = if end { b } else { a };
                self.travel(child, rect, axis, end)
            }
            (_, Some([a, b])) => {
                let (a, b) = (self.travel(a.0, a.1, axis, end), self.travel(b.0, b.1, axis, end));
                (a.0.min(b.0), a.1.min(b.1))
            }
            (_, None) => {
                let limits = self.limits(node);
                let size = extent(rect, axis);
                ((size - along(limits.min, axis)).max(0.0), (along(limits.max, axis) - size).max(0.0))
            }
        }
    }

    /// The sizes that move the end (`end`) or start edge of `node` drawn in `rect` outward by
    /// `grow` points: only the groups touching that edge change. Paths are relative to `path`.
    pub(in crate::docking) fn follow(
        &self,
        (node, rect): (&Node<P>, Rect),
        axis: SplitAxis,
        end: bool,
        grow: f32,
        path: &mut Vec<bool>,
        out: &mut Vec<(Vec<bool>, SplitSize)>,
    ) {
        let Some([a, b]) = self.children(node, rect) else { return };
        let Node::Split { axis: split_axis, first, second, .. } = node else { return };
        let docked = matches!(**first, Node::Dock { .. }) || matches!(**second, Node::Dock { .. });
        let visit = |child: (&Node<P>, Rect), second_child: bool, path: &mut Vec<bool>, out: &mut Vec<(Vec<bool>, SplitSize)>| {
            path.push(second_child);
            self.follow(child, axis, end, grow, path, out);
            path.pop();
        };
        if docked {
            let dock_first = matches!(**first, Node::Dock { .. });
            visit(if dock_first { b } else { a }, dock_first, path, out);
        } else if *split_axis == axis {
            let available = extent(a.1, axis) + extent(b.1, axis) + grow;
            let first_size = extent(a.1, axis) + if end { 0.0 } else { grow };
            if available > 0.0 {
                out.push((path.clone(), SplitSize::Ratio((first_size / available).clamp(0.0, 1.0))));
            }
            visit(if end { b } else { a }, end, path, out);
        } else {
            visit(a, false, path, out);
            visit(b, true, path, out);
        }
    }

    /// The divider of the split `node` drawn in `rect` moved by `delta` points, clamped to its
    /// travel: the split's own size and those of the splits that keep the groups away from the
    /// divider at their sizes. Paths are relative to `path`, the split's own.
    pub(in crate::docking) fn drag(&self, node: &Node<P>, rect: Rect, delta: f32, path: &mut Vec<bool>) -> Vec<(Vec<bool>, SplitSize)> {
        let Node::Split { axis, size, first, second } = node else { return Vec::new() };
        let Some(split) = self.split(*axis, *size, first, second, rect) else { return Vec::new() };
        let (lo, hi) = self.range(node, rect);
        let delta = delta.clamp(lo, hi);
        if delta == 0.0 || !delta.is_finite() || split.available() <= 0.0 {
            return Vec::new();
        }
        let mut out = vec![(path.clone(), SplitSize::Ratio(((split.first_size() + delta) / split.available()).clamp(0.0, 1.0)))];
        path.push(false);
        self.follow((first, split.first), *axis, true, delta, path, &mut out);
        path.pop();
        path.push(true);
        self.follow((second, split.second), *axis, false, -delta, path, &mut out);
        path.pop();
        out
    }

    /// How far the divider of the split `node` drawn in `rect` can move: `(lo, hi)`, with
    /// `lo <= 0 <= hi`.
    pub(in crate::docking) fn range(&self, node: &Node<P>, rect: Rect) -> (f32, f32) {
        let Node::Split { axis, size, first, second } = node else { return (0.0, 0.0) };
        let Some(split) = self.split(*axis, *size, first, second, rect) else { return (0.0, 0.0) };
        let (first_in, first_out) = self.travel(first, split.first, *axis, true);
        let (second_in, second_out) = self.travel(second, split.second, *axis, false);
        (-first_in.min(second_out), first_out.min(second_in))
    }
}
