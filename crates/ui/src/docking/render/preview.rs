use std::{fmt::Debug, hash::Hash};

use egui::{Context, Key, Rect, Stroke, StrokeKind};

use super::{Renderer, clamp_float};
use crate::docking::{Action, Drag, GroupDrag, cancel_drag, resolve_drop};

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn finish_group_drag(&mut self, ctx: &Context) {
        let Some(drag) = ctx.data(|d| d.get_temp::<GroupDrag<P>>(self.id.with("group-drag"))) else { return };
        if !self.layout.floating.iter().any(|g| g.contains(&drag.panel) && g.panels().all(|p| self.permissions(p).movable)) {
            ctx.data_mut(|d| d.remove::<GroupDrag<P>>(self.id.with("group-drag")));
            return;
        }
        let (escape, focused, down, released, pointer) =
            ctx.input(|i| (i.key_pressed(Key::Escape), i.focused, i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.interact_pos()));
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
        let proposal = resolve_drop(&drag.panel, pointer, &self.targets);
        if released {
            cancel_drag::<P>(ctx, self.id);
            if pointer.distance(drag.origin) < 2.0 {
                return;
            }
            let action = if let Some(proposal) = proposal.filter(|_| !gone) {
                Some(Action::Move { panel: drag.panel.clone(), anchor: proposal.anchor, placement: proposal.placement })
            } else if self.permissions(&drag.panel).floatable
                && (gone || !drag.source.contains(pointer) || self.layout.floating.iter().any(|g| g.contains(&drag.panel)))
            {
                let rect = clamp_float(Rect::from_min_size(pointer - drag.grab, drag.source.size()), ctx.content_rect());
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
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, self.id.with("drop-preview")));
        if let Some(proposal) = proposal {
            painter.rect(
                proposal.preview.shrink(1.0),
                0.0,
                self.style.accent.gamma_multiply(0.15),
                Stroke::new(2.0, self.style.accent),
                StrokeKind::Inside,
            );
            if let Some(marker) = proposal.insertion {
                painter.rect_filled(marker, 0.0, self.style.accent);
            }
        } else if self.permissions(&drag.panel).floatable {
            let rect = clamp_float(Rect::from_min_size(pointer - drag.grab, drag.source.size()), ctx.content_rect());
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, self.style.accent), StrokeKind::Inside);
        }
    }
}
