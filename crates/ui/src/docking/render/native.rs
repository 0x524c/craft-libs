//! Floating windows in their own OS windows (feature `native-windows`). Where the backend has
//! viewports, every floating window whose panels allow it is drawn with
//! `show_viewport_immediate`, always on top of other windows and hidden while none of the
//! application's windows has focus. Where egui embeds viewports (wasm, tests) windows stay
//! in-window at their in-window rect. Tabs, groups and whole windows drag between OS windows
//! (see `cross.rs`).

use std::{fmt::Debug, hash::Hash};

use egui::{Context, Event, Id, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, WindowLevel, pos2, vec2};

use super::Renderer;
use super::cross::{DragPointer, Surface};
use crate::docking::chrome::{ChromeStyle, TitleBar};
use crate::docking::placement::{monitor_of, restore, sanitized};
use crate::docking::{Action, Drag, Floating, GroupDrag, GroupGesture, Node};

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
    /// Where the window was when the drag began, for Escape.
    pub start: Pos2,
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
struct Report<P> {
    closed: bool,
    placed: Option<[f32; 4]>,
    monitor_size: Option<Vec2>,
    /// Its drop targets, for drags from other windows.
    surface: Option<Surface<P>>,
}

impl<P> Default for Report<P> {
    fn default() -> Self {
        Self { closed: false, placed: None, monitor_size: None, surface: None }
    }
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    /// Show the native windows and return the windows to draw in-window.
    pub(super) fn native_windows<'w>(&mut self, ctx: &Context, windows: Vec<&'w Floating<P>>) -> Vec<&'w Floating<P>> {
        let native = !ctx.embed_viewports();
        let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).filter(|p| p.is_finite()).unwrap_or(Pos2::ZERO);
        let visibility = if native { self.float_visibility(ctx) } else { (true, false) };
        self.cross.on = native;
        let mut shown = Vec::new();
        let mut in_window = Vec::new();
        let mut ids = Vec::new();
        let mut natives = Vec::new();
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
            natives.push((window, rect, viewport_id(self.id, window.id, n)));
        }
        if native {
            self.map_desktop(ctx, &natives, visibility.0);
            // The main window owns drags of the panels it draws.
            let id = self.id;
            let dragged = ctx.data(|d| {
                d.get_temp::<Drag<P>>(id.with("drag")).map(|d| d.panel).or(d.get_temp::<GroupDrag<P>>(id.with("group-drag")).map(|d| d.panel))
            });
            if dragged.is_some_and(|panel| !natives.iter().any(|(w, ..)| w.contains(&panel))) {
                let origin = self.cross.main.map(|m| m.min);
                self.record_pointer(ctx, None, origin, None);
            }
        }
        for (window, rect, viewport) in natives {
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

    /// The main window and the shown native windows on the desktop, front first: the window
    /// focused most recently first. A window whose position is unknown is left out.
    fn map_desktop(&mut self, ctx: &Context, natives: &[(&Floating<P>, [f32; 4], ViewportId)], visible: bool) {
        let key = self.id.with("native-order");
        let mut order = ctx.data(|d| d.get_temp::<Vec<u64>>(key)).unwrap_or_default();
        order.retain(|id| natives.iter().any(|(w, ..)| w.id == *id));
        let moving = ctx.data(|d| d.get_temp::<WindowDrag>(self.id.with("window-drag")));
        let mut windows = Vec::new();
        for (window, rect, viewport) in natives {
            let info = ctx.input(|i| i.raw.viewports.get(viewport).cloned());
            // A new window opens in front.
            if info.as_ref().is_some_and(|i| i.focused == Some(true)) || !order.contains(&window.id) {
                order.retain(|id| *id != window.id);
                order.insert(0, window.id);
            }
            let [x, y, w, h] = *rect;
            let placed = match info {
                None => Some(Rect::from_min_size(pos2(x, y), vec2(w, h))),
                Some(info) => info.outer_rect.zip(info.inner_rect).map(|(outer, inner)| {
                    let min = moving.filter(|m| m.window == window.id).map_or(outer.min, |m| m.origin);
                    Rect::from_min_size(min, inner.size())
                }),
            };
            if let Some(placed) = placed.filter(|r| r.is_finite() && visible) {
                windows.push((window.id, placed));
            }
        }
        windows.sort_by_key(|(id, _)| order.iter().position(|o| o == id));
        ctx.data_mut(|d| d.insert_temp(key, order));
        self.cross.windows = windows;
        self.cross.main = ctx.input(|i| i.viewport().outer_rect.and(i.viewport().inner_rect)).filter(|r| r.is_finite());
    }

    /// Whether floating windows show this frame, and whether that changed since the last.
    fn float_visibility(&mut self, ctx: &Context) -> (bool, bool) {
        let (focused, now) = ctx.input(|i| {
            let infos = || i.raw.viewports.values();
            // A backend that reports no focus at all keeps the windows.
            let focused = i.focused || infos().any(|v| v.focused == Some(true)) || infos().all(|v| v.focused.is_none());
            (focused, i.time)
        });
        self.cross.focused = focused;
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
        let (targets, docks, windows) = (self.targets.len(), self.dock_targets.len(), self.frame.windows.len());
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
            let mut surface = Surface {
                window: window.id,
                origin: None,
                bounds: full,
                targets: self.targets.split_off(targets),
                dock_targets: self.dock_targets.split_off(docks),
                windows: self.frame.windows.split_off(windows),
            };
            surface.origin = self.window_gestures(ui, window);
            self.paint_cross(ui, &mut surface);
            self.forward_events(ui);
            ui.input(|i| Report {
                closed: i.viewport().close_requested(),
                placed: placed(i.viewport().outer_rect, i.viewport().inner_rect),
                monitor_size: i.viewport().monitor_size,
                surface: Some(surface),
            })
        });
        // Targets in another OS window are in its coordinates.
        self.targets.truncate(targets);
        self.dock_targets.truncate(docks);
        self.frame.windows.truncate(windows);
        self.cross.surfaces.extend(report.surface);
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
        if movable && response.bar.drag_started() {
            self.start_window_drag(ui, window.id);
        }
    }

    /// After a native window's tree drew: a drag of its only tab, or of its only group, moves
    /// the window as its bar does. Then the window follows a bar drag, and writes the pointer
    /// of a drag it owns. Returns the window's desktop origin, where known.
    fn window_gestures(&mut self, ui: &Ui, window: &Floating<P>) -> Option<Pos2> {
        let ctx = ui.ctx();
        let (drag_key, group_key) = (self.id.with("drag"), self.id.with("group-drag"));
        let tab = ctx.data(|d| d.get_temp::<Drag<P>>(drag_key)).filter(|d| window.contains(&d.panel));
        let group = ctx.data(|d| d.get_temp::<GroupDrag<P>>(group_key)).filter(|d| window.contains(&d.panel));
        let lone = window.panels().count() == 1 && tab.is_some();
        let whole = matches!(window.root, Node::Tabs { .. }) && group.as_ref().is_some_and(|g| g.gesture == GroupGesture::Group);
        if (lone || whole) && window.panels().all(|p| self.permissions(p).movable) {
            ctx.data_mut(|d| {
                d.remove::<Drag<P>>(drag_key);
                d.remove::<GroupDrag<P>>(group_key);
            });
            self.start_window_drag(ui, window.id);
        }
        let outer = ui.input(|i| i.viewport().outer_rect).map(|r| r.min).filter(|p| p.is_finite());
        if let Some(origin) = self.follow_window_drag(ui, window.id) {
            return Some(origin);
        }
        let owns = ctx.data(|d| d.get_temp::<Drag<P>>(drag_key).map(|d| d.panel).or(d.get_temp::<GroupDrag<P>>(group_key).map(|d| d.panel)));
        if owns.is_some_and(|panel| window.contains(&panel)) {
            self.record_pointer(ctx, Some(window.id), outer, None);
        }
        outer
    }

    /// Start moving the window with the pointer, from the position the window system reports,
    /// or, where it reports none (Wayland), by handing the move to it.
    fn start_window_drag(&mut self, ui: &Ui, window: u64) {
        let press = ui.input(|i| i.pointer.press_origin().or(i.pointer.latest_pos()));
        match (ui.input(|i| i.viewport().outer_rect), press) {
            (Some(outer), Some(press)) if outer.min.is_finite() => {
                let drag = WindowDrag { window, grab: press.to_vec2(), origin: outer.min, start: outer.min };
                ui.ctx().data_mut(|d| d.insert_temp(self.id.with("window-drag"), drag));
            }
            _ => ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag),
        }
    }

    /// Move the window by `OuterPosition` from the position last commanded, while the button
    /// is held. Escape puts it back; losing focus leaves it. Returns the desktop origin of
    /// this frame's input while the window is moving.
    fn follow_window_drag(&mut self, ui: &Ui, window: u64) -> Option<Pos2> {
        let key = self.id.with("window-drag");
        let mut drag = ui.ctx().data(|d| d.get_temp::<WindowDrag>(key)).filter(|d| d.window == window)?;
        let origin = drag.origin;
        let pointer = self.record_pointer(ui.ctx(), Some(window), Some(origin), Some(window));
        if pointer.escape || !self.cross.focused {
            if pointer.escape {
                ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(drag.start));
            }
            ui.ctx().data_mut(|d| d.remove::<WindowDrag>(key));
            return Some(origin);
        }
        if !pointer.down {
            ui.ctx().data_mut(|d| d.remove::<WindowDrag>(key));
            self.end_window_drag(drag, pointer);
            return Some(origin);
        }
        // The pointer on the desktop is the commanded origin plus the local pointer.
        let step = ui.input(|i| i.pointer.latest_pos()).filter(|p| p.is_finite()).map_or(Vec2::ZERO, |p| p.to_vec2() - drag.grab);
        if step != Vec2::ZERO {
            drag.origin += step;
            ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(drag.origin));
            ui.ctx().data_mut(|d| d.insert_temp(key, drag));
        }
        Some(origin)
    }

    /// Release of a bar drag. The window already sits where it was dragged; over a target in
    /// another window it docks there once every window has drawn (`cross.rs`).
    fn end_window_drag(&mut self, drag: WindowDrag, pointer: DragPointer) {
        self.cross.release = Some((drag, pointer));
    }

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
