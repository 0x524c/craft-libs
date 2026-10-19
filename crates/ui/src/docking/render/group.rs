use std::{fmt::Debug, hash::Hash};

use egui::{Key, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use super::{Renderer, clamp_float, stripless};
use crate::docking::{Action, DropTarget, Group, GroupDrag, GroupGesture, Placement, Target, begin_drag, surface::TAB_PADDING};
use crate::tabs;

/// Where a group is drawn. It decides the drop targets the group offers and its gestures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::docking) enum GroupKind {
    /// A `Tabs` node of the split tree: side drops split it.
    Frame,
    /// A group in a dock column.
    Dock,
    /// A group in the floating window drawn at `window`.
    Floating { window: Rect },
}

/// The group a tab belongs to, for its menu and minimize gestures.
pub(super) struct TabGroup<'a, P> {
    pub(super) panels: &'a [P],
    pub(super) selected: &'a P,
    pub(super) minimized: bool,
    pub(super) kind: GroupKind,
    pub(super) rect: Rect,
}

/// The top band of a strip that drags the whole group.
const BORDER_BAND: f32 = 4.0;

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    /// Groups stacked top to bottom. Minimized groups take their strip; the others share the
    /// rest equally.
    pub(super) fn stacked_groups(&mut self, ui: &mut Ui, groups: &[Group<P>], rect: Rect, kind: GroupKind) {
        let strip = self.style.tab_height;
        let open = groups.iter().filter(|g| !g.minimized).count();
        let fixed = groups.len().saturating_sub(open) as f32 * strip;
        let share = if open > 0 { (rect.height() - fixed).max(0.0) / open as f32 } else { 0.0 };
        let mut top = rect.top();
        for group in groups {
            let bottom = (top + if group.minimized { strip } else { share }).min(rect.bottom());
            let group_rect = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), bottom));
            self.group(ui, &group.panels, group.active, group.minimized, group_rect, kind);
            top = bottom;
        }
    }

    pub(super) fn group(&mut self, ui: &mut Ui, panels: &[P], active: usize, minimized: bool, rect: Rect, kind: GroupKind) {
        let Some(selected) = panels.get(active) else { return };
        let policy = self.permissions(selected);
        let splits = kind == GroupKind::Frame;
        if stripless(panels, &*self.content) {
            self.contents(ui, selected, rect);
            self.targets.push(DropTarget {
                anchor: selected.clone(),
                rect,
                tab_strip: None,
                tabs: vec![(selected.clone(), Rect::NOTHING)],
                allow_splits: splits,
                accepts_tabs: policy.accepts_tabs,
            });
            return;
        }
        let movable = panels.iter().all(|p| self.permissions(p).movable);
        let strip = Rect::from_min_max(rect.min, pos2(rect.right(), (rect.top() + self.style.tab_height).min(rect.bottom())));
        ui.painter().rect_filled(strip, 0.0, self.style.tab_background);
        let floating = matches!(kind, GroupKind::Floating { .. });
        let requested = self.content.group_header_width(panels, selected, floating);
        let header_width = if requested.is_finite() { requested.clamp(0.0, strip.width()) } else { 0.0 };
        if header_width > 0.0 {
            let header = Rect::from_min_max(pos2(strip.right() - header_width, strip.top()), strip.max);
            let mut child = ui.new_child(UiBuilder::new().id(self.id.with(("header", selected))).max_rect(header));
            child.set_clip_rect(header.intersect(ui.clip_rect()));
            self.content.group_header(&mut child, panels, selected, floating);
        }
        // The drag bar keeps its reserved width: tabs shrink, then overflow, before it narrows.
        let bar_width = self.style.drag_bar_width.clamp(0.0, (strip.width() - header_width).max(0.0));
        let tabs_width = (strip.width() - header_width - bar_width).max(0.0);
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
        let fit = tabs::fit(&widths, active, tabs_width, 40.0, 22.0);
        let owner = TabGroup { panels, selected, minimized, kind, rect };
        let mut x = strip.left();
        let mut shown = Vec::new();
        for (index, width) in &fit.shown {
            let Some(panel) = panels.get(*index) else { continue };
            let tab = Rect::from_min_size(pos2(x, strip.top()), vec2(*width, strip.height()));
            self.tab(ui, panel, tab, *index == active, rect, Some(&owner));
            shown.push((panel.clone(), tab));
            x += width;
        }
        if !fit.overflow.is_empty() && fit.overflow_width > 0.0 {
            let overflow = Rect::from_min_size(pos2(x, strip.top()), vec2(fit.overflow_width, strip.height()));
            let response = ui.interact(overflow, self.id.with(("overflow", selected)), Sense::click());
            self.register_interaction(ui.ctx(), response.id);
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
            x += fit.overflow_width;
        }
        let bar = Rect::from_min_max(pos2(x.min(strip.right() - header_width), strip.top()), pos2(strip.right() - header_width, strip.bottom()));
        self.group_handle(ui, "drag-bar", bar, &owner, movable);
        // Registered last, so the band wins over the tabs below it.
        let border = Rect::from_min_max(strip.min, pos2(strip.right() - header_width, (strip.top() + BORDER_BAND).min(strip.bottom())));
        self.group_handle(ui, "drag-border", border, &owner, movable);
        // Logical order, hidden tabs included without a hit area, for no-op detection.
        let tabs =
            panels.iter().map(|panel| (panel.clone(), shown.iter().find(|(p, _)| p == panel).map_or(Rect::NOTHING, |(_, rect)| *rect))).collect();
        self.targets.push(DropTarget {
            anchor: selected.clone(),
            rect,
            tab_strip: Some(strip),
            tabs,
            allow_splits: splits,
            accepts_tabs: policy.accepts_tabs,
        });
        if !minimized {
            self.contents(ui, selected, Rect::from_min_max(pos2(rect.left(), strip.bottom()), rect.max));
        }
        ui.painter().rect_stroke(rect, 0.0, self.style.border, StrokeKind::Inside);
    }

    /// The drag bar (`drag-bar`: click, double-click, menu and drag) or the top border band
    /// (`drag-border`: drag only). Both move the whole group.
    fn group_handle(&mut self, ui: &mut Ui, name: &'static str, hit: Rect, owner: &TabGroup<'_, P>, movable: bool) {
        if !crate::docking::targets::usable(hit) {
            return;
        }
        let bar = name == "drag-bar";
        let sense = if bar { Sense::click_and_drag() } else { Sense::drag() };
        let response = ui.interact(hit, self.id.with((name, owner.panels.first())), sense);
        let response = if movable { response.on_hover_cursor(egui::CursorIcon::Grab) } else { response };
        self.register_interaction(ui.ctx(), response.id);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.style.move_group_label));
        if movable && response.drag_started() {
            self.start_group_drag(ui, owner.panels, self.drag_source(owner));
        }
        if !bar {
            return;
        }
        if owner.kind != GroupKind::Frame && response.double_clicked() {
            self.output.actions.push(Action::SetMinimized { panel: owner.selected.clone(), minimized: !owner.minimized });
        }
        if response.has_focus() || response.hovered() {
            ui.painter().with_clip_rect(hit).rect_stroke(
                hit.shrink(1.0),
                0.0,
                Stroke::new(1.0, self.style.accent.gamma_multiply(0.5)),
                StrokeKind::Inside,
            );
        }
        let keyboard = response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
        let command = if response.secondary_clicked() || keyboard {
            Some(egui::SetOpenCommand::Bool(true))
        } else if response.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::menu(&response).open_memory(command).show(|ui| self.group_menu(ui, owner, owner.selected));
    }

    fn start_group_drag(&mut self, ui: &Ui, panels: &[P], rect: Rect) {
        let Some(first) = panels.first() else { return };
        let origin = ui.input(|i| i.pointer.press_origin()).or_else(|| ui.ctx().pointer_interact_pos()).unwrap_or(rect.center());
        let drag = GroupDrag {
            panel: first.clone(),
            original: [rect.left(), rect.top(), rect.width(), rect.height()],
            gesture: GroupGesture::Group,
            members: panels.to_vec(),
            origin,
            source: rect,
        };
        ui.ctx().data_mut(|d| d.insert_temp(self.id.with("group-drag"), drag));
    }

    /// The group items of the menu tail: Close Tab Group, Minimize or Expand, Float Tab Group
    /// and Group with. `panel` is the panel whose menu this is.
    fn group_menu(&mut self, ui: &mut Ui, owner: &TabGroup<'_, P>, panel: &P) {
        let all = |check: &dyn Fn(crate::docking::Permissions) -> bool| owner.panels.iter().all(|p| check(self.permissions(p)));
        let closable = all(&|p| p.closable);
        let movable = all(&|p| p.movable);
        let floatable = movable && all(&|p| p.floatable);
        if closable && self.menu_action(ui, &self.close_group_label()) {
            self.output.actions.push(Action::CloseGroup { panel: panel.clone() });
        }
        if owner.kind != GroupKind::Frame {
            let label = if owner.minimized { self.style.expand_label.clone() } else { self.style.minimize_label.clone() };
            if self.menu_action(ui, &label) {
                self.output.actions.push(Action::SetMinimized { panel: panel.clone(), minimized: !owner.minimized });
            }
        }
        if floatable && !matches!(owner.kind, GroupKind::Floating { .. }) && self.menu_action(ui, &self.float_group_label()) {
            let rect = clamp_float(owner.rect.translate(vec2(24.0, 24.0)), ui.ctx().content_rect());
            self.output.actions.push(Action::MoveGroup {
                panel: panel.clone(),
                to: Target::Float { rect: Some([rect.left(), rect.top(), rect.width(), rect.height()]) },
            });
        }
        let targets = if movable { self.group_targets(owner.panels) } else { Vec::new() };
        if !targets.is_empty() {
            let logical = self.style.group_with_label.clone();
            let text = self.content.action_label(ui, &logical, &self.style.font, self.style.text);
            let response = ui.menu_button(text, |ui| {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for target in &targets {
                        if self.menu_label(ui, target).clicked() {
                            self.output.actions.push(Action::MoveGroup {
                                panel: panel.clone(),
                                to: Target::At { anchor: target.clone(), placement: Placement::Tab { before: None } },
                            });
                            ui.close();
                        }
                    }
                });
            });
            response.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &logical));
        }
        self.frame_items(ui, owner, panel);
    }

    /// A stock menu button with shaped text and its logical accessibility label; true when
    /// clicked, after closing the menu.
    pub(super) fn menu_action(&mut self, ui: &mut Ui, logical: &str) -> bool {
        let text = self.content.action_label(ui, logical, &self.style.font, self.style.text);
        let response = ui.button(text);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), logical));
        if response.clicked() {
            ui.close();
        }
        response.clicked()
    }

    /// One anchor per other group that accepts a whole group as tabs, in layout order.
    fn group_targets(&self, members: &[P]) -> Vec<P> {
        let Some(first) = members.first() else { return Vec::new() };
        let mut places = Vec::new();
        let mut anchors = Vec::new();
        for anchor in self.layout.move_targets(first, |p| self.permissions(p)) {
            if members.contains(anchor) {
                continue;
            }
            let Some(place) = self.layout.place(anchor) else { continue };
            if !places.contains(&place) {
                places.push(place);
                anchors.push(anchor.clone());
            }
        }
        anchors
    }

    pub(super) fn menu_label(&mut self, ui: &mut Ui, panel: &P) -> egui::Response {
        let logical = self.label_of(panel);
        let text = self.content.label(ui, panel, &logical, &self.style.font, self.style.text);
        let response = ui.button(text);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &logical));
        response
    }

    pub(super) fn tab(&mut self, ui: &mut Ui, panel: &P, rect: Rect, active: bool, group: Rect, owner: Option<&TabGroup<'_, P>>) {
        let minimized = owner.filter(|owner| owner.kind != GroupKind::Frame).map(|owner| owner.minimized);
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
            if minimized == Some(true) && !active {
                self.output.actions.push(Action::SetMinimized { panel: panel.clone(), minimized: false });
            }
        }
        if let Some(minimized) = minimized
            && response.double_clicked()
        {
            self.output.actions.push(Action::SetMinimized { panel: panel.clone(), minimized: !minimized });
        }
        if policy.movable && response.drag_started() && begin_drag(ui.ctx(), self.id, panel.clone(), group) {
            self.latch_undock(ui.ctx(), owner.map(|owner| owner.kind));
        }
        response.context_menu(|ui| {
            self.content.panel_menu(ui, panel);
            if policy.movable && policy.floatable {
                let logical = self.float_label();
                let text = self.content.action_label(ui, &logical, &self.style.font, self.style.text);
                let response = ui.button(text);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &logical));
                if response.clicked() {
                    let size = self.float_size(panel, group.size());
                    let rect = clamp_float(Rect::from_min_size(group.min + vec2(24.0, 24.0), size), ui.ctx().content_rect());
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
            if let Some(owner) = owner {
                ui.separator();
                self.group_menu(ui, owner, panel);
            }
        });
    }
}
