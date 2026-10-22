//! Floating windows in their own OS windows (feature `native-windows`). A window with a native
//! rect is drawn with `show_viewport_immediate` where the backend has viewports; where egui
//! embeds viewports (wasm, tests) it stays in-window at its in-window rect. Tabs and groups do
//! not drag between OS windows.

use std::{fmt::Debug, hash::Hash};

use egui::{Context, Pos2, Rect, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, vec2};

use super::{Renderer, clamp_float, group::TabGroup};
use crate::docking::{Action, Drag, Floating, GroupDrag};

/// Smallest and largest native window, and the farthest it may sit from the desktop origin.
const MIN_SIZE: [f32; 2] = [160.0, 100.0];
const MAX_SIZE: f32 = 16_384.0;
const MAX_POSITION: f32 = 100_000.0;

/// A rect safe to hand to the windowing system: finite, its size and position bounded.
/// Hostile workspace files and bad monitor reports included.
pub(super) fn sanitized(rect: [f32; 4]) -> [f32; 4] {
    let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
    let [x, y, w, h] = rect;
    [
        finite(x, 0.0).clamp(-MAX_POSITION, MAX_POSITION),
        finite(y, 0.0).clamp(-MAX_POSITION, MAX_POSITION),
        finite(w, 420.0).clamp(MIN_SIZE[0], MAX_SIZE),
        finite(h, 320.0).clamp(MIN_SIZE[1], MAX_SIZE),
    ]
}

/// The desktop rect a native viewport reports: outer position, inner size. None when the
/// backend reports nothing usable.
fn placed(outer: Option<Rect>, inner: Option<Rect>) -> Option<[f32; 4]> {
    let (outer, inner) = (outer?, inner?);
    (outer.min.is_finite() && inner.size().is_finite()).then(|| sanitized([outer.min.x, outer.min.y, inner.width(), inner.height()]))
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> Renderer<'_, P> {
    /// Show the native windows and return the windows to draw in-window.
    pub(super) fn native_windows<'w>(&mut self, ctx: &Context, windows: Vec<&'w Floating<P>>) -> Vec<&'w Floating<P>> {
        let native = !ctx.embed_viewports();
        let mut shown = Vec::new();
        let mut in_window = Vec::new();
        for window in windows {
            match window.native {
                Some(rect) if native && window.panels().all(|p| self.content.allow_native(p)) => {
                    if let Some(id) = self.native_window(ctx, window, rect) {
                        shown.push(id);
                    }
                }
                _ => in_window.push(window),
            }
        }
        // Forget where windows that are no longer native were told to open.
        let key = self.id.with("native-shown");
        let previous = ctx.data_mut(|d| d.remove_temp::<Vec<egui::Id>>(key)).unwrap_or_default();
        ctx.data_mut(|d| {
            for id in previous.iter().filter(|id| !shown.contains(id)) {
                d.remove::<[f32; 4]>(*id);
            }
            d.insert_temp(key, shown);
        });
        in_window
    }

    /// One floating window in its own viewport. Returns the key of its spawn rect.
    fn native_window(&mut self, ctx: &Context, window: &Floating<P>, rect: [f32; 4]) -> Option<egui::Id> {
        let first = window.panels().next()?.clone();
        let viewport = ViewportId::from_hash_of((self.id, "native", &first));
        // The window opens at its saved rect; after that the window system owns its position
        // and size, which are read back below.
        let spawn_key = self.id.with(("native-spawn", &first));
        let spawn = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(spawn_key, || sanitized(rect)));
        let title = crate::docking::dock_model::first_active(&window.root).map(|p| self.label_of(p)).unwrap_or_default();
        let builder = ViewportBuilder::default()
            .with_title(title)
            .with_position([spawn[0], spawn[1]])
            .with_inner_size([spawn[2], spawn[3]])
            .with_min_inner_size(MIN_SIZE);
        if self.activation.as_ref().is_some_and(|panel| window.contains(panel)) {
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        }
        let (targets, windows) = (self.targets.len(), self.frame.windows.len());
        let (closed, placed) = ctx.show_viewport_immediate(viewport, builder, |ui, class| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, self.style.floating_background);
            self.floating_body(ui, window, rect, rect);
            // An embedded fallback reads the main window's input: its close request and place
            // are not this window's.
            if matches!(class, ViewportClass::Immediate | ViewportClass::Deferred) {
                ui.input(|i| (i.viewport().close_requested(), placed(i.viewport().outer_rect, i.viewport().inner_rect)))
            } else {
                (false, None)
            }
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
        if closed {
            ctx.data_mut(|d| d.remove::<[f32; 4]>(spawn_key));
            self.output.actions.push(Action::SetNative { panel: first, rect: None });
            return None;
        }
        if let Some(placed) = placed.filter(|placed| placed.iter().zip(rect).any(|(a, b)| (a - b).abs() > 0.5)) {
            self.output.actions.push(Action::SetNative { panel: first, rect: Some(placed) });
        }
        Some(spawn_key)
    }

    /// Move Panel to New Window: a floating panel's window, or the panel floated on its own.
    pub(super) fn new_window_item(&mut self, ui: &mut egui::Ui, owner: &TabGroup<'_, P>, panel: &P) {
        let ctx = ui.ctx().clone();
        if ctx.embed_viewports() {
            return;
        }
        let window = self.layout.floating.iter().find(|w| w.contains(panel));
        if window.is_some_and(|w| w.native.is_some()) {
            return;
        }
        let movers: Vec<P> = window.map_or_else(|| vec![panel.clone()], |w| w.panels().cloned().collect());
        let allowed = |p: &P| self.content.allow_native(p) && self.permissions(p).floatable && self.permissions(p).movable;
        if !movers.iter().all(allowed) {
            return;
        }
        let in_window = match window {
            Some(window) => window.rect,
            None => {
                let size = self.float_size(panel, owner.rect.size());
                let rect = clamp_float(Rect::from_min_size(owner.rect.min + vec2(24.0, 24.0), size), self.float_bounds);
                [rect.left(), rect.top(), rect.width(), rect.height()]
            }
        };
        if !self.menu_action(ui, &self.style.new_window_label.clone()) {
            return;
        }
        // Over the panel's place in the app window: its content's position on the desktop.
        let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).filter(|p| p.is_finite()).unwrap_or(Pos2::ZERO);
        let [x, y, w, h] = in_window;
        if window.is_none() {
            self.output.actions.push(Action::Float { panel: panel.clone(), rect: in_window });
        }
        let native = sanitized([origin.x + x, origin.y + y, w, h]);
        self.output.actions.push(Action::SetNative { panel: panel.clone(), rect: Some(native) });
    }
}
