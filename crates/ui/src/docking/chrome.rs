//! Floating window chrome: an opaque frame with anchored resize handles, and a title bar with a
//! grip, an optional ▾ menu button and ×. [`super::DockArea`] draws its floating windows with
//! these; other hosts (task panes) use them with their own ids and actions. Nothing here changes
//! a layout: the caller turns the responses into its own commands.

use egui::{Color32, Context, FontId, Id, Key, LayerId, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetText, pos2, vec2};

use super::{DockStyle, PanelLimits, resize::AnchoredResize};

/// Height of a floating window's title bar.
pub const TITLE_HEIGHT: f32 = 14.0;

/// Names of the resize handles, appended to the frame id: `id.with(name)`.
pub const RESIZE_HANDLES: [&str; 5] = ["resize", "resize-left", "resize-right", "resize-bottom", "resize-bottom-left"];

/// Colours and font of the frame and title bar.
#[derive(Clone, Debug)]
pub struct ChromeStyle {
    /// Painted under the contents, so the window stays opaque when the contents are not.
    pub background: Color32,
    pub stroke: Stroke,
    pub title_background: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub accent: Color32,
    pub font: FontId,
}

impl ChromeStyle {
    pub fn from_ui(ui: &Ui) -> Self {
        Self::from_dock(&DockStyle::from_ui(ui))
    }

    pub fn from_dock(style: &DockStyle) -> Self {
        Self {
            background: style.floating_background,
            stroke: style.floating_stroke,
            title_background: style.tab_background,
            text: style.text,
            weak_text: style.inactive_text,
            accent: style.accent,
            font: style.font.clone(),
        }
    }
}

/// Resize handle hit rects and the edges they move: both sides, the bottom and both bottom
/// corners. The top edge belongs to the title bar.
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

/// An opaque in-window frame on its own `Order::Middle` layer, `id`, at `rect`.
pub struct FloatingFrame<'a> {
    id: Id,
    rect: Rect,
    bounds: Option<Rect>,
    limits: PanelLimits,
    resizable: bool,
    resize_label: &'a str,
}

/// What a [`FloatingFrame`] did this frame.
pub struct FrameResponse<R> {
    pub inner: R,
    /// Where the frame was drawn, inside its bounds.
    pub rect: Rect,
    pub layer: LayerId,
    /// The rect a resize handle asks for: pointer drags anchored at the opposite edges, or
    /// arrow keys on a focused handle (8 pt per press). Escape and focus loss end a resize.
    pub resized: Option<Rect>,
    /// The handle whose pointer resize started this frame.
    pub resize_started: Option<Id>,
    /// Every handle's response, for focus and gesture bookkeeping.
    pub handles: Vec<Response>,
}

impl<'a> FloatingFrame<'a> {
    pub fn new(id: Id, rect: Rect) -> Self {
        Self { id, rect, bounds: None, limits: PanelLimits::default(), resizable: true, resize_label: "Resize window" }
    }

    /// The frame stays inside `bounds`; the default is the viewport's content rect.
    pub fn bounds(mut self, bounds: Rect) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Size limits of a resize. The frame itself is drawn at the given rect.
    pub fn limits(mut self, limits: PanelLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Accessibility label of the resize handles.
    pub fn resize_label(mut self, label: &'a str) -> Self {
        self.resize_label = label;
        self
    }

    pub fn layer(id: Id) -> LayerId {
        LayerId::new(Order::Middle, id)
    }

    /// Draw the frame, then `add_contents` with the frame's rect, then the outline and handles.
    pub fn show<R>(self, ctx: &Context, style: &ChromeStyle, add_contents: impl FnOnce(&mut Ui, Rect) -> R) -> FrameResponse<R> {
        let bounds = self.bounds.unwrap_or_else(|| ctx.content_rect());
        let rect = clamp(self.rect, bounds);
        let area = egui::Area::new(self.id)
            .fade_in(false)
            .order(Order::Middle)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .movable(false)
            .constrain_to(bounds);
        let shown = area.show(ctx, |ui| {
            // Drawn at `rect` itself, also in egui's first (sizing) pass of a new area.
            ui.set_min_size(rect.size());
            ui.allocate_rect(rect, Sense::hover());
            ui.painter().rect_filled(rect, 0.0, style.background);
            let inner = add_contents(ui, rect);
            ui.painter().rect_stroke(rect, 0.0, style.stroke, StrokeKind::Outside);
            let (resized, resize_started, handles) = if self.resizable { self.handles(ui, rect, bounds, style) } else { (None, None, Vec::new()) };
            FrameResponse { inner, rect, layer: Self::layer(self.id), resized, resize_started, handles }
        });
        shown.inner
    }

    fn handles(&self, ui: &mut Ui, rect: Rect, bounds: Rect, style: &ChromeStyle) -> (Option<Rect>, Option<Id>, Vec<Response>) {
        let mut resized = None;
        let mut started = None;
        let mut responses = Vec::new();
        let limits = PanelLimits { min: self.limits.min.max(vec2(40.0, TITLE_HEIGHT + 20.0)).min(self.limits.max), ..self.limits };
        let cancelled = ui.input(|input| input.key_pressed(Key::Escape) || !input.focused);
        for (name, handle, direction) in handles(rect) {
            if !super::targets::usable(handle) {
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
            let response = ui.interact(handle, self.id.with(name), Sense::drag()).on_hover_cursor(cursor);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.resize_label));
            let anchor_id = response.id.with("pointer-anchor");
            if response.drag_started() {
                response.request_focus();
                if let Some(origin) = ui.input(|i| i.pointer.press_origin())
                    && let Some(anchor) = AnchoredResize::new(rect, origin, direction)
                {
                    ui.ctx().data_mut(|data| data.insert_temp(anchor_id, anchor));
                    started = Some(response.id);
                }
            }
            let mut delta = Vec2::ZERO;
            if response.has_focus() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
                    )
                });
                ui.input_mut(|input| {
                    for (key, step) in [
                        (Key::ArrowLeft, vec2(-8.0, 0.0)),
                        (Key::ArrowRight, vec2(8.0, 0.0)),
                        (Key::ArrowUp, vec2(0.0, -8.0)),
                        (Key::ArrowDown, vec2(0.0, 8.0)),
                    ] {
                        if input.consume_key(egui::Modifiers::NONE, key) {
                            delta += step;
                        }
                    }
                });
            }
            let next = if response.dragged() || response.drag_stopped() {
                ui.ctx()
                    .data(|data| data.get_temp::<AnchoredResize>(anchor_id))
                    .and_then(|anchor| ui.ctx().pointer_interact_pos().and_then(|pointer| anchor.rect(pointer, bounds, limits)))
            } else if delta != Vec2::ZERO {
                AnchoredResize::new(rect, rect.max, direction).and_then(|anchor| anchor.rect(rect.max + delta, bounds, limits))
            } else {
                None
            };
            if response.enabled()
                && !cancelled
                && let Some(next) = next.filter(|next| *next != rect)
            {
                resized = Some(next);
            }
            if response.drag_stopped() {
                response.surrender_focus();
            }
            if cancelled || !ui.input(|input| input.pointer.primary_down()) {
                ui.ctx().data_mut(|data| data.remove::<AnchoredResize>(anchor_id));
            }
            if name == "resize" && (response.hovered() || response.dragged() || response.has_focus()) {
                ui.painter().line_segment([handle.left_bottom(), handle.right_top()], Stroke::new(2.0, style.accent));
            }
            responses.push(response);
        }
        (resized, started, responses)
    }
}

/// Keep `rect` inside `bounds`, shrinking it when it is larger.
pub fn clamp(rect: Rect, bounds: Rect) -> Rect {
    if !super::targets::usable(bounds) {
        return rect;
    }
    let size = rect.size().max(vec2(1.0, 1.0)).min(bounds.size());
    let min = rect.min.max(bounds.min).min(bounds.max - size);
    Rect::from_min_size(min, size)
}

/// A title bar: two grip lines (or a title), an optional ▾ menu button and an optional ×, 10 pt
/// in from the right edge. The bar senses clicks and drags; double-clicks are the caller's.
pub struct TitleBar<'a> {
    id: Id,
    move_label: &'a str,
    title: Option<WidgetText>,
    menu: Option<&'a str>,
    close: Option<&'a str>,
    movable: bool,
}

/// The parts of a [`TitleBar`] that were shown.
pub struct TitleResponse {
    /// Drag to move; double-click is the caller's (docking minimizes every group).
    pub bar: Response,
    pub menu: Option<Response>,
    pub close: Option<Response>,
}

impl<'a> TitleBar<'a> {
    /// `id` is the bar's own; the buttons use `id.with("menu")` and `id.with("close")`.
    /// `move_label` names the bar for accessibility.
    pub fn new(id: Id, move_label: &'a str) -> Self {
        Self { id, move_label, title: None, menu: None, close: None, movable: true }
    }

    /// Text at the left instead of the grip lines.
    pub fn title(mut self, title: impl Into<WidgetText>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Show a ▾ button with this accessibility label.
    pub fn menu(mut self, label: &'a str) -> Self {
        self.menu = Some(label);
        self
    }

    /// Show a × button with this accessibility label.
    pub fn close(mut self, label: &'a str) -> Self {
        self.close = Some(label);
        self
    }

    /// Whether the bar shows the grab cursor. It senses drags either way.
    pub fn movable(mut self, movable: bool) -> Self {
        self.movable = movable;
        self
    }

    pub fn show(self, ui: &mut Ui, rect: Rect, style: &ChromeStyle) -> TitleResponse {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0.0, style.title_background);
        let side = rect.height();
        let close_rect = self.close.map(|_| Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(side, side)));
        let menu_right = close_rect.map_or(rect.right() - 3.0, |close| close.left() - 2.0);
        let menu_rect = self.menu.map(|_| Rect::from_min_max(pos2(menu_right - side, rect.top()), pos2(menu_right, rect.bottom())));
        let buttons = menu_rect.or(close_rect).map_or(rect.right(), |r| r.left());
        let bar_rect = Rect::from_min_max(rect.min, pos2(buttons.max(rect.left()), rect.bottom()));
        let bar = ui.interact(bar_rect, self.id, Sense::click_and_drag());
        let bar = if self.movable { bar.on_hover_cursor(egui::CursorIcon::Grab) } else { bar };
        bar.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.move_label));
        if let Some(title) = self.title {
            let galley = title.into_galley(ui, Some(egui::TextWrapMode::Truncate), (bar_rect.width() - 12.0).max(0.0), style.font.clone());
            painter.galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y * 0.5), galley, style.text);
        } else {
            let grip = Stroke::new(0.6, style.weak_text);
            for k in 0..2 {
                let y = rect.center().y - 1.5 + k as f32 * 3.0;
                painter.line_segment([pos2(rect.center().x - 9.0, y), pos2(rect.center().x + 9.0, y)], grip);
            }
        }
        if bar.has_focus() {
            painter.rect_stroke(bar_rect.shrink(1.0), 0.0, Stroke::new(1.0, style.accent), StrokeKind::Inside);
        }
        let button = |ui: &mut Ui, rect: Rect, name: &str, label: &str| {
            let response = ui.interact(rect, self.id.with(name), Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
            let color = if response.hovered() || response.has_focus() { style.text } else { style.weak_text };
            (response, Stroke::new(1.2, color))
        };
        let menu = menu_rect.zip(self.menu).map(|(menu_rect, label)| {
            let (response, stroke) = button(ui, menu_rect, "menu", label);
            let c = menu_rect.center();
            painter.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5), c + vec2(0.0, 2.5)], stroke.color, Stroke::NONE));
            response
        });
        let close = close_rect.zip(self.close).map(|(close_rect, label)| {
            let (response, stroke) = button(ui, close_rect, "close", label);
            let r = Rect::from_center_size(close_rect.center(), Vec2::splat((side * 0.5).max(4.0)));
            painter.line_segment([r.left_top(), r.right_bottom()], stroke);
            painter.line_segment([r.right_top(), r.left_bottom()], stroke);
            response
        });
        TitleResponse { bar, menu, close }
    }
}
