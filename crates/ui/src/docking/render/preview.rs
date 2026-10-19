use std::{fmt::Debug, hash::Hash};

use egui::{Context, Key, Pos2, Rect, Stroke, StrokeKind, Vec2, vec2};

use super::{Renderer, clamp_float};
use crate::docking::{Action, Drag, DropRequest, GroupDrag, cancel_drag, dock_model::Place, resolve_drop};

/// Height of a panel floated out of a dock column when the host gives no floating size.
const DOCKED_FLOAT_HEIGHT: f32 = 320.0;

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn finish_group_drag(&mut self, ctx: &Context) {
        let Some(drag) = ctx.data(|d| d.get_temp::<GroupDrag<P>>(self.id.with("group-drag"))) else { return };
        if !self.layout.floating.iter().any(|g| g.contains(&drag.panel) && g.panels().all(|p| self.permissions(p).movable)) {
            ctx.data_mut(|d| d.remove::<GroupDrag<P>>(self.id.with("group-drag")));
            return;
        }
        let (escape, focused, down, released, pointer) =
            ctx.input(|i| (i.key_pressed(Key::Escape), i.focused, i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.interact_pos()));
        if !escape
            && focused
            && (down || released)
            && let Some(pointer) = pointer.filter(|p| p.is_finite())
            && !self.blockers.iter().any(|r| r.contains(pointer))
            && self.host_drop(ctx, &drag.panel, pointer, true, released, None)
        {
            if released {
                ctx.data_mut(|d| d.remove::<GroupDrag<P>>(self.id.with("group-drag")));
            }
            return;
        }
        // A button release outside the window may never reach egui.
        let has_pointer = ctx.input(|i| i.pointer.has_pointer());
        // Touch-up also removes the pointer, but still completes a drop.
        let touch_ended = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Touch { phase: egui::TouchPhase::End, .. })));
        if escape || !focused || !down || !has_pointer {
            cancel_drag::<P>(ctx, self.id);
            if escape || !focused {
                self.output.actions.push(Action::MoveFloating { panel: drag.panel, rect: drag.original });
            } else if released
                && (has_pointer || touch_ended)
                && let Some(pointer) = pointer.filter(|p| p.is_finite())
                && !self.blockers.iter().any(|r| r.contains(pointer))
            {
                self.output.group_drop = Some((drag.panel, pointer));
            }
        } else if pointer.is_some_and(|p| !self.blockers.iter().any(|r| r.contains(p))) {
            self.output.group_drag = Some(drag.panel);
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
        let floatable = self.permissions(&drag.panel).floatable
            && (gone || !drag.source.contains(pointer) || self.layout.floating.iter().any(|g| g.contains(&drag.panel)));
        let floating_rect = floatable.then(|| self.tab_float_rect(ctx, &drag, pointer));
        if self.host_drop(ctx, &drag.panel, pointer, false, released, floating_rect) {
            if released {
                cancel_drag::<P>(ctx, self.id);
            }
            return;
        }
        let proposal = resolve_drop(&drag.panel, pointer, &self.targets);
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
            if !same_group && !proposal.noop {
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
