use std::{collections::HashMap, fmt::Debug, hash::Hash};

use egui::{Context, Id, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};

use crate::layout::{ResizeHandle, ResizeHandleStyle, SplitAxis, SplitSize, split_rect};

use super::{Action, DockContent, DockOutput, DockStyle, DropTarget, Layout, Node, PanelLimits, Permissions, dock_model::Place};

mod dock;
mod floating;
mod frame;
mod iconic;
#[cfg(feature = "native-windows")]
mod native;

pub(in crate::docking) use frame::{FrameState, view};

pub(in crate::docking) use group::GroupKind;

pub(in crate::docking) use iconic::Iconic;
pub use iconic::{close_flyout, flyout, open_flyout};

#[cfg(test)]
pub(in crate::docking) use dock::fit_widths;
pub(in crate::docking) use floating::RESIZE_HANDLES;
mod group;
mod preview;

pub(super) struct Renderer<'a, P> {
    pub(super) id: Id,
    pub(super) style: &'a DockStyle,
    pub(super) content: &'a mut dyn DockContent<P>,
    pub(super) activation: Option<P>,
    pub(super) blockers: Vec<Rect>,
    pub(super) layout: &'a Layout<P>,
    /// The docked area, for host drop requests.
    pub(super) workspace: Rect,
    /// Where floating windows, flyouts and drag ghosts go: [`crate::docking::DockArea::float_bounds`].
    pub(super) float_bounds: Rect,
    pub(super) targets: Vec<DropTarget<P>>,
    /// [D] Seams, group bands, column edges and new-dock bands, see [`crate::docking::targets::DockDrop`].
    pub(super) dock_targets: Vec<crate::docking::targets::DockDrop<P>>,
    pub(super) owned_interactions: Vec<Id>,
    pub(super) output: DockOutput<P>,
    pub(super) frame: FrameState<P>,
    /// [D] Flyout and iconic column state for this frame.
    pub(super) iconic: Iconic<P>,
    /// What a drag would float from each drawn panel, group and column, for commands.
    pub(super) float_space: crate::docking::FloatSpace<P>,
    /// Each panel's place in `layout`, and its entry in `float_space.contents`.
    pub(super) places: HashMap<P, Place>,
    pub(super) contents: HashMap<P, usize>,
    /// Frames drawn as the fixed-size side of their parent split, for split previews.
    pub(super) fixed_slots: Vec<FixedSlot<P>>,
}

/// A frame that holds the fixed size of its parent split: the parent's and the other child's
/// rects, where a side split of the frame goes (see `dock_model::split_frame`).
pub(super) struct FixedSlot<P> {
    pub(super) panels: Vec<P>,
    pub(super) axis: SplitAxis,
    pub(super) first: bool,
    pub(super) parent: Rect,
    pub(super) sibling: Rect,
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(super) fn place(&self, panel: &P) -> Option<&Place> {
        self.places.get(panel)
    }

    /// The height `panel`'s content last used: `Some(None)` when it filled its body.
    pub(super) fn content_height(&self, panel: &P) -> Option<Option<f32>> {
        self.contents.get(panel).and_then(|i| self.float_space.contents.get(*i)).map(|(_, height)| *height)
    }

    pub(super) fn label_of(&self, panel: &P) -> String {
        self.content.logical_label(panel)
    }

    pub(super) fn permissions(&self, panel: &P) -> Permissions {
        self.content.permissions(panel)
    }

    pub(super) fn limits(&self, panel: &P) -> PanelLimits {
        self.content.limits(panel)
    }

    pub(super) fn register_interaction(&mut self, ctx: &Context, id: Id) {
        // A validated tree has at most 2*MAX_PANELS nodes. Tabs, stack dividers,
        // floating grips/corners and splits fit comfortably inside this fixed bound.
        const CAP: usize = 8 * crate::docking::MAX_PANELS;
        if self.owned_interactions.len() < CAP && !self.owned_interactions.contains(&id) {
            self.owned_interactions.push(id);
        }
        // Retain prior-frame controls until return, and publish current controls before
        // app callbacks. No Context lock spans a callback or a stop_dragging call.
        ctx.data_mut(|d| {
            let ids = d.get_temp_mut_or_default::<Vec<Id>>(self.id.with("owned-interactions"));
            if ids.len() < CAP && !ids.contains(&id) {
                ids.push(id);
            }
        });
    }

    pub(in crate::docking) fn node(&mut self, ui: &mut Ui, node: &Node<P>, rect: Rect, path: &mut Vec<bool>) {
        if !crate::docking::targets::usable(rect) {
            return;
        }
        self.note_edge(path, rect);
        match node {
            Node::Split { axis: SplitAxis::Horizontal, first, second, .. } if self.dock_split(ui, first, second, rect, path) => {}
            Node::Split { axis, size, first, second } => {
                let Some(wanted) = split_rect(rect, *axis, *size, self.style.gap, 0.0) else { return };
                let (lo, hi) = split_bounds(wanted.available(), *axis, self.node_limits(first), self.node_limits(second));
                let first_size = wanted.first_size().clamp(lo, hi);
                let Some(split) = split_rect(rect, *axis, SplitSize::FixedFirst(first_size), self.style.gap, 0.0) else { return };
                let fixed = match size {
                    SplitSize::FixedFirst(_) => Some((first, true, split.second)),
                    SplitSize::FixedSecond(_) => Some((second, false, split.first)),
                    SplitSize::Ratio(_) => None,
                };
                let panels = |node: &Node<P>| match node {
                    Node::Tabs { panels, .. } => Some(panels.clone()),
                    Node::Stack { entries } => Some(entries.iter().map(|e| e.panel.clone()).collect()),
                    Node::Split { .. } | Node::Dock { .. } => None,
                };
                if let Some((node, first, sibling)) = fixed
                    && let Some(panels) = panels(node)
                {
                    self.fixed_slots.push(FixedSlot { panels, axis: *axis, first, parent: rect, sibling });
                }
                path.push(false);
                self.node(ui, first, split.first, path);
                path.pop();
                path.push(true);
                self.node(ui, second, split.second, path);
                path.pop();
                // The divider's inner-edge hit margin must win over child scrollbars, just
                // as accordion dividers win over the adjacent panel's content.
                // Named by its layout path, which `ResizeSplit` takes and hits report.
                let layout_path = self.original_path(path);
                let response = ResizeHandle::new(
                    self.id.with(("split", &layout_path)),
                    *axis,
                    split.divider,
                    &self.style.resize_label,
                    ResizeHandleStyle { hovered: self.style.accent, dragged: self.style.accent, focused: self.style.accent },
                )
                .value(split.first_size(), lo..=hi)
                .show(ui);
                self.register_interaction(ui.ctx(), response.response.id);
                if response.delta != 0.0
                    && let Some(size) = split.resized(*size, (split.first_size() + response.delta).clamp(lo, hi) - split.first_size(), 0.0)
                {
                    self.output.actions.push(Action::ResizeSplit { path: layout_path, size });
                }
            }
            Node::Tabs { panels, active } => self.group(ui, panels, *active, false, rect, GroupKind::Frame),
            Node::Dock { side, columns } => self.dock(ui, *side, columns, rect, rect.width()),
            Node::Stack { entries } => {
                let total_header = (entries.len() as f32 * self.style.tab_height).min(rect.height());
                let remaining = (rect.height() - total_header).max(0.0);
                let total_wanted: f64 = entries.iter().filter(|e| e.open).map(|e| f64::from(e.height.unwrap_or(self.style.min_pane).max(1.0))).sum();
                let header = self.style.tab_height.min(rect.height() / entries.len().max(1) as f32);
                let mut top = rect.top();
                let mut dividers = Vec::new();
                for (index, entry) in entries.iter().enumerate() {
                    let share = if entry.open && total_wanted > 0.0 {
                        (f64::from(remaining) * f64::from(entry.height.unwrap_or(self.style.min_pane).max(1.0)) / total_wanted) as f32
                    } else {
                        0.0
                    };
                    let panel_rect = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), (top + header + share).min(rect.bottom())));
                    let title_rect = Rect::from_min_max(panel_rect.min, pos2(panel_rect.right(), (top + header).min(panel_rect.bottom())));
                    self.tab(ui, &entry.panel, title_rect, entry.open, panel_rect, None);
                    let collapse = if self.style.right_to_left {
                        Rect::from_min_max(title_rect.min, pos2((title_rect.left() + 18.0).min(title_rect.right()), title_rect.bottom()))
                    } else {
                        Rect::from_min_max(pos2((title_rect.right() - 18.0).max(title_rect.left()), title_rect.top()), title_rect.max)
                    };
                    let response = ui.interact(collapse, self.id.with(("collapse", &entry.panel)), Sense::click());
                    self.register_interaction(ui.ctx(), response.id);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), entry.open, self.label_of(&entry.panel))
                    });
                    ui.painter().with_clip_rect(collapse).text(
                        collapse.center(),
                        egui::Align2::CENTER_CENTER,
                        if entry.open { "−" } else { "+" },
                        self.style.font.clone(),
                        self.style.text,
                    );
                    if response.clicked() {
                        self.output.actions.push(Action::SetStackOpen { panel: entry.panel.clone(), open: !entry.open });
                    }
                    let body_rect = Rect::from_min_max(pos2(rect.left(), title_rect.bottom()), panel_rect.max);
                    if entry.open {
                        self.contents(ui, &entry.panel, body_rect);
                    }
                    self.targets.push(DropTarget {
                        anchor: entry.panel.clone(),
                        rect: panel_rect,
                        tab_strip: Some(title_rect),
                        tabs: vec![(entry.panel.clone(), title_rect)],
                        allow_splits: self.permissions(&entry.panel).accepts_splits,
                        accepts_tabs: self.permissions(&entry.panel).accepts_tabs,
                        right_to_left: self.style.right_to_left,
                    });
                    top = panel_rect.bottom();
                    if entry.open && remaining > 0.0 {
                        dividers.push((index, Rect::from_min_max(pos2(rect.left(), top - 1.0), pos2(rect.right(), top + 1.0)).intersect(rect)));
                    }
                }
                // Register dividers after the headers so their narrow hit regions win at the
                // shared boundary. Closed entries retain their saved sizes and are skipped.
                for (index, divider) in dividers {
                    let Some(first) = entries.get(index) else { continue };
                    let Some(second) = entries.iter().skip(index.saturating_add(1)).find(|entry| entry.open) else { continue };
                    let a = f64::from(first.height.unwrap_or(self.style.min_pane).max(1.0));
                    let b = f64::from(second.height.unwrap_or(self.style.min_pane).max(1.0));
                    let scale = f64::from(remaining) / total_wanted;
                    let first_size = (a * scale) as f32;
                    let pair_size = ((a + b) * scale) as f32;
                    let body_limits = |panel: &P| {
                        let mut limits = self.limits(panel);
                        limits.min.y = limits.min.y.max(self.style.min_pane);
                        limits.max = limits.max.max(limits.min);
                        limits
                    };
                    let (lo, hi) = split_bounds(pair_size, SplitAxis::Vertical, body_limits(&first.panel), body_limits(&second.panel));
                    let response = ResizeHandle::new(
                        self.id.with(("stack-resize", &first.panel)),
                        SplitAxis::Vertical,
                        divider,
                        &self.style.resize_label,
                        ResizeHandleStyle { hovered: self.style.accent, dragged: self.style.accent, focused: self.style.accent },
                    )
                    .value(first_size, lo..=hi)
                    .show(ui);
                    self.register_interaction(ui.ctx(), response.response.id);
                    if response.delta != 0.0 && scale > 0.0 {
                        let delta = f64::from((first_size + response.delta).clamp(lo, hi) - first_size) / scale;
                        let next_a = (a + delta).clamp((a + b - 1_000_000.0).max(1.0), (a + b - 1.0).min(1_000_000.0));
                        self.output.actions.push(Action::ResizeStack { panel: first.panel.clone(), height: Some(next_a as f32) });
                        self.output.actions.push(Action::ResizeStack { panel: second.panel.clone(), height: Some((a + b - next_a) as f32) });
                    }
                }
            }
        }
    }

    pub(super) fn node_limits(&self, node: &Node<P>) -> PanelLimits {
        node_limits(node, self.style, &*self.content)
    }

    pub(super) fn contents(&mut self, ui: &mut Ui, panel: &P, rect: Rect) {
        if !crate::docking::targets::usable(rect) {
            return;
        }
        ui.painter().rect_filled(rect, 0.0, self.style.background);
        let mut child =
            ui.new_child(UiBuilder::new().id(self.id.with(("body", panel))).max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        self.content.body(&mut child, panel);
        let at = self.contents.get(panel).copied();
        let at = self.float_space.record_content(at, panel.clone(), child.min_rect().height(), rect.height());
        self.contents.insert(panel.clone(), at);
        // Any button: a canvas pans with the middle button and opens menus with the secondary.
        if child.rect_contains_pointer(rect) && child.input(|i| i.pointer.any_pressed()) {
            self.output.focused = Some(panel.clone());
        }
    }
}

/// A lone protected or bare panel draws no tab strip and no drag bar.
pub(in crate::docking) fn stripless<P>(panels: &[P], content: &dyn DockContent<P>) -> bool {
    match panels {
        [panel] => content.permissions(panel).is_protected() || content.bare(panel),
        _ => false,
    }
}

/// Size policy of a subtree, shared by the renderer and its measurements.
pub(in crate::docking) fn node_limits<P>(node: &Node<P>, style: &DockStyle, content: &dyn DockContent<P>) -> PanelLimits {
    match node {
        Node::Dock { columns, .. } => {
            let width: f32 = columns.iter().map(|c| dock::column_width(c, style)).sum();
            PanelLimits { min: vec2(width.clamp(style.min_pane, 1_000_000.0), style.min_pane), ..PanelLimits::default() }
        }
        Node::Tabs { panels, .. } => {
            let mut limits = PanelLimits::default();
            for panel in panels {
                let policy = content.limits(panel);
                limits.min = limits.min.max(policy.min).max(Vec2::splat(style.min_pane));
                limits.max = limits.max.min(policy.max);
            }
            limits.max = limits.max.max(limits.min);
            if !stripless(panels, content) {
                limits.min.y = (limits.min.y + style.tab_height).min(1_000_000.0);
                limits.max.y = (limits.max.y + style.tab_height).min(1_000_000.0);
            }
            limits
        }
        Node::Stack { entries } => {
            let mut limits = PanelLimits::default();
            for entry in entries {
                let policy = content.limits(&entry.panel);
                limits.min.x = limits.min.x.max(policy.min.x).max(style.min_pane);
                limits.min.y += if entry.open { policy.min.y.max(style.min_pane) } else { 0.0 } + style.tab_height;
                limits.max.x = limits.max.x.min(policy.max.x);
            }
            limits.min = limits.min.min(Vec2::splat(1_000_000.0));
            limits.max = limits.max.max(limits.min);
            limits
        }
        Node::Split { axis, first, second, .. } => {
            let a = node_limits(first, style, content);
            let b = node_limits(second, style, content);
            let limits = match axis {
                SplitAxis::Horizontal => PanelLimits {
                    min: vec2(a.min.x + b.min.x + style.gap, a.min.y.max(b.min.y)),
                    max: vec2(a.max.x + b.max.x + style.gap, a.max.y.min(b.max.y)),
                },
                SplitAxis::Vertical => PanelLimits {
                    min: vec2(a.min.x.max(b.min.x), a.min.y + b.min.y + style.gap),
                    max: vec2(a.max.x.min(b.max.x), a.max.y + b.max.y + style.gap),
                },
            };
            PanelLimits { min: limits.min.min(Vec2::splat(1_000_000.0)), max: limits.max.max(limits.min).min(Vec2::splat(1_000_000.0)) }
        }
    }
}

pub(super) fn clamp_float(rect: Rect, screen: Rect) -> Rect {
    crate::docking::chrome::clamp(rect, screen)
}

pub(super) fn split_bounds(available: f32, axis: SplitAxis, first: PanelLimits, second: PanelLimits) -> (f32, f32) {
    let (a_min, a_max, b_min, b_max) = match axis {
        SplitAxis::Horizontal => (first.min.x, first.max.x, second.min.x, second.max.x),
        SplitAxis::Vertical => (first.min.y, first.max.y, second.min.y, second.max.y),
    };
    let scale = if a_min + b_min > available { available / (a_min + b_min).max(1.0) } else { 1.0 };
    let lo = (a_min * scale).min(available);
    let hi = (available - b_min * scale).max(lo).min(available);
    let constrained_lo = lo.max(available - b_max);
    let constrained_hi = hi.min(a_max);
    if constrained_lo <= constrained_hi { (constrained_lo, constrained_hi) } else { (lo, hi) }
}
