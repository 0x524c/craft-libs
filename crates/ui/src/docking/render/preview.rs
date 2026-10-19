use std::{fmt::Debug, hash::Hash};

use egui::{Color32, Context, Key, Pos2, Rect, Stroke, StrokeKind, Vec2, vec2};

use super::{Renderer, clamp_float, dock::group_action};
use crate::docking::{
    Action, Drag, DropProposal, DropRequest, GroupDrag, GroupGesture, Placement, Target, Zone, cancel_drag, dock_model::Place, resolve_drop,
};

/// Ctrl (Windows, Linux) or Cmd (macOS) held: the drop floats and no target is offered.
fn float_only(ctx: &Context) -> bool {
    ctx.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.mac_cmd)
}

/// Height of a panel floated out of a dock column when the host gives no floating size.
const DOCKED_FLOAT_HEIGHT: f32 = 320.0;

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn finish_group_drag(&mut self, ctx: &Context) {
        let key = self.id.with("group-drag");
        let Some(drag) = ctx.data(|d| d.get_temp::<GroupDrag<P>>(key)) else { return };
        if !drag.members.iter().all(|p| self.layout.contains(p) && self.permissions(p).movable) {
            ctx.data_mut(|d| d.remove::<GroupDrag<P>>(key));
            return;
        }
        let (escape, focused, down, released, pointer) =
            ctx.input(|i| (i.key_pressed(Key::Escape), i.focused, i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.interact_pos()));
        let pointer = pointer.filter(|p| p.is_finite());
        let ghost = pointer.map(|pointer| clamp_float(drag.source.translate(pointer - drag.origin), ctx.content_rect()));
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
                GroupGesture::Window => self.drop_window(ctx, &drag, pointer, float_only),
                GroupGesture::Group | GroupGesture::Column => self.drop_group(ctx, &drag, pointer, float_only),
            }
        } else if let Some(pointer) = pointer.filter(|p| !self.blockers.iter().any(|r| r.contains(*p))) {
            self.output.group_drag = Some(drag.panel.clone());
            self.paint_group_drag(ctx, &drag, pointer, float_only);
        }
    }

    /// The valid target under the pointer for a whole group. A target holding the group
    /// itself is a no-op proposal; a target the model would reject is no target. A window
    /// moving live looks through itself and does not split frames or join a body's centre.
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
        valid(proposal)
    }

    /// Release of a group drag: join or split at the target, or float at the ghost with the
    /// group's size. Over its own docked position nothing changes; a floating group released
    /// over itself moves there.
    fn drop_group(&mut self, ctx: &Context, drag: &GroupDrag<P>, pointer: Pos2, float_only: bool) {
        if pointer.distance(drag.origin) < 2.0 {
            return;
        }
        if !float_only && let Some((action, _)) = self.dock_group_proposal(ctx, drag, pointer) {
            self.output.actions.extend(action);
            return;
        }
        let floating = matches!(self.layout.place(&drag.panel), Some(Place::Floating { .. }));
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
        let rect = clamp_float(drag.source.translate(pointer - drag.origin), ctx.content_rect());
        let action = group_action(drag, Target::Float { rect: Some([rect.left(), rect.top(), rect.width(), rect.height()]) });
        if self.layout.clone().apply_with_permissions(action.clone(), |p| self.permissions(p)).is_ok() {
            self.output.actions.push(action);
        }
    }

    /// Ghost outline at the pointer (a window moving live is its own ghost) and one
    /// destination preview.
    fn paint_group_drag(&self, ctx: &Context, drag: &GroupDrag<P>, pointer: Pos2, float_only: bool) {
        let painter = self.preview_painter(ctx);
        if drag.gesture != GroupGesture::Window {
            let ghost = clamp_float(drag.source.translate(pointer - drag.origin), ctx.content_rect());
            painter.rect_stroke(ghost, 0.0, Stroke::new(1.0, self.style.accent.gamma_multiply(0.6)), StrokeKind::Inside);
        }
        if float_only {
            return;
        }
        if let Some((action, line)) = self.dock_group_proposal(ctx, drag, pointer) {
            if action.is_some() {
                self.paint_dock_line(ctx, line);
            }
            return;
        }
        if let Some(proposal) = self.group_proposal(drag, pointer).filter(|p| !p.noop) {
            let split = matches!(proposal.placement, Placement::Split(zone) | Placement::Edge(zone) if zone != Zone::Center);
            let fill = if split { self.style.accent.gamma_multiply(0.15) } else { Color32::TRANSPARENT };
            if crate::docking::targets::usable(proposal.preview) {
                painter.rect(proposal.preview.shrink(1.0), 0.0, fill, Stroke::new(2.0, self.style.accent), StrokeKind::Inside);
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
        let floating_rect = floatable.then(|| self.tab_float_rect(ctx, &drag, pointer));
        if self.host_drop(ctx, &drag.panel, pointer, false, released, floating_rect) {
            if released {
                cancel_drag::<P>(ctx, self.id);
            }
            return;
        }
        let dock = if float_only || gone { None } else { self.dock_tab_proposal(ctx, &drag.panel, pointer) };
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
            self.extra_proposal(std::slice::from_ref(&drag.panel), pointer, false).or_else(|| resolve_drop(&drag.panel, pointer, &self.targets))
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
                painter.rect(
                    proposal.preview.shrink(1.0),
                    0.0,
                    self.style.accent.gamma_multiply(0.15),
                    Stroke::new(2.0, self.style.accent),
                    StrokeKind::Inside,
                );
            }
            if let Some(marker) = proposal.insertion {
                painter.rect_filled(marker, 0.0, self.style.accent);
            }
        } else if let Some(rect) = floating_rect {
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, self.style.accent), StrokeKind::Inside);
        }
    }

    /// The single drop preview layer, above every floating window.
    pub(super) fn preview_painter(&self, ctx: &Context) -> egui::Painter {
        let layer = egui::LayerId::new(egui::Order::Tooltip, self.id.with("drop-preview"));
        ctx.move_to_top(layer);
        ctx.layer_painter(layer)
    }

    /// Size of a panel floated by a drag or the Float menu: the host's floating size, else the
    /// source size within the panel limits. A panel leaving a dock column does not take the
    /// column's height.
    pub(super) fn float_size(&self, panel: &P, source: Vec2) -> Vec2 {
        if let Some(size) = self.content.floating_size(panel).filter(|s| s.is_finite() && s.x > 0.0 && s.y > 0.0) {
            return size;
        }
        let mut size = source;
        if matches!(self.layout.place(panel), Some(Place::Dock { .. })) {
            size.y = size.y.min(DOCKED_FLOAT_HEIGHT);
        }
        let limits = self.limits(panel);
        let header = vec2(0.0, self.style.tab_height);
        size.max(limits.min + header).min(limits.max + header)
    }

    fn tab_float_rect(&self, ctx: &Context, drag: &Drag<P>, pointer: Pos2) -> Rect {
        let size = self.float_size(&drag.panel, drag.source.size());
        let grab = drag.grab.min(vec2(size.x, self.style.tab_height));
        clamp_float(Rect::from_min_size(pointer - grab, size), ctx.content_rect())
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
            screen: ctx.content_rect(),
        };
        let Some(feedback) = self.content.resolve_drag(ctx, self.style, &request) else { return false };
        if !released {
            let painter = self.preview_painter(ctx);
            if let Some(rect) = feedback.preview.filter(|rect| crate::docking::targets::usable(*rect)) {
                painter.rect(rect, 0.0, self.style.accent.gamma_multiply(0.15), Stroke::new(2.0, self.style.accent), StrokeKind::Inside);
            }
            if let Some(rect) = feedback.insertion.filter(|rect| crate::docking::targets::usable(*rect)) {
                painter.rect_filled(rect, 0.0, self.style.accent);
            }
        }
        true
    }
}
