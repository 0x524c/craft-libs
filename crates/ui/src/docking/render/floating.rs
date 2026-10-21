use std::{fmt::Debug, hash::Hash};

use egui::{Context, Id, Key, Pos2, Rect, Ui, pos2, vec2};

use super::{GroupKind, Renderer, clamp_float, node_limits};
use crate::docking::chrome::{ChromeStyle, FloatingFrame, RESIZE_BAND, TitleBar};
use crate::docking::{Action, DropProposal, Floating, Group, GroupDrag, GroupGesture, Node, PanelLimits, Placement, Target};
use crate::layout::{ResizeHandle, ResizeHandleStyle, SplitAxis};

pub(in crate::docking) use crate::docking::chrome::RESIZE_HANDLES;

/// Height of the bar between two stacked groups of a floating window.
const GAP: f32 = 3.0;

/// A floating stack as drawn, for its band and seam targets.
pub(in crate::docking) struct StackHit<P> {
    pub(super) window: Rect,
    /// Each group's members, rect and tab strip, top to bottom.
    pub(super) groups: Vec<(Vec<P>, Rect, Rect)>,
    /// The upper group's index and the band of each seam.
    pub(super) seams: Vec<(usize, Rect)>,
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn floating(&mut self, ctx: &Context, windows: &[Floating<P>]) {
        let screen = self.float_bounds;
        if !crate::docking::targets::usable(screen) {
            return;
        }
        let layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
        let mut windows: Vec<_> = windows.iter().collect();
        windows.sort_by_key(|window| {
            let id = window.panels().next().map(|panel| self.id.with(("floating", panel)));
            layers.iter().position(|layer| Some(layer.id) == id).unwrap_or(usize::MAX)
        });
        #[cfg(feature = "native-windows")]
        let windows = self.native_windows(ctx, windows);
        for window in windows {
            self.floating_window(ctx, window, screen);
        }
    }

    /// Size limits of a floating window: its title bar, each group's strip and the bodies of
    /// the groups that are not minimized.
    pub(super) fn window_limits(&self, window: &Floating<P>) -> PanelLimits {
        // A movable window is resizable, with a resize band above its title bar.
        let band = if window.panels().all(|panel| self.permissions(panel).movable) { RESIZE_BAND } else { 0.0 };
        let chrome = band + self.style.title_height;
        let mut limits = PanelLimits { min: vec2(0.0, chrome), max: vec2(1_000_000.0, chrome) };
        for (index, group) in window.groups.iter().enumerate() {
            let policy = node_limits(&Node::Tabs { panels: group.panels.clone(), active: 0 }, self.style, &*self.content);
            let gap = if index > 0 { GAP } else { 0.0 };
            limits.min.x = limits.min.x.max(policy.min.x);
            limits.max.x = limits.max.x.min(policy.max.x);
            let (min, max) = if group.minimized { (self.style.tab_height, self.style.tab_height) } else { (policy.min.y, policy.max.y) };
            limits.min.y += min + gap;
            limits.max.y += max + gap;
        }
        let min = limits.min.min(vec2(1_000_000.0, 1_000_000.0));
        PanelLimits { min, max: limits.max.max(min).min(vec2(1_000_000.0, 1_000_000.0)) }
    }

    fn floating_window(&mut self, ctx: &Context, window: &Floating<P>, screen: Rect) {
        let Some(first) = window.panels().next() else { return };
        let [x, y, width, height] = window.rect;
        let limits = self.window_limits(window);
        let size = vec2(width, height).max(limits.min).min(limits.max);
        let rect = crate::docking::chrome::clamp(Rect::from_min_size(pos2(x, y), size), screen);
        let id = self.id.with(("floating", first));
        if self.activation.as_ref().is_some_and(|panel| window.contains(panel)) {
            ctx.move_to_top(FloatingFrame::layer(id));
        }
        let movable = window.panels().all(|panel| self.permissions(panel).movable);
        let style = ChromeStyle::from_dock(self.style);
        let resize_label = self.style.resize_window_label.clone();
        let response = FloatingFrame::new(id, rect).bounds(screen).limits(limits).resizable(movable).resize_label(&resize_label).show(
            ctx,
            &style,
            |ui, contents| self.window_contents(ui, window, contents, rect, &style),
        );
        for handle in &response.handles {
            self.register_interaction(ctx, handle.id);
        }
        let down = ctx.input(|i| i.pointer.primary_down());
        ctx.data_mut(|data| {
            let key = self.id.with("pointer-resize");
            if let Some(started) = response.resize_started {
                data.insert_temp(key, started);
            } else if !down && data.get_temp::<Id>(key).is_some_and(|id| response.handles.iter().any(|h| h.id == id)) {
                data.remove::<Id>(key);
            }
        });
        if let Some(resized) = response.resized {
            self.output
                .actions
                .push(Action::MoveFloating { panel: first.clone(), rect: [resized.left(), resized.top(), resized.width(), resized.height()] });
        }
    }

    /// The title bar, then the stacked groups below it, in `contents`. Moves carry the whole
    /// `frame`, resize band included.
    fn window_contents(&mut self, ui: &mut Ui, window: &Floating<P>, contents: Rect, frame: Rect, style: &ChromeStyle) {
        let title = Rect::from_min_max(contents.min, pos2(contents.right(), (contents.top() + self.style.title_height).min(contents.bottom())));
        self.title_bar(ui, window, title, frame, style);
        let body = Rect::from_min_max(pos2(contents.left(), title.bottom()), contents.max);
        self.floating_stack(ui, &window.groups, body, frame);
    }

    /// Drag moves the whole window live, double-click minimizes or expands every group, ×
    /// closes every panel in it.
    fn title_bar(&mut self, ui: &mut Ui, window: &Floating<P>, title: Rect, rect: Rect, style: &ChromeStyle) {
        let Some(first) = window.panels().next() else { return };
        let movable = window.panels().all(|p| self.permissions(p).movable);
        let closable = self.style.close_button && window.panels().all(|p| self.permissions(p).closable);
        let (move_label, close_label) = (self.style.move_window_label.clone(), self.style.close_window_label.clone());
        let mut bar = TitleBar::new(self.id.with(("move-group", Some(first))), &move_label).movable(movable);
        if closable {
            bar = bar.close(&close_label);
        }
        let response = bar.show(ui, title, style);
        self.register_interaction(ui.ctx(), response.bar.id);
        if movable && response.bar.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).or_else(|| ui.ctx().pointer_interact_pos()).unwrap_or(title.center());
            let drag = GroupDrag {
                panel: first.clone(),
                original: [rect.left(), rect.top(), rect.width(), rect.height()],
                gesture: GroupGesture::Window,
                members: window.panels().cloned().collect(),
                origin,
                source: rect,
            };
            ui.ctx().data_mut(|d| d.insert_temp(self.id.with("group-drag"), drag));
        }
        if movable
            && response.bar.dragged()
            && let Some(drag) = ui.ctx().data(|d| d.get_temp::<GroupDrag<P>>(self.id.with("group-drag")))
            && drag.gesture == GroupGesture::Window
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
            && !ui.input(|i| i.key_pressed(Key::Escape) || !i.focused)
        {
            // From the press origin, so fractional pointer steps do not accumulate rounding.
            let moved = clamp_float(drag.source.translate(pointer - drag.origin), self.float_bounds);
            self.output.actions.push(Action::MoveFloating { panel: first.clone(), rect: [moved.left(), moved.top(), moved.width(), moved.height()] });
        }
        if response.bar.double_clicked() {
            let minimized = !window.groups.iter().all(|g| g.minimized);
            for group in window.groups.iter().filter(|g| g.minimized != minimized) {
                if let Some(panel) = group.active_panel() {
                    self.output.actions.push(Action::SetMinimized { panel: panel.clone(), minimized });
                }
            }
        }
        if let Some(close) = response.close {
            self.register_interaction(ui.ctx(), close.id);
            if close.clicked() {
                self.output.actions.push(Action::CloseWindow { panel: first.clone() });
            }
        }
    }

    /// Groups top to bottom with a bar between them. Minimized groups take their strip; the
    /// others share the rest by their preferred heights, equally when none is set.
    pub(super) fn floating_stack(&mut self, ui: &mut Ui, groups: &[Group<P>], body: Rect, window: Rect) {
        let strip = self.style.tab_height;
        let open = groups.iter().filter(|g| !g.minimized).count();
        let fixed = groups.len().saturating_sub(open) as f32 * strip + groups.len().saturating_sub(1) as f32 * GAP;
        let available = (body.height() - fixed).max(0.0);
        let equal = if open > 0 { available / open as f32 } else { 0.0 };
        let wanted = |g: &Group<P>| g.height.filter(|h| h.is_finite() && *h > 0.0).unwrap_or(equal).max(1.0);
        let total: f32 = groups.iter().filter(|g| !g.minimized).map(wanted).sum();
        let scale = if total > 0.0 { available / total } else { 0.0 };
        let mut hit = StackHit { window, groups: Vec::new(), seams: Vec::new() };
        let mut top = body.top();
        let mut rects = Vec::new();
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                let gap = Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), (top + GAP).min(body.bottom())));
                ui.painter().rect_filled(gap, 0.0, self.style.tab_background);
                hit.seams.push((index - 1, Rect::from_center_size(gap.center(), vec2(gap.width(), self.style.seam_band))));
                top = gap.bottom();
            }
            let height = if group.minimized { strip } else { wanted(group) * scale };
            let rect = Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), (top + height).min(body.bottom())));
            self.group(ui, &group.panels, group.active, group.minimized, rect, GroupKind::Floating { window });
            let strip_rect = Rect::from_min_max(rect.min, pos2(rect.right(), (rect.top() + strip).min(rect.bottom())));
            hit.groups.push((group.panels.clone(), rect, strip_rect));
            rects.push(rect);
            top = rect.bottom();
        }
        // Dividers between two open groups resize both; registered after the strips, so their
        // hit region wins at the shared edge.
        for (index, pair) in groups.windows(2).enumerate() {
            let ([upper, lower], Some(a), Some(b)) = (pair, rects.get(index), rects.get(index + 1)) else { continue };
            if upper.minimized || lower.minimized || scale <= 0.0 {
                continue;
            }
            let (Some(upper_panel), Some(lower_panel)) = (upper.active_panel(), lower.active_panel()) else { continue };
            let divider = Rect::from_min_max(pos2(body.left(), a.bottom()), pos2(body.right(), b.top()));
            let least = strip + self.style.min_pane;
            let pair_size = a.height() + b.height();
            let (lo, hi) = if pair_size >= 2.0 * least { (least, pair_size - least) } else { (a.height(), a.height()) };
            let response = ResizeHandle::new(
                self.id.with(("group-resize", upper.panels.first())),
                SplitAxis::Vertical,
                divider,
                &self.style.resize_label,
                ResizeHandleStyle { hovered: self.style.accent, dragged: self.style.accent, focused: self.style.accent },
            )
            .value(a.height(), lo..=hi)
            .show(ui);
            self.register_interaction(ui.ctx(), response.response.id);
            if response.delta != 0.0 {
                let next = (a.height() + response.delta).clamp(lo, hi);
                self.output.actions.push(Action::ResizeGroup { panel: upper_panel.clone(), height: Some(next / scale) });
                self.output.actions.push(Action::ResizeGroup { panel: lower_panel.clone(), height: Some((pair_size - next) / scale) });
            }
        }
        self.frame.stacks.push(hit);
    }

    /// Targets the group targets do not cover: the bands and seams of a floating stack (P3,
    /// P4), then the layout's outer edge (P8). None leaves the pointer to the group targets.
    /// `skip_own` looks through the window holding `members` (a window moving live).
    pub(super) fn extra_proposal(&self, members: &[P], pointer: Pos2, skip_own: bool) -> Option<DropProposal<P>> {
        let own = |panels: &[P]| panels.iter().any(|p| members.contains(p));
        let window = self.frame.stacks.iter().rev().find(|w| w.window.contains(pointer) && !(skip_own && w.groups.iter().any(|(g, ..)| own(g))));
        if let Some(window) = window {
            let in_group = window.groups.iter().any(|(_, rect, _)| rect.contains(pointer)) || window.seams.iter().any(|(_, b)| b.contains(pointer));
            if !in_group {
                // The title bar occludes what lies behind the window.
                let anchor = members.first()?.clone();
                return Some(DropProposal {
                    anchor,
                    placement: Placement::Tab { before: None },
                    preview: Rect::NOTHING,
                    insertion: None,
                    noop: true,
                });
            }
            return self.stack_proposal(window, members, pointer);
        }
        let (anchor, zone, preview) = self.edge_proposal(members, pointer)?;
        Some(DropProposal { anchor, placement: Placement::Edge(zone), preview, insertion: None, noop: false })
    }

    fn stack_proposal(&self, hit: &StackHit<P>, members: &[P], pointer: Pos2) -> Option<DropProposal<P>> {
        let line = |y: f32| Rect::from_min_max(pos2(hit.window.left(), y - 1.0), pos2(hit.window.right(), y + 1.0));
        let same = |panels: &[P]| panels == members;
        // An anchor in the group that is not itself moving.
        let anchor = |panels: &[P]| panels.iter().find(|p| !members.contains(p)).cloned();
        for (upper, band) in &hit.seams {
            if !band.contains(pointer) {
                continue;
            }
            let (Some((a, rect, _)), Some((b, ..))) = (hit.groups.get(*upper), hit.groups.get(upper + 1)) else { return None };
            let line = line(rect.bottom() + GAP * 0.5);
            if same(a) || same(b) {
                let anchor = members.first()?.clone();
                return Some(DropProposal {
                    anchor,
                    placement: Placement::Group { below: true, height: None },
                    preview: Rect::NOTHING,
                    insertion: None,
                    noop: true,
                });
            }
            let (anchor, below) = match anchor(a) {
                Some(panel) => (panel, true),
                None => (anchor(b)?, false),
            };
            return Some(DropProposal {
                anchor,
                placement: Placement::Group { below, height: None },
                preview: Rect::NOTHING,
                insertion: Some(line),
                noop: false,
            });
        }
        let (panels, rect, strip) = hit.groups.iter().find(|(_, rect, _)| rect.contains(pointer))?;
        if strip.contains(pointer) || same(panels) {
            return None;
        }
        let band = rect.height() * 0.25;
        let below = if pointer.y < rect.top() + band {
            false
        } else if pointer.y > rect.bottom() - band {
            true
        } else {
            return None;
        };
        let insertion = Some(line(if below { rect.bottom() } else { rect.top() }));
        Some(DropProposal {
            anchor: anchor(panels)?,
            placement: Placement::Group { below, height: None },
            preview: Rect::NOTHING,
            insertion,
            noop: false,
        })
    }

    /// What a group drag carries: the whole window rect for the only group of a floating
    /// window, so floating it again keeps the window's size; else the group's rect.
    pub(super) fn drag_source(&self, owner: &super::group::TabGroup<'_, P>) -> Rect {
        match owner.kind {
            GroupKind::Floating { window }
                if owner.panels.first().is_some_and(|p| self.layout.floating.iter().any(|w| w.groups.len() == 1 && w.contains(p))) =>
            {
                window
            }
            _ => owner.rect,
        }
    }

    /// Release of a title bar drag. The window already sits where it was dragged; a dock or
    /// group target docks or merges every group in it, else hosts get the release in
    /// `group_drop`.
    pub(super) fn drop_window(&mut self, drag: &GroupDrag<P>, pointer: Pos2, float_only: bool) {
        if !float_only && let Some((action, _)) = self.dock_group_proposal(drag, pointer) {
            self.output.actions.extend(action);
            return;
        }
        let proposal = if float_only { None } else { self.group_proposal(drag, pointer) };
        match proposal.filter(|p| !p.noop) {
            Some(proposal) => {
                let to = Target::At { anchor: proposal.anchor, placement: proposal.placement };
                self.output.actions.push(super::dock::group_action(drag, to));
            }
            None => self.output.group_drop = Some((drag.panel.clone(), pointer)),
        }
    }
}
