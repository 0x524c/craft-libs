use std::{fmt::Debug, hash::Hash};

use egui::{Key, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use super::{Renderer, clamp_float};
use crate::docking::{Action, DropTarget, GroupDrag, Permissions, Placement, begin_drag, surface::TAB_PADDING};
use crate::tabs;

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    /// Groups stacked top to bottom in equal shares of `rect`.
    pub(super) fn stacked_groups(&mut self, ui: &mut Ui, groups: &[crate::docking::Group<P>], rect: Rect, floating: bool) {
        let share = rect.height() / groups.len().max(1) as f32;
        for (index, group) in groups.iter().enumerate() {
            let top = rect.top() + share * index as f32;
            let group_rect = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), top + share));
            self.group(ui, &group.panels, group.active, group_rect, floating);
        }
    }

    pub(super) fn group(&mut self, ui: &mut Ui, panels: &[P], active: usize, rect: Rect, floating: bool) {
        let Some(selected) = panels.get(active) else { return };
        let policy = self.permissions(selected);
        let bare = panels.len() == 1 && policy == Permissions::PROTECTED;
        if bare {
            self.contents(ui, selected, rect);
            self.targets.push(DropTarget {
                anchor: selected.clone(),
                rect,
                tab_strip: None,
                tabs: Vec::new(),
                allow_splits: !floating,
                accepts_tabs: false,
            });
            return;
        }
        let mut strip = Rect::from_min_max(rect.min, pos2(rect.right(), (rect.top() + self.style.tab_height).min(rect.bottom())));
        ui.painter().rect_filled(strip, 0.0, self.style.tab_background);
        if floating {
            let grip = Rect::from_min_max(strip.min, pos2((strip.left() + 12.0).min(strip.right()), strip.bottom()));
            let response = ui.interact(grip, self.id.with(("move-group", panels.first())), Sense::drag()).on_hover_cursor(egui::CursorIcon::Grab);
            self.register_interaction(ui.ctx(), response.id);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.move_label));
            if panels.iter().all(|p| self.permissions(p).movable)
                && response.drag_started()
                && let Some(first) = panels.first()
            {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        self.id.with("group-drag"),
                        GroupDrag { panel: first.clone(), original: [rect.left(), rect.top(), rect.width(), rect.height()] },
                    )
                });
            }
            if panels.iter().all(|p| self.permissions(p).movable)
                && response.dragged()
                && ui.ctx().data(|d| d.get_temp::<GroupDrag<P>>(self.id.with("group-drag"))).is_some()
                && !ui.input(|i| i.key_pressed(Key::Escape) || !i.focused)
            {
                let moved = clamp_float(rect.translate(ui.input(|i| i.pointer.delta())), ui.ctx().content_rect());
                if let Some(first) = panels.first() {
                    self.output
                        .actions
                        .push(Action::MoveFloating { panel: first.clone(), rect: [moved.left(), moved.top(), moved.width(), moved.height()] });
                }
            }
            ui.painter().line_segment([grip.center() - vec2(0.0, 5.0), grip.center() + vec2(0.0, 5.0)], self.style.border);
            strip.min.x = grip.right();
        }
        let requested = self.content.group_header_width(panels, selected, floating);
        let header_width = if requested.is_finite() { requested.clamp(0.0, strip.width()) } else { 0.0 };
        if header_width > 0.0 {
            let header = Rect::from_min_max(pos2(strip.right() - header_width, strip.top()), strip.max);
            let mut child = ui.new_child(UiBuilder::new().id(self.id.with(("header", selected))).max_rect(header));
            child.set_clip_rect(header.intersect(ui.clip_rect()));
            self.content.group_header(&mut child, panels, selected, floating);
        }
        let widths: Vec<f32> = panels
            .iter()
            .map(|p| {
                self.content
                    .label(ui, p, &self.label_of(p), &self.style.font, self.style.text)
                    .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, self.style.font.clone())
                    .size()
                    .x
                    + TAB_PADDING
            })
            .collect();
        let fit = tabs::fit(&widths, active, (strip.width() - header_width).max(0.0), 40.0, 22.0);
        let mut x = strip.left();
        let mut targets = Vec::new();
        for (index, width) in &fit.shown {
            let Some(panel) = panels.get(*index) else { continue };
            let tab = Rect::from_min_size(pos2(x, strip.top()), vec2(*width, strip.height()));
            self.tab(ui, panel, tab, *index == active, rect);
            targets.push((panel.clone(), tab));
            x += width;
        }
        if !fit.overflow.is_empty() && fit.overflow_width > 0.0 {
            let overflow = Rect::from_min_size(pos2(x, strip.top()), vec2(fit.overflow_width, strip.height()));
            let response = ui.interact(overflow, self.id.with(("overflow", selected)), Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.panels_label));
            ui.painter().with_clip_rect(overflow).text(overflow.center(), egui::Align2::CENTER_CENTER, "…", self.style.font.clone(), self.style.text);
            egui::Popup::menu(&response).show(|ui| {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for index in &fit.overflow {
                        if let Some(panel) = panels.get(*index)
                            && self.menu_label(ui, panel).clicked()
                        {
                            self.output.actions.push(Action::Activate { panel: panel.clone() });
                            self.output.focused = Some(panel.clone());
                            ui.close();
                        }
                    }
                });
            });
        }
        // Include hidden tab identities for source-group detection without creating hit areas.
        for panel in panels {
            if !targets.iter().any(|(p, _)| p == panel) {
                targets.push((panel.clone(), Rect::NOTHING));
            }
        }
        self.targets.push(DropTarget {
            anchor: selected.clone(),
            rect,
            tab_strip: Some(strip),
            tabs: targets,
            allow_splits: !floating,
            accepts_tabs: policy.accepts_tabs,
        });
        self.contents(ui, selected, Rect::from_min_max(pos2(rect.left(), strip.bottom()), rect.max));
        ui.painter().rect_stroke(rect, 0.0, self.style.border, StrokeKind::Inside);
    }

    pub(super) fn menu_label(&mut self, ui: &mut Ui, panel: &P) -> egui::Response {
        let logical = self.label_of(panel);
        let text = self.content.label(ui, panel, &logical, &self.style.font, self.style.text);
        let response = ui.button(text);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &logical));
        response
    }

    pub(super) fn tab(&mut self, ui: &mut Ui, panel: &P, rect: Rect, active: bool, group: Rect) {
        let policy = self.permissions(panel);
        let label = self.label_of(panel);
        let color = if active { self.style.text } else { self.style.inactive_text };
        // A tab squeezed below its natural width cuts its label with '…' inside the padding its
        // natural width was measured with, instead of letting it run under its neighbours.
        let galley = self.content.label(ui, panel, &label, &self.style.font, color).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - TAB_PADDING).max(0.0),
            self.style.font.clone(),
        );
        let elided = galley.elided;
        let response = tabs::Tab::new(self.id.with(("tab", panel)), &label, active)
            .sense(if policy.movable { Sense::click_and_drag() } else { Sense::click() })
            .focus_stroke(Stroke::new(1.0, self.style.accent))
            .show_at(ui, rect, |ui, response| {
                let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                painter.rect_filled(rect, 0.0, if active || response.hovered() { self.style.active_background } else { self.style.tab_background });
                painter.galley(rect.center() - galley.size() * 0.5, galley.clone(), color);
            });
        let response = if elided { response.on_hover_text(&label) } else { response };
        self.register_interaction(ui.ctx(), response.id);
        if response.clicked() {
            self.output.actions.push(Action::Activate { panel: panel.clone() });
            self.output.focused = Some(panel.clone());
        }
        if policy.movable && response.drag_started() {
            begin_drag(ui.ctx(), self.id, panel.clone(), group);
        }
        response.context_menu(|ui| {
            self.content.panel_menu(ui, panel);
            if policy.movable && policy.floatable {
                let text = self.content.action_label(ui, &self.style.float_label, &self.style.font, self.style.text);
                let response = ui.button(text);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.float_label));
                if response.clicked() {
                    let rect = clamp_float(group.translate(vec2(24.0, 24.0)), ui.ctx().content_rect());
                    self.output.actions.push(Action::Float { panel: panel.clone(), rect: [rect.left(), rect.top(), rect.width(), rect.height()] });
                    ui.close();
                }
            }
            let targets: Vec<P> =
                if policy.movable { self.layout.move_targets(panel, |p| self.permissions(p)).into_iter().cloned().collect() } else { Vec::new() };
            if !targets.is_empty() {
                let text = self.content.action_label(ui, &self.style.move_label, &self.style.font, self.style.text);
                let response = ui.menu_button(text, |ui| {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for target in &targets {
                            if self.menu_label(ui, target).clicked() {
                                self.output.actions.push(Action::Move {
                                    panel: panel.clone(),
                                    anchor: target.clone(),
                                    placement: Placement::Tab { before: None },
                                });
                                ui.close();
                            }
                        }
                    });
                });
                response.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.move_label));
            }
            if policy.closable {
                let text = self.content.action_label(ui, &self.style.close_label, &self.style.font, self.style.text);
                let response = ui.button(text);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.close_label));
                if response.clicked() {
                    self.output.actions.push(Action::Close { panel: panel.clone() });
                    ui.close();
                }
            }
        });
    }
}
