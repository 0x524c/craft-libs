use std::{fmt::Debug, hash::Hash};

use egui::{Context, Key, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::{Renderer, clamp_float};
use crate::docking::{Action, Node};

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    pub(in crate::docking) fn floating(&mut self, ctx: &Context) {
        let screen = ctx.content_rect();
        if !crate::docking::targets::usable(screen) {
            return;
        }
        let layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
        let mut groups: Vec<_> = self.layout.floating.iter().collect();
        groups.sort_by_key(|group| {
            let id = group.panels().next().map(|panel| self.id.with(("floating", panel)));
            layers.iter().position(|layer| Some(layer.id) == id).unwrap_or(usize::MAX)
        });
        for group in groups {
            let Some(first) = group.panels().next() else { continue };
            let [x, y, width, height] = group.rect;
            let limits = self.node_limits(&Node::Tabs { panels: group.panels().cloned().collect(), active: 0 });
            let size = vec2(width, height).max(limits.min).min(limits.max);
            let rect = clamp_float(Rect::from_min_size(pos2(x, y), size), screen);
            let id = self.id.with(("floating", first));
            if self.activation.as_ref().is_some_and(|panel| group.contains(panel)) {
                ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
                ctx.memory_mut(|memory| memory.request_focus(self.id.with(("tab", self.activation.as_ref().unwrap_or(first)))));
                self.output.focused = self.activation.clone();
            }
            egui::Area::new(id).order(egui::Order::Middle).fixed_pos(rect.min).movable(false).constrain_to(screen).show(ctx, |ui| {
                ui.set_min_size(rect.size());
                let rect = Rect::from_min_size(ui.min_rect().min, rect.size());
                ui.allocate_rect(rect, Sense::hover());
                self.stacked_groups(ui, &group.groups, rect, true);
                let corner = Rect::from_min_max((rect.max - vec2(12.0, 12.0)).max(rect.min), rect.max);
                let response = ui.interact(corner, id.with("resize"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeNwSe);
                self.register_interaction(ui.ctx(), response.id);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.resize_window_label));
                if response.drag_started() {
                    response.request_focus();
                }
                let mut delta = if response.dragged() { ui.input(|i| i.pointer.delta()) } else { Vec2::ZERO };
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
                if response.enabled() && delta != Vec2::ZERO {
                    let size = (rect.size() + delta).max(limits.min.max(vec2(40.0, self.style.tab_height + 20.0))).min(limits.max).min(screen.size());
                    self.output.actions.push(Action::MoveFloating { panel: first.clone(), rect: [rect.left(), rect.top(), size.x, size.y] });
                }
                if response.hovered() || response.dragged() || response.has_focus() {
                    ui.painter().line_segment([corner.left_bottom(), corner.right_top()], Stroke::new(2.0, self.style.accent));
                }
            });
        }
    }
}
