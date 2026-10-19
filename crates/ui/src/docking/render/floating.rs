use std::{fmt::Debug, hash::Hash};

use egui::{Context, Id, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::{GroupKind, Renderer, clamp_float};
use crate::docking::{Action, Floating, Node, PanelLimits, resize::AnchoredResize};

/// Floating window resize handles: id suffix, hit rect and the edges they move.
fn handles(rect: Rect) -> [(&'static str, Rect, Vec2); 5] {
    [
        ("resize", Rect::from_min_max((rect.max - vec2(12.0, 12.0)).max(rect.min), rect.max), vec2(1.0, 1.0)),
        (
            "resize-left",
            Rect::from_min_max(pos2(rect.left(), rect.top() + 12.0), pos2((rect.left() + 4.0).min(rect.right()), rect.bottom() - 12.0)),
            vec2(-1.0, 0.0),
        ),
        (
            "resize-right",
            Rect::from_min_max(pos2((rect.right() - 4.0).max(rect.left()), rect.top() + 12.0), pos2(rect.right(), rect.bottom() - 12.0)),
            vec2(1.0, 0.0),
        ),
        (
            "resize-bottom",
            Rect::from_min_max(pos2(rect.left() + 12.0, (rect.bottom() - 4.0).max(rect.top())), pos2(rect.right() - 12.0, rect.bottom())),
            vec2(0.0, 1.0),
        ),
        (
            "resize-bottom-left",
            Rect::from_min_max(
                pos2(rect.left(), (rect.bottom() - 12.0).max(rect.top())),
                pos2((rect.left() + 12.0).min(rect.right()), rect.bottom()),
            ),
            vec2(-1.0, 1.0),
        ),
    ]
}

/// Names of every floating window resize handle, for focus classification.
pub(in crate::docking) const RESIZE_HANDLES: [&str; 5] = ["resize", "resize-left", "resize-right", "resize-bottom", "resize-bottom-left"];

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn floating(&mut self, ctx: &Context) {
        let screen = ctx.content_rect();
        if !crate::docking::targets::usable(screen) {
            return;
        }
        let layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
        let mut windows: Vec<_> = self.layout.floating.iter().collect();
        windows.sort_by_key(|window| {
            let id = window.panels().next().map(|panel| self.id.with(("floating", panel)));
            layers.iter().position(|layer| Some(layer.id) == id).unwrap_or(usize::MAX)
        });
        for window in windows {
            let Some(first) = window.panels().next() else { continue };
            let [x, y, width, height] = window.rect;
            let limits = self.node_limits(&Node::Tabs { panels: window.panels().cloned().collect(), active: 0 });
            let mut size = vec2(width, height).max(limits.min).min(limits.max);
            if window.groups.iter().all(|group| group.minimized) {
                size.y = size.y.min(window.groups.len() as f32 * self.style.tab_height);
            }
            let rect = clamp_float(Rect::from_min_size(pos2(x, y), size), screen);
            let id = self.id.with(("floating", first));
            if self.activation.as_ref().is_some_and(|panel| window.contains(panel)) {
                ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
                ctx.memory_mut(|memory| memory.request_focus(self.id.with(("tab", self.activation.as_ref().unwrap_or(first)))));
                self.output.focused = self.activation.clone();
            }
            egui::Area::new(id).fade_in(false).order(egui::Order::Middle).fixed_pos(rect.min).movable(false).constrain_to(screen).show(ctx, |ui| {
                ui.set_min_size(rect.size());
                let rect = Rect::from_min_size(ui.min_rect().min, rect.size());
                ui.allocate_rect(rect, Sense::hover());
                // Opaque even when the host's panel colours are transparent.
                ui.painter().rect_filled(rect, 0.0, self.style.floating_background);
                self.stacked_groups(ui, &window.groups, rect, GroupKind::Floating { window: rect });
                ui.painter().rect_stroke(rect, 0.0, self.style.floating_stroke, egui::StrokeKind::Outside);
                self.resize_handles(ui, window, id, rect, limits, screen);
            });
        }
    }

    /// Anchored resize from the side edges, the bottom edge and both bottom corners: the
    /// opposite edges stay where they are.
    fn resize_handles(&mut self, ui: &mut Ui, window: &Floating<P>, id: Id, rect: Rect, limits: PanelLimits, screen: Rect) {
        let Some(first) = window.panels().next() else { return };
        for (name, handle, direction) in handles(rect) {
            if !crate::docking::targets::usable(handle) {
                continue;
            }
            let cursor = if direction.x == 0.0 {
                egui::CursorIcon::ResizeVertical
            } else if direction.y == 0.0 {
                egui::CursorIcon::ResizeHorizontal
            } else if direction.x == direction.y {
                egui::CursorIcon::ResizeNwSe
            } else {
                egui::CursorIcon::ResizeNeSw
            };
            let response = ui.interact(handle, id.with(name), Sense::drag()).on_hover_cursor(cursor);
            self.register_interaction(ui.ctx(), response.id);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.resize_window_label));
            let anchor_id = response.id.with("pointer-anchor");
            if response.drag_started() {
                response.request_focus();
                if let Some(origin) = ui.input(|i| i.pointer.press_origin())
                    && let Some(anchor) = AnchoredResize::new(rect, origin, direction)
                {
                    ui.ctx().data_mut(|data| {
                        if let Some(previous) = data.get_temp::<Id>(self.id.with("pointer-resize")) {
                            data.remove::<AnchoredResize>(previous.with("pointer-anchor"));
                        }
                        data.insert_temp(self.id.with("pointer-resize"), response.id);
                        data.insert_temp(anchor_id, anchor);
                    });
                }
            }
            let cancelled = ui.input(|input| input.key_pressed(Key::Escape) || !input.focused);
            let mut delta = Vec2::ZERO;
            if response.has_focus() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
                    )
                });
                ui.input_mut(|input| {
                    if input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                        delta.x -= 8.0;
                    }
                    if input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                        delta.x += 8.0;
                    }
                    if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                        delta.y -= 8.0;
                    }
                    if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                        delta.y += 8.0;
                    }
                });
            }
            let pointer_resize = response.dragged() || response.drag_stopped();
            let limits = PanelLimits { min: limits.min.max(vec2(40.0, self.style.tab_height + 20.0)).min(limits.max), ..limits };
            let next = if pointer_resize {
                ui.ctx()
                    .data(|data| data.get_temp::<AnchoredResize>(anchor_id))
                    .and_then(|anchor| ui.ctx().pointer_interact_pos().and_then(|pointer| anchor.rect(pointer, screen, limits)))
            } else if delta != Vec2::ZERO {
                AnchoredResize::new(rect, rect.max, direction).and_then(|anchor| anchor.rect(rect.max + delta, screen, limits))
            } else {
                None
            };
            let permitted = response.enabled() && window.panels().all(|panel| self.permissions(panel).movable);
            if permitted
                && !cancelled
                && let Some(resized) = next.filter(|next| *next != rect)
            {
                self.output
                    .actions
                    .push(Action::MoveFloating { panel: first.clone(), rect: [resized.left(), resized.top(), resized.width(), resized.height()] });
            }
            if response.drag_stopped() {
                response.surrender_focus();
            }
            if cancelled || !permitted || !ui.input(|input| input.pointer.primary_down()) {
                ui.ctx().data_mut(|data| {
                    data.remove::<AnchoredResize>(anchor_id);
                    if data.get_temp::<Id>(self.id.with("pointer-resize")) == Some(response.id) {
                        data.remove::<Id>(self.id.with("pointer-resize"));
                    }
                });
            }
            if name == "resize" && (response.hovered() || response.dragged() || response.has_focus()) {
                ui.painter().line_segment([handle.left_bottom(), handle.right_top()], Stroke::new(2.0, self.style.accent));
            }
        }
    }
}
