use std::{fmt::Debug, hash::Hash};

use egui::{Context, Key, Pos2, Rect, Ui, Vec2, pos2, vec2};

use super::{Renderer, clamp_float, dock::group_action};
use crate::docking::chrome::drop_preview;
use crate::docking::{
    Action, Drag, DropProposal, DropRequest, GroupDrag, GroupGesture, Node, Placement, Target, Zone, cancel_drag,
    dock_model::{Place, node_at},
    resolve_drop,
    surface::TAB_PADDING,
    zone_rect,
};
use crate::layout::SplitAxis;

/// Ctrl (Windows, Linux) or Cmd (macOS) held: the drop floats and no target is offered.
fn float_only(ctx: &Context) -> bool {
    ctx.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.mac_cmd)
}

/// Height of a panel floated out of a dock column when the host gives no floating size.
const DOCKED_FLOAT_HEIGHT: f32 = 320.0;

/// Least height of a floating window or flyout fitted to a short panel's content.
pub(super) const MIN_FITTED_HEIGHT: f32 = 120.0;

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn finish_group_drag(&mut self, ui: &Ui) {
        let ctx = ui.ctx();
        let key = self.id.with("group-drag");
        let Some(drag) = ctx.data(|d| d.get_temp::<GroupDrag<P>>(key)) else { return };
        if !drag.members.iter().all(|p| self.layout.contains(p) && self.permissions(p).movable) {
            ctx.data_mut(|d| d.remove::<GroupDrag<P>>(key));
            return;
        }
        let (escape, focused, down, released, pointer) =
            ctx.input(|i| (i.key_pressed(Key::Escape), i.focused, i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.interact_pos()));
        let pointer = pointer.filter(|p| p.is_finite());
        let ghost = pointer.map(|pointer| self.group_ghost(&drag, pointer));
        if !escape
            && focused
            && (down || released)
            && let Some(pointer) = pointer
            && !self.blockers.iter().any(|r| r.contains(pointer))
            && self.host_drop(ctx, &drag.panel, pointer, true, released, ghost.filter(|_| drag.gesture != GroupGesture::Window))
        {
            if released {
                ctx.data_mut(|d| d.remove::<GroupDrag<P>>(key));
            }
            return;
        }
        // A button release outside the window may never reach egui.
        let has_pointer = ctx.input(|i| i.pointer.has_pointer());
        // Touch-up also removes the pointer, but still completes a drop.
        let touch_ended = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Touch { phase: egui::TouchPhase::End, .. })));
        let float_only = float_only(ctx);
        if escape || !focused || !down || !has_pointer {
            cancel_drag::<P>(ctx, self.id);
            if escape || !focused {
                if drag.gesture == GroupGesture::Window {
                    self.output.actions.push(Action::MoveFloating { panel: drag.panel, rect: drag.original });
                }
                return;
            }
            // Leaving the window ends a group drag where it is, with no drop.
            if !released || !(has_pointer || touch_ended) {
                return;
            }
            let Some(pointer) = pointer.filter(|p| !self.blockers.iter().any(|r| r.contains(*p))) else { return };
            match drag.gesture {
                GroupGesture::Window => self.drop_window(&drag, pointer, float_only),
                GroupGesture::Group | GroupGesture::Column => self.drop_group(&drag, pointer, float_only),
            }
        } else if let Some(pointer) = pointer.filter(|p| !self.blockers.iter().any(|r| r.contains(*p))) {
            self.output.group_drag = Some(drag.panel.clone());
            self.paint_group_drag(ui, &drag, pointer, float_only);
        }
    }

    /// The valid target under the pointer for a whole group. A target holding the group
    /// itself is a no-op proposal; a target the model would reject is no target. A window
    /// moving live looks through itself and does not split frames or join a body's center.
    pub(super) fn group_proposal(&self, drag: &GroupDrag<P>, pointer: Pos2) -> Option<DropProposal<P>> {
        let window = drag.gesture == GroupGesture::Window;
        let valid = |proposal: DropProposal<P>| {
            let to = Target::At { anchor: proposal.anchor.clone(), placement: proposal.placement.clone() };
            (proposal.noop || self.layout.clone().apply_with_permissions(group_action(drag, to), |p| self.permissions(p)).is_ok()).then_some(proposal)
        };
        if let Some(proposal) = self.extra_proposal(&drag.members, pointer, window) {
            return valid(proposal);
        }
        let own = |t: &crate::docking::DropTarget<P>| t.tabs.iter().any(|(p, _)| drag.members.contains(p));
        let pointer = self.seam_pointer(pointer);
        let target =
            self.targets.iter().rev().find(|t| crate::docking::targets::usable(t.rect) && t.rect.contains(pointer) && !(window && own(t)))?;
        if own(target) {
            return Some(DropProposal {
                anchor: drag.panel.clone(),
                placement: Placement::Tab { before: None },
                preview: target.rect,
                insertion: None,
                noop: true,
            });
        }
        let proposal = resolve_drop(&drag.panel, pointer, std::slice::from_ref(target))?;
        if window && matches!(proposal.placement, Placement::Split(_)) {
            return None;
        }
        valid(self.split_preview(proposal))
    }

    /// The preview of a side split beside a frame that holds its parent's fixed size: the half
    /// of the parent or of the other child that the new frame takes.
    fn split_preview(&self, mut proposal: DropProposal<P>) -> DropProposal<P> {
        let Placement::Split(zone) = proposal.placement else { return proposal };
        let axis = match zone {
            Zone::Center => return proposal,
            Zone::Left | Zone::Right => SplitAxis::Horizontal,
            Zone::Top | Zone::Bottom => SplitAxis::Vertical,
        };
        let Some(slot) = self.fixed_slots.iter().find(|slot| slot.axis == axis && slot.panels.contains(&proposal.anchor)) else { return proposal };
        let opposite = match zone {
            Zone::Left => Zone::Right,
            Zone::Right => Zone::Left,
            Zone::Top => Zone::Bottom,
            _ => Zone::Top,
        };
        let before = matches!(zone, Zone::Left | Zone::Top);
        let preview = if before == slot.first { zone_rect(slot.parent, zone) } else { zone_rect(slot.sibling, opposite) };
        proposal.preview = preview.unwrap_or(proposal.preview);
        proposal
    }

    /// A pointer in the gap between docked frames, outside every target, moves onto the edge of
    /// the nearest frame that takes splits, so a seam splits beside that frame rather than floating.
    pub(super) fn seam_pointer(&self, pointer: Pos2) -> Pos2 {
        let usable = |rect: Rect| crate::docking::targets::usable(rect);
        if self.targets.iter().any(|t| usable(t.rect) && t.rect.contains(pointer)) {
            return pointer;
        }
        self.targets
            .iter()
            .filter(|t| t.allow_splits && usable(t.rect))
            .map(|t| (t.rect.distance_to_pos(pointer), t.rect))
            .filter(|(distance, _)| *distance <= self.style.gap)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(pointer, |(_, rect)| rect.clamp(pointer))
    }

    /// Release of a group drag: join or split at the target, or float at the ghost with the
    /// group's size. Over its own docked position nothing changes; a floating group released
    /// over itself moves there.
    fn drop_group(&mut self, drag: &GroupDrag<P>, pointer: Pos2, float_only: bool) {
        if pointer.distance(drag.origin) < 2.0 {
            return;
        }
        if !float_only && let Some((action, _)) = self.dock_group_proposal(drag, pointer) {
            self.output.actions.extend(action);
            return;
        }
        let floating = matches!(self.place(&drag.panel), Some(Place::Floating { .. }));
        let proposal = if float_only { None } else { self.group_proposal(drag, pointer) };
        if let Some(proposal) = proposal.as_ref().filter(|p| !p.noop) {
            let to = Target::At { anchor: proposal.anchor.clone(), placement: proposal.placement.clone() };
            self.output.actions.push(group_action(drag, to));
            return;
        }
        if proposal.is_some() && !floating {
            return;
        }
        // No target: hosts that still apply their own policy for a whole-group release get it.
        self.output.group_drop = Some((drag.panel.clone(), pointer));
        let rect = self.group_ghost(drag, pointer);
        let action = group_action(drag, Target::Float { rect: Some([rect.left(), rect.top(), rect.width(), rect.height()]) });
        if self.layout.clone().apply_with_permissions(action.clone(), |p| self.permissions(p)).is_ok() {
            self.output.actions.push(action);
        }
    }

    /// The ghost at the pointer (a window moving live is its own ghost) under one destination
    /// preview. The ghost has a translucent fill, a 2 pt outline and the members' labels.
    fn paint_group_drag(&mut self, ui: &Ui, drag: &GroupDrag<P>, pointer: Pos2, float_only: bool) {
        let ctx = ui.ctx();
        let painter = self.preview_painter(ctx);
        if drag.gesture != GroupGesture::Window {
            let ghost = self.group_ghost(drag, pointer);
            drop_preview(&painter, ghost, self.style.accent, true);
            let strip = Rect::from_min_size(ghost.min, vec2(ghost.width(), self.style.tab_height.min(ghost.height())));
            let labels = painter.with_clip_rect(strip.shrink(2.0));
            let mut x = strip.left() + TAB_PADDING * 0.5;
            for panel in &drag.members {
                let width = strip.right() - TAB_PADDING * 0.5 - x;
                if width <= 0.0 {
                    break;
                }
                let label = self.label_of(panel);
                let galley = self.tab_label(ui, panel, &label, self.style.text, width);
                let advance = galley.size().x + TAB_PADDING;
                labels.galley(pos2(x, strip.center().y - galley.size().y * 0.5), galley, self.style.text);
                x += advance;
            }
        }
        if float_only {
            return;
        }
        if let Some((action, line)) = self.dock_group_proposal(drag, pointer) {
            if action.is_some() {
                self.paint_dock_line(ctx, line);
            }
            return;
        }
        if let Some(proposal) = self.group_proposal(drag, pointer).filter(|p| !p.noop) {
            let split = matches!(proposal.placement, Placement::Split(zone) | Placement::Edge(zone) if zone != Zone::Center);
            if crate::docking::targets::usable(proposal.preview) {
                drop_preview(&painter, proposal.preview.shrink(1.0), self.style.accent, split);
            }
            if let Some(marker) = proposal.insertion {
                painter.rect_filled(marker, 0.0, self.style.accent);
            }
        }
    }

    pub(in crate::docking) fn finish_drag(&mut self, ctx: &Context) {
        let Some(drag) = ctx.data(|d| d.get_temp::<Drag<P>>(self.id.with("drag"))) else { return };
        let (pointer, down, released, focused, gone) =
            ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down(), i.pointer.primary_released(), i.focused, !i.pointer.has_pointer()));
        // Leaving the window is the drop: on X11 and Windows the release outside never reaches
        // egui, so the drag would otherwise stay on the pointer. `interact_pos` still holds the
        // last position this frame, and the panel floats there rather than docking at the edge.
        let released = released || gone;
        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if escape || !focused || !self.layout.contains(&drag.panel) || (!down && !released) {
            cancel_drag::<P>(ctx, self.id);
            return;
        }
        let Some(pointer) = pointer.filter(|p| p.is_finite()) else {
            if released {
                cancel_drag::<P>(ctx, self.id);
            }
            return;
        };
        if self.blockers.iter().any(|rect| rect.contains(pointer)) {
            if released {
                cancel_drag::<P>(ctx, self.id);
            }
            return;
        }
        let float_only = float_only(ctx) || drag.undock;
        let floatable = self.permissions(&drag.panel).floatable
            && (float_only || gone || !drag.source.contains(pointer) || self.layout.floating.iter().any(|g| g.contains(&drag.panel)));
        let floating_rect = floatable.then(|| self.tab_float_rect(&drag, pointer));
        if self.host_drop(ctx, &drag.panel, pointer, false, released, floating_rect) {
            if released {
                cancel_drag::<P>(ctx, self.id);
            }
            return;
        }
        let dock = if float_only || gone { None } else { self.dock_tab_proposal(&drag.panel, pointer) };
        if let Some((action, line)) = dock {
            if released {
                cancel_drag::<P>(ctx, self.id);
                if let Some(action) = action.filter(|_| pointer.distance(drag.origin) >= 2.0) {
                    self.output.actions.push(action);
                    self.output.focused = Some(drag.panel);
                }
            } else if action.is_some() {
                self.paint_dock_line(ctx, line);
            }
            return;
        }
        let proposal = if float_only {
            None
        } else {
            self.extra_proposal(std::slice::from_ref(&drag.panel), pointer, false)
                .or_else(|| resolve_drop(&drag.panel, self.seam_pointer(pointer), &self.targets).map(|p| self.split_preview(p)))
        };
        if released {
            cancel_drag::<P>(ctx, self.id);
            if pointer.distance(drag.origin) < 2.0 || proposal.as_ref().is_some_and(|proposal| proposal.noop && !gone) {
                return;
            }
            let action = if let Some(proposal) = proposal.filter(|_| !gone) {
                Some(Action::Move { panel: drag.panel.clone(), anchor: proposal.anchor, placement: proposal.placement })
            } else if let Some(rect) = floating_rect {
                Some(Action::Float { panel: drag.panel.clone(), rect: [rect.left(), rect.top(), rect.width(), rect.height()] })
            } else {
                None
            };
            if let Some(action) = action {
                // Validate the proposed gesture against the same complete snapshot. The app
                // still applies it via its own command path after body rendering finishes.
                let mut candidate = self.layout.clone();
                if candidate.apply_with_permissions(action.clone(), |p| self.permissions(p)).is_ok() {
                    self.output.actions.push(action);
                    self.output.focused = Some(drag.panel);
                }
            }
            return;
        }
        let painter = self.preview_painter(ctx);
        if let Some(proposal) = proposal {
            // Reordering inside the source strip shows only the insertion line.
            let same_group = proposal.insertion.is_some()
                && self.targets.iter().any(|target| target.rect == proposal.preview && target.tabs.iter().any(|(panel, _)| panel == &drag.panel));
            if !same_group && !proposal.noop && crate::docking::targets::usable(proposal.preview) {
                drop_preview(&painter, proposal.preview.shrink(1.0), self.style.accent, true);
            }
            if let Some(marker) = proposal.insertion {
                painter.rect_filled(marker, 0.0, self.style.accent);
            }
        } else if let Some(rect) = floating_rect {
            drop_preview(&painter, rect, self.style.accent, false);
        }
    }

    /// The single drop preview layer, above every floating window.
    pub(super) fn preview_painter(&self, ctx: &Context) -> egui::Painter {
        let layer = egui::LayerId::new(egui::Order::Tooltip, self.id.with("drop-preview"));
        ctx.move_to_top(layer);
        ctx.layer_painter(layer)
    }

    /// Size of a panel floated by a drag or the Float menu: the host's floating size, else the
    /// source size within the panel limits. Out of a dock or frame, or with a floating size, the
    /// height fits the content the panel last drew, up to that height (else 320). Content that
    /// filled its body floats at that height.
    pub(super) fn float_size(&self, panel: &P, source: Vec2) -> Vec2 {
        let chrome = self.style.title_height + self.style.tab_height;
        if let Some(size) = self.content.floating_size(panel).filter(|s| s.is_finite() && s.x > 0.0 && s.y > 0.0) {
            return vec2(size.x, self.fitted_height(panel, chrome, size.y).unwrap_or(size.y));
        }
        let mut size = self.iconic.float_width(panel).map_or(source, |width| vec2(width, DOCKED_FLOAT_HEIGHT));
        if matches!(self.place(panel), Some(Place::Dock { .. } | Place::Node(_))) {
            size.y = match self.fitted_height(panel, chrome, DOCKED_FLOAT_HEIGHT) {
                Some(height) => height,
                None if self.content_height(panel) == Some(None) => DOCKED_FLOAT_HEIGHT,
                None => size.y.min(DOCKED_FLOAT_HEIGHT),
            };
        }
        let limits = self.limits(panel);
        let header = vec2(0.0, self.style.tab_height);
        size.max(limits.min + header).min(limits.max + header)
    }

    /// Height of a window around `panel`'s group: `chrome` (title bar and tab strip) and the
    /// content height the panel last drew, at least [`MIN_FITTED_HEIGHT`] and at most `max`.
    /// `None` for a panel the area has not drawn.
    pub(super) fn fitted_height(&self, panel: &P, chrome: f32, max: f32) -> Option<f32> {
        let content = self.content_height(panel).flatten()?;
        Some((chrome + content).max(MIN_FITTED_HEIGHT).min(max))
    }

    /// Record what a drag floats from `panels`, one group drawn at `group` whose panels drag
    /// from `panel` rects, for commands that float without a rect.
    pub(super) fn record_float(&mut self, panels: &[P], panel: Vec2, group: Vec2) {
        let group = self.group_float_size(panels, group);
        for p in panels {
            let size = self.float_size(p, panel);
            self.float_space.panels.push((p.clone(), size));
            self.float_space.groups.push((p.clone(), group));
        }
    }

    /// Record what a drag floats from a dock column drawn at `rect`.
    pub(super) fn record_column_float(&mut self, members: Vec<P>, rect: Rect) {
        let size = self.group_float_size(&members, rect.size());
        self.float_space.columns.extend(members.into_iter().map(|p| (p, size)));
    }

    /// Size of a group or column floated by a drag or Float Tab Group: the source width, and the
    /// tallest member's floating height, else the source height capped like a panel leaving a
    /// dock column. A group fits that height to its active panel's content. A group that already
    /// floats keeps its size.
    pub(super) fn group_float_size(&self, members: &[P], source: Vec2) -> Vec2 {
        let Some(place) = members.first().and_then(|panel| self.place(panel)) else { return source };
        if matches!(place, Place::Floating { .. }) {
            return source;
        }
        let valid = |s: &Vec2| s.is_finite() && s.x > 0.0 && s.y > 0.0;
        let host = members.iter().filter_map(|panel| self.content.floating_size(panel).filter(valid)).map(|s| s.y).reduce(f32::max);
        let docked = matches!(place, Place::Dock { .. });
        let chrome = self.style.title_height + self.style.tab_height;
        let max = host.or(docked.then_some(DOCKED_FLOAT_HEIGHT));
        let fitted = max.and_then(|max| self.group_active(members, place).and_then(|active| self.fitted_height(active, chrome, max)));
        vec2(source.x, fitted.or(host).unwrap_or(if docked { source.y.min(DOCKED_FLOAT_HEIGHT) } else { source.y }))
    }

    /// The active panel of the group at `place` when `members` are that whole group.
    fn group_active<'a>(&'a self, members: &[P], place: &Place) -> Option<&'a P> {
        let (panels, active) = match place {
            Place::Node(path) => match self.layout.root.as_ref().and_then(|root| node_at(root, path)) {
                Some(Node::Tabs { panels, active }) => (panels, *active),
                _ => return None,
            },
            _ => self.layout.group(place).map(|group| (&group.panels, group.active))?,
        };
        (panels.len() == members.len() && members.iter().all(|m| panels.contains(m))).then(|| panels.get(active)).flatten()
    }

    /// Where a group or column drag floats with the pointer at `pointer`, keeping the grab
    /// offset, inside the window (Z2).
    fn group_ghost(&self, drag: &GroupDrag<P>, pointer: Pos2) -> Rect {
        let size = if drag.gesture == GroupGesture::Window { drag.source.size() } else { self.group_float_size(&drag.members, drag.source.size()) };
        let grab = (drag.origin - drag.source.min).clamp(Vec2::ZERO, size);
        clamp_float(Rect::from_min_size(pointer - grab, size), self.float_bounds)
    }

    fn tab_float_rect(&self, drag: &Drag<P>, pointer: Pos2) -> Rect {
        let size = self.float_size(&drag.panel, drag.source.size());
        let grab = drag.grab.min(vec2(size.x, self.style.tab_height));
        clamp_float(Rect::from_min_size(pointer - grab, size), self.float_bounds)
    }

    /// Offer the gesture to [`crate::docking::DockContent::resolve_drag`]. True when the host
    /// owns it; its feedback replaces the renderer's preview.
    pub(super) fn host_drop(
        &mut self,
        ctx: &Context,
        panel: &P,
        pointer: Pos2,
        whole_group: bool,
        released: bool,
        floating_rect: Option<Rect>,
    ) -> bool {
        let request = DropRequest {
            panel,
            pointer,
            targets: &self.targets,
            floating_rect,
            whole_group,
            released,
            workspace: self.workspace,
            screen: self.float_bounds,
        };
        let Some(feedback) = self.content.resolve_drag(ctx, self.style, &request) else { return false };
        if !released {
            let painter = self.preview_painter(ctx);
            if let Some(rect) = feedback.preview.filter(|rect| crate::docking::targets::usable(*rect)) {
                drop_preview(&painter, rect, self.style.accent, true);
            }
            if let Some(rect) = feedback.insertion.filter(|rect| crate::docking::targets::usable(*rect)) {
                painter.rect_filled(rect, 0.0, self.style.accent);
            }
        }
        true
    }
}
