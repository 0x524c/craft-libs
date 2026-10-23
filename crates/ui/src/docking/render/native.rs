//! Floating windows in their own OS windows (feature `native-windows`). Where the backend has
//! viewports, every floating window whose panels allow it is drawn with
//! `show_viewport_immediate`, always on top of other windows and hidden while none of the
//! application's windows has focus. Where egui embeds viewports (wasm, tests) windows stay
//! in-window at their in-window rect. Tabs and groups do not drag between OS windows.

use std::{fmt::Debug, hash::Hash};

use egui::{Context, Event, Id, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, WindowLevel, pos2, vec2};

use super::Renderer;
use crate::docking::chrome::{ChromeStyle, TitleBar};
use crate::docking::placement::{monitor_of, restore, sanitized};
use crate::docking::{Action, Drag, Floating, GroupDrag};

/// Smallest native window.
const MIN_SIZE: [f32; 2] = [160.0, 100.0];
/// Height of the bar drawn at the top of a native window.
pub(in crate::docking) const BAR_HEIGHT: f32 = 18.0;
/// How long floating windows stay after the application's last window lost focus, so focus
/// moving between our own windows does not hide them.
const HIDE_DELAY: f64 = 0.25;

/// A drag of a native window's bar, in the context's data under the area's `"window-drag"`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::docking) struct WindowDrag {
    /// [`Floating::id`] of the dragged window.
    pub window: u64,
    /// Where the pointer pressed, in the window's local points.
    pub grab: Vec2,
    /// The window's outer position on the desktop: reported when the drag began, then as last
    /// commanded. The reported position lags and jitters while the window moves.
    pub origin: Pos2,
}

/// The viewport of the native window `window` of the dock area `area`. The `n`th window with
/// an id an earlier window of the same frame took gets its own.
pub(in crate::docking) fn viewport_id(area: Id, window: u64, n: usize) -> ViewportId {
    match n {
        0 => ViewportId::from_hash_of((area, "native", window)),
        n => ViewportId::from_hash_of((area, "native", window, n)),
    }
}

/// Whether floating windows show: while one of the application's windows has focus, and until
/// [`HIDE_DELAY`] after `last_focused`, when one last had it.
pub(in crate::docking) fn floats_shown(focused: bool, now: f64, last_focused: f64) -> bool {
    focused || now - last_focused < HIDE_DELAY
}

/// The desktop rect a native viewport reports: outer position, inner size. None when the
/// backend reports nothing usable.
fn placed(outer: Option<Rect>, inner: Option<Rect>) -> Option<[f32; 4]> {
    let (outer, inner) = (outer?, inner?);
    (outer.min.is_finite() && inner.size().is_finite()).then(|| sanitized([outer.min.x, outer.min.y, inner.width(), inner.height()]))
}

/// What a native window's frame reports back to the main window.
#[derive(Default)]
struct Report {
    closed: bool,
    placed: Option<[f32; 4]>,
    monitor_size: Option<Vec2>,
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    /// Show the native windows and return the windows to draw in-window.
    pub(super) fn native_windows<'w>(&mut self, ctx: &Context, windows: Vec<&'w Floating<P>>) -> Vec<&'w Floating<P>> {
        let native = !ctx.embed_viewports();
        let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).filter(|p| p.is_finite()).unwrap_or(Pos2::ZERO);
        let visibility = if native { self.float_visibility(ctx) } else { (true, false) };
        let mut shown = Vec::new();
        let mut in_window = Vec::new();
        let mut ids = Vec::new();
        for window in windows {
            let allowed = window.panels().all(|p| self.content.allow_native(p) && self.permissions(p).floatable);
            let Some(first) = window.panels().next().cloned().filter(|_| native && allowed) else {
                in_window.push(window);
                continue;
            };
            // Every float is native: one still in-window opens over its place in the app window.
            let rect = window.native.unwrap_or_else(|| {
                let [x, y, w, h] = window.rect;
                let rect = sanitized([origin.x + x, origin.y + y, w, h]);
                self.output.actions.push(Action::SetNative { panel: first, rect: Some(rect) });
                rect
            });
            let n = ids.iter().filter(|id| **id == window.id).count();
            ids.push(window.id);
            let viewport = viewport_id(self.id, window.id, n);
            if let Some(key) = self.native_window(ctx, window, rect, viewport, visibility) {
                shown.push(key);
            }
        }
        // Forget where windows that are no longer native were told to open.
        let key = self.id.with("native-shown");
        let previous = ctx.data_mut(|d| d.remove_temp::<Vec<Id>>(key)).unwrap_or_default();
        ctx.data_mut(|d| {
            for id in previous.iter().filter(|id| !shown.contains(id)) {
                d.remove::<[f32; 4]>(*id);
            }
            d.insert_temp(key, shown);
        });
        in_window
    }

    /// Whether floating windows show this frame, and whether that changed since the last.
    fn float_visibility(&mut self, ctx: &Context) -> (bool, bool) {
        let (focused, now) = ctx.input(|i| {
            let infos = || i.raw.viewports.values();
            // A backend that reports no focus at all keeps the windows.
            let focused = i.focused || infos().any(|v| v.focused == Some(true)) || infos().all(|v| v.focused.is_none());
            (focused, i.time)
        });
        let key = self.id.with("native-visibility");
        let (last, was) = ctx.data(|d| d.get_temp::<(f64, bool)>(key)).unwrap_or((now, true));
        let last = if focused { now } else { last };
        let visible = floats_shown(focused, now, last);
        if visible && !focused {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((HIDE_DELAY - (now - last)).max(0.0)));
        }
        ctx.data_mut(|d| d.insert_temp(key, (last, visible)));
        (visible, visible != was)
    }

    /// One floating window in its own viewport. Returns the key of its spawn rect.
    fn native_window(&mut self, ctx: &Context, window: &Floating<P>, rect: [f32; 4], viewport: ViewportId, visibility: (bool, bool)) -> Option<Id> {
        let (visible, toggled) = visibility;
        let first = window.panels().next()?.clone();
        // The window opens at its saved place, moved onto a connected monitor; after that the
        // window system owns its position and size, which are read back below.
        let spawn_key = self.id.with(("native-spawn", viewport));
        let spawned = ctx.data(|d| d.get_temp::<[f32; 4]>(spawn_key));
        let spawn = spawned.unwrap_or_else(|| {
            let (main, size) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().monitor_size));
            restore(rect, window.monitor.as_ref(), &self.monitors, main, size)
        });
        ctx.data_mut(|d| d.insert_temp(spawn_key, spawn));
        let title = crate::docking::dock_model::first_active(&window.root).map(|p| self.label_of(p)).unwrap_or_default();
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_position([spawn[0], spawn[1]])
            .with_inner_size([spawn[2], spawn[3]])
            .with_min_inner_size(MIN_SIZE)
            .with_window_level(WindowLevel::AlwaysOnTop);
        // The bar replaces the OS title bar. macOS keeps its frame for edge resizing.
        #[cfg(target_os = "macos")]
        let builder = builder.with_titlebar_shown(false).with_title_shown(false).with_fullsize_content_view(true).with_titlebar_buttons_shown(false);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_decorations(false);
        if toggled || (spawned.is_none() && !visible) {
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Visible(visible));
        }
        if visible && self.activation.as_ref().is_some_and(|panel| window.contains(panel)) {
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        }
        let (targets, windows) = (self.targets.len(), self.frame.windows.len());
        let report = ctx.show_viewport_immediate(viewport, builder, |ui, class| {
            let full = ui.max_rect();
            ui.painter().rect_filled(full, 0.0, self.style.floating_background);
            self.native_bar(ui, window);
            let body = Rect::from_min_max(pos2(full.left(), (full.top() + BAR_HEIGHT).min(full.bottom())), full.max);
            self.floating_body(ui, window, body, full);
            #[cfg(not(target_os = "macos"))]
            if window.panels().all(|p| self.permissions(p).movable) {
                resize_edges(ui, self.id.with(("native-edges", window.id)), full);
            }
            // An embedded fallback reads the main window's input: its close request, place and
            // keys are not this window's.
            if !matches!(class, ViewportClass::Immediate | ViewportClass::Deferred) {
                return Report::default();
            }
            self.forward_events(ui);
            ui.input(|i| Report {
                closed: i.viewport().close_requested(),
                placed: placed(i.viewport().outer_rect, i.viewport().inner_rect),
                monitor_size: i.viewport().monitor_size,
            })
        });
        // Targets in another OS window are in its coordinates, and no drag crosses windows.
        self.targets.truncate(targets);
        self.frame.windows.truncate(windows);
        ctx.data_mut(|d| {
            if d.get_temp::<Drag<P>>(self.id.with("drag")).is_some_and(|drag| window.contains(&drag.panel)) {
                d.remove::<Drag<P>>(self.id.with("drag"));
            }
            if d.get_temp::<GroupDrag<P>>(self.id.with("group-drag")).is_some_and(|drag| window.contains(&drag.panel)) {
                d.remove::<GroupDrag<P>>(self.id.with("group-drag"));
            }
        });
        if report.closed {
            // Like the bar's ×: its panels close and reopen from the application's menu.
            self.output.actions.push(Action::CloseWindow { panel: first });
            return Some(spawn_key);
        }
        let now = report.placed.unwrap_or(rect);
        if let Some(placed) = report.placed.filter(|placed| placed.iter().zip(rect).any(|(a, b)| (a - b).abs() > 0.5)) {
            self.output.actions.push(Action::SetNative { panel: first.clone(), rect: Some(placed) });
            // The in-window rect follows, for builds that draw the window in-window.
            if window.panels().all(|p| self.permissions(p).movable) {
                let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).filter(|p| p.is_finite()).unwrap_or(Pos2::ZERO);
                let [x, y, w, h] = placed;
                self.output.actions.push(Action::MoveFloating { panel: first.clone(), rect: [x - origin.x, y - origin.y, w, h] });
            }
        }
        let monitor = monitor_of(&self.monitors, now).cloned();
        let size = match monitor {
            Some(_) => None,
            None => report.monitor_size.filter(|s| s.is_finite() && s.x > 0.0 && s.y > 0.0).map(|s| [s.x, s.y]),
        };
        if (monitor.is_some() || size.is_some()) && (monitor != window.monitor || size != window.monitor_size) {
            self.output.actions.push(Action::SetMonitor { panel: first, monitor, monitor_size: size });
        }
        Some(spawn_key)
    }

    /// The bar at the top of a native window: its title and ×. Dragging it moves the window.
    fn native_bar(&mut self, ui: &mut Ui, window: &Floating<P>) {
        let Some(first) = window.panels().next().cloned() else { return };
        let full = ui.max_rect();
        let rect = Rect::from_min_size(full.min, vec2(full.width(), BAR_HEIGHT.min(full.height())));
        let movable = window.panels().all(|p| self.permissions(p).movable);
        let closable = self.style.close_button && window.panels().all(|p| self.permissions(p).closable);
        let (move_label, close_label) = (self.style.move_window_label.clone(), self.style.close_window_label.clone());
        let title = crate::docking::dock_model::first_active(&window.root).map(|p| self.label_of(p)).unwrap_or_default();
        let mut bar = TitleBar::new(self.id.with(("native-bar", window.id)), &move_label).title(title).movable(movable);
        if closable {
            bar = bar.close(&close_label);
        }
        let response = bar.show(ui, rect, &ChromeStyle::from_dock(self.style));
        if let Some(close) = response.close
            && close.clicked()
        {
            self.output.actions.push(Action::CloseWindow { panel: first });
        }
        if movable {
            self.drag_window(ui, &response.bar, window.id);
        }
    }

    /// Move the window with its bar: by `OuterPosition` from the position last commanded, or,
    /// where the window system reports no position (Wayland), by handing the move to it.
    fn drag_window(&mut self, ui: &Ui, bar: &egui::Response, window: u64) {
        let key = self.id.with("window-drag");
        let pointer = ui.input(|i| i.pointer.latest_pos());
        if bar.drag_started() {
            let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
            match (ui.input(|i| i.viewport().outer_rect), press) {
                (Some(outer), Some(press)) if outer.min.is_finite() => {
                    let drag = WindowDrag { window, grab: press.to_vec2(), origin: outer.min };
                    ui.ctx().data_mut(|d| d.insert_temp(key, drag));
                }
                _ => ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag),
            }
        }
        let Some(mut drag) = ui.ctx().data(|d| d.get_temp::<WindowDrag>(key)).filter(|d| d.window == window) else { return };
        if bar.dragged()
            && let Some(pointer) = pointer
        {
            // The pointer on the desktop is the commanded origin plus the local pointer.
            let step = pointer.to_vec2() - drag.grab;
            if step != Vec2::ZERO {
                drag.origin += step;
                ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(drag.origin));
                ui.ctx().data_mut(|d| d.insert_temp(key, drag));
            }
        }
        if bar.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            ui.ctx().data_mut(|d| d.remove::<WindowDrag>(key));
            self.end_window_drag(drag);
        }
    }

    /// Release of a bar drag. The window already sits where it was dragged.
    fn end_window_drag(&mut self, _drag: WindowDrag) {}

    /// Keys, text and clipboard events the window's widgets left over, for the application's
    /// shortcuts. Nothing while a text field of the window has focus.
    fn forward_events(&mut self, ui: &Ui) {
        if ui.ctx().text_edit_focused() {
            return;
        }
        let events = ui.input(|i| {
            i.events
                .iter()
                .filter(|e| matches!(e, Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)))
                .cloned()
                .collect::<Vec<_>>()
        });
        self.output.forwarded_events.extend(events);
    }
}

/// Bands along the edges and corners of an undecorated window that resize it.
#[cfg(not(target_os = "macos"))]
fn resize_edges(ui: &Ui, id: Id, rect: Rect) {
    use egui::{CursorIcon as C, ResizeDirection as D, Sense};
    const EDGE: f32 = 4.0;
    let (e, c) = (EDGE, EDGE * 2.0);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let zones = [
        (D::NorthWest, Rect::from_min_max(pos2(l, t), pos2(l + c, t + c)), C::ResizeNorthWest),
        (D::NorthEast, Rect::from_min_max(pos2(r - c, t), pos2(r, t + c)), C::ResizeNorthEast),
        (D::SouthWest, Rect::from_min_max(pos2(l, b - c), pos2(l + c, b)), C::ResizeSouthWest),
        (D::SouthEast, Rect::from_min_max(pos2(r - c, b - c), pos2(r, b)), C::ResizeSouthEast),
        (D::North, Rect::from_min_max(pos2(l + c, t), pos2(r - c, t + e)), C::ResizeNorth),
        (D::South, Rect::from_min_max(pos2(l + c, b - e), pos2(r - c, b)), C::ResizeSouth),
        (D::West, Rect::from_min_max(pos2(l, t + c), pos2(l + e, b - c)), C::ResizeWest),
        (D::East, Rect::from_min_max(pos2(r - e, t + c), pos2(r, b - c)), C::ResizeEast),
    ];
    for (i, (direction, zone, cursor)) in zones.into_iter().enumerate() {
        if !zone.is_positive() {
            continue;
        }
        let response = ui.interact(zone, id.with(i), Sense::drag()).on_hover_cursor(cursor);
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}
