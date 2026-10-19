//! Drive a [`DockArea`] with real egui input in application tests (feature `testing`).
//!
//! ```ignore
//! let mut driver = DockDriver::new(app_layout(), Panels::default());
//! let bar = driver.hit(&Hit::DragBar(PanelId::Layers)).unwrap();
//! driver.drag(bar.center(), egui::pos2(4.0, 300.0));
//! assert!(driver.layout.dock_path(Side::Start).is_some());
//! ```

use std::{fmt::Debug, hash::Hash};

use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, UiBuilder, epaint::ClippedShape, vec2};

use super::{DockArea, DockContent, DockOutput, DockStyle, Hit, Layout};

/// Pointer travel that starts a drag before moving to its destination.
const NUDGE: egui::Vec2 = egui::Vec2::new(8.0, 6.0);

type StyleFn = Box<dyn Fn(&mut DockStyle)>;

/// A [`DockArea`] in a headless egui context, with its layout and content. Each frame advances
/// the clock by 1/60 s, so double-clicks register. [`Self::step`] applies what a frame emits,
/// the way an application's command path would.
pub struct DockDriver<P, C> {
    pub ctx: Context,
    pub layout: Layout<P>,
    pub content: C,
    /// The area's id, for [`DockArea::hit_rects`] and the free docking functions.
    pub id: Id,
    /// The window, 800x600 by default.
    pub screen: Rect,
    /// Where the area draws inside the window; the whole window by default.
    pub area: Rect,
    /// Whether the window has focus. Losing it cancels gestures (C2).
    pub focused: bool,
    /// Modifiers held for the following events.
    pub modifiers: Modifiers,
    /// Passed to [`DockArea::auto_collapse_iconic`].
    pub auto_collapse: bool,
    /// Passed to [`DockArea::maximized`].
    pub maximized: Option<P>,
    /// Passed to [`DockArea::raise_panel`] on the next frame only.
    pub raise: Option<P>,
    style: Option<StyleFn>,
    shapes: Vec<ClippedShape>,
    time: f64,
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static, C: DockContent<P>> DockDriver<P, C> {
    /// A driver whose first frames have settled, so every control has its rect.
    pub fn new(layout: Layout<P>, content: C) -> Self {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let mut driver = Self {
            ctx: Context::default(),
            layout,
            content,
            id: Id::new("dock-driver"),
            screen,
            area: screen,
            focused: true,
            modifiers: Modifiers::NONE,
            auto_collapse: true,
            maximized: None,
            raise: None,
            style: None,
            shapes: Vec::new(),
            time: 0.0,
        };
        driver.settle();
        driver
    }

    /// Adjust the style each frame, after [`DockStyle::from_ui`]. Settles again.
    pub fn with_style(mut self, style: impl Fn(&mut DockStyle) + 'static) -> Self {
        self.style = Some(Box::new(style));
        self.settle();
        self
    }

    /// Draw in `area` of the window. Settles again.
    pub fn with_area(mut self, area: Rect) -> Self {
        self.area = area;
        self.settle();
        self
    }

    /// Render one frame with `events` and return its output. Nothing is applied.
    pub fn frame(&mut self, events: Vec<Event>) -> DockOutput<P> {
        let mut output = DockOutput::default();
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(self.screen),
            events: std::iter::once(Event::ModifiersChanged(self.modifiers)).chain(events).collect(),
            focused: self.focused,
            time: Some(self.time),
            ..Default::default()
        };
        let (layout, content, area, id) = (&self.layout, &mut self.content, self.area, self.id);
        let (style_fn, maximized, raise, auto) = (&self.style, self.maximized.clone(), self.raise.take(), self.auto_collapse);
        let mut full = self.ctx.run_ui(input, |ui| {
            let mut style = DockStyle::from_ui(ui);
            if let Some(adjust) = style_fn {
                adjust(&mut style);
            }
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                let mut dock = DockArea::new(id).maximized(maximized.clone()).auto_collapse_iconic(auto);
                if let Some(panel) = raise.clone() {
                    dock = dock.raise_panel(panel);
                }
                output = dock.show_content(ui, layout, &style, content);
            });
        });
        full.textures_delta.clear();
        self.shapes = full.shapes;
        output
    }

    /// Render one frame and apply its actions, all or none, with the content's permissions. A
    /// rejected batch is reported in the output's `error`.
    pub fn step(&mut self, events: Vec<Event>) -> DockOutput<P> {
        let mut output = self.frame(events);
        if output.error.is_none()
            && let Err(error) = self.layout.apply_all(output.actions.clone(), |p| self.content.permissions(p))
        {
            output.error = Some(error);
        }
        output
    }

    /// Empty frames until moved widgets report their rects; egui reports them a frame late.
    pub fn settle(&mut self) {
        for _ in 0..3 {
            self.step(Vec::new());
        }
    }

    pub fn move_to(&mut self, pos: Pos2) -> DockOutput<P> {
        self.step(vec![Event::PointerMoved(pos)])
    }

    pub fn press(&mut self, pos: Pos2) -> DockOutput<P> {
        self.step(vec![Event::PointerMoved(pos), self.button(pos, true)])
    }

    pub fn release(&mut self, pos: Pos2) -> DockOutput<P> {
        self.step(vec![Event::PointerMoved(pos), self.button(pos, false)])
    }

    /// Press and release at `pos`; the output is the release frame's.
    pub fn click(&mut self, pos: Pos2) -> DockOutput<P> {
        self.press(pos);
        self.release(pos)
    }

    /// Press at `from`, move past the drag threshold and on to `to`, without releasing.
    pub fn hold(&mut self, from: Pos2, to: Pos2) -> DockOutput<P> {
        self.press(from);
        self.move_to(from + NUDGE);
        self.move_to(to)
    }

    /// A whole drag; the output is the release frame's.
    pub fn drag(&mut self, from: Pos2, to: Pos2) -> DockOutput<P> {
        self.hold(from, to);
        self.release(to)
    }

    /// Press and release `key` in one frame.
    pub fn key(&mut self, key: Key) -> DockOutput<P> {
        let modifiers = self.modifiers;
        self.step(vec![
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
        ])
    }

    /// Give or take the window's focus for this and the following frames.
    pub fn focus(&mut self, focused: bool) -> DockOutput<P> {
        self.focused = focused;
        self.step(vec![Event::WindowFocused(focused)])
    }

    /// The pointer leaves the window.
    pub fn leave(&mut self) -> DockOutput<P> {
        self.step(vec![Event::PointerGone])
    }

    /// Every gesture source of the last frame.
    pub fn hits(&self) -> Vec<(Hit<P>, Rect)> {
        DockArea::<P>::hit_rects(&self.ctx, self.id)
    }

    /// The rect of one gesture source in the last frame.
    pub fn hit(&self, hit: &Hit<P>) -> Option<Rect> {
        self.hits().into_iter().find(|(h, _)| h == hit).map(|(_, rect)| rect)
    }

    /// The shapes of the last frame.
    pub fn shapes(&self) -> &[ClippedShape] {
        &self.shapes
    }

    /// Whether the last frame filled `rect` (an insertion line or a split preview).
    pub fn filled(&self, rect: Rect) -> bool {
        self.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => shape.rect == rect && shape.fill != egui::Color32::TRANSPARENT,
            _ => false,
        })
    }

    /// Whether the last frame outlined `rect` without filling it.
    pub fn outlined(&self, rect: Rect) -> bool {
        self.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => shape.rect == rect && shape.fill == egui::Color32::TRANSPARENT && shape.stroke.width > 0.0,
            _ => false,
        })
    }

    fn button(&self, pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: self.modifiers }
    }
}
