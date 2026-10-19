use std::{fmt::Debug, hash::Hash};

use egui::{Color32, Context, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use super::{
    Action, DockError, DropTarget, Layout, Node, Permissions,
    render::{RESIZE_HANDLES, Renderer},
    resize::AnchoredResize,
};

/// Horizontal room around a tab's label, both when measuring its natural width and when cutting it.
pub(super) const TAB_PADDING: f32 = 24.0;

/// Application-supplied chrome. Body renderers retain their own UI style and contents.
#[derive(Clone, Debug)]
pub struct DockStyle {
    pub tab_height: f32,
    pub gap: f32,
    pub min_pane: f32,
    pub background: Color32,
    pub tab_background: Color32,
    pub active_background: Color32,
    pub text: Color32,
    pub inactive_text: Color32,
    pub border: Stroke,
    pub accent: Color32,
    pub font: FontId,
    /// Localize these action labels in the application adapter.
    pub float_label: String,
    pub close_label: String,
    pub move_label: String,
    pub panels_label: String,
    pub resize_label: String,
    pub resize_window_label: String,
    /// Accessibility label of a group's drag bar and top border.
    pub move_group_label: String,
    pub close_group_label: String,
    pub minimize_label: String,
    pub expand_label: String,
    pub float_group_label: String,
    pub group_with_label: String,
    /// Minimum width of the drag bar left of the group header. Tabs shrink, then overflow,
    /// before the bar narrows.
    pub drag_bar_width: f32,
    /// Painted under every floating window, so it stays opaque when the panel colours are not.
    pub floating_background: Color32,
    pub floating_stroke: Stroke,
}

impl DockStyle {
    pub fn from_ui(ui: &Ui) -> Self {
        let visuals = ui.visuals();
        Self {
            tab_height: 26.0,
            gap: 4.0,
            min_pane: 40.0,
            background: visuals.panel_fill,
            tab_background: visuals.extreme_bg_color,
            active_background: visuals.widgets.active.bg_fill,
            text: visuals.text_color(),
            inactive_text: visuals.weak_text_color(),
            border: visuals.widgets.noninteractive.bg_stroke,
            accent: visuals.selection.stroke.color,
            font: FontId::proportional(12.0),
            float_label: "Float panel".into(),
            close_label: "Close panel".into(),
            move_label: "Move to group".into(),
            panels_label: "Panels".into(),
            resize_label: "Resize panels".into(),
            resize_window_label: "Resize panel window".into(),
            move_group_label: "Move tab group".into(),
            close_group_label: "Close Tab Group".into(),
            minimize_label: "Minimize".into(),
            expand_label: "Expand".into(),
            float_group_label: "Float Tab Group".into(),
            group_with_label: "Group with".into(),
            drag_bar_width: 40.0,
            floating_background: visuals.window_fill,
            floating_stroke: visuals.window_stroke,
        }
    }

    pub(super) fn valid(&self) -> bool {
        [self.tab_height, self.gap, self.min_pane, self.border.width, self.font.size, self.drag_bar_width, self.floating_stroke.width]
            .iter()
            .all(|v| v.is_finite() && (0.0..=4096.0).contains(v))
            && self.tab_height > 0.0
            && self.font.size > 0.0
    }
}

#[derive(Clone, Debug)]
pub struct DockOutput<P> {
    pub actions: Vec<Action<P>>,
    pub error: Option<DockError>,
    pub focused: Option<P>,
    /// Whole-floating-group gesture identity; host may display its own group drop preview.
    pub group_drag: Option<P>,
    /// Whole-group release for host-owned atomic docking policy.
    pub group_drop: Option<(P, Pos2)>,
}

impl<P> Default for DockOutput<P> {
    fn default() -> Self {
        Self { actions: Vec::new(), error: None, focused: None, group_drag: None, group_drop: None }
    }
}

/// Preferred body bounds. Tiny viewports relax minima proportionally rather than producing
/// inverted rectangles. Maximums constrain a pane when its sibling can occupy the remainder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLimits {
    pub min: Vec2,
    pub max: Vec2,
}

impl Default for PanelLimits {
    fn default() -> Self {
        Self { min: Vec2::ZERO, max: Vec2::splat(1_000_000.0) }
    }
}

impl PanelLimits {
    pub(super) fn valid(self) -> bool {
        self.min.is_finite()
            && self.max.is_finite()
            && self.min.x >= 0.0
            && self.min.y >= 0.0
            && self.max.x >= self.min.x
            && self.max.y >= self.min.y
            && self.max.x <= 1_000_000.0
            && self.max.y <= 1_000_000.0
    }
}

#[derive(Clone)]
pub(super) struct Drag<P> {
    pub(super) panel: P,
    pub(super) origin: Pos2,
    pub(super) source: Rect,
    pub(super) grab: Vec2,
}

/// What a group gesture moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupGesture {
    /// A floating window's grip: the window follows the pointer.
    Window,
    /// A drag bar or top border: a ghost follows the pointer, the release picks the target.
    Group,
}

#[derive(Clone)]
pub(super) struct GroupDrag<P> {
    /// The first member; identifies the gesture.
    pub(super) panel: P,
    /// The floating window rect at the press, restored when a window move is cancelled.
    pub(super) original: [f32; 4],
    pub(super) gesture: GroupGesture,
    pub(super) members: Vec<P>,
    pub(super) origin: Pos2,
    /// The group (or window) rect at the press. A floated group keeps this size.
    pub(super) source: Rect,
}

/// Start a docking drag from an application's existing tab/header. Use the same area ID when
/// switching from a legacy layout to [`DockArea`]. The model is untouched until a valid release.
pub fn begin_drag<P: Clone + Send + Sync + 'static>(ctx: &Context, area_id: Id, panel: P, source_rect: Rect) -> bool {
    let Some(origin) = ctx.pointer_interact_pos().filter(|p| p.is_finite()) else { return false };
    if !super::targets::usable(source_rect) {
        return false;
    }
    let grab = (origin - source_rect.min).clamp(Vec2::ZERO, source_rect.size());
    ctx.data_mut(|d| d.insert_temp(area_id.with("drag"), Drag { panel, origin, source: source_rect, grab }));
    true
}

/// Whether this area's tab or group gesture is active.
pub fn is_dragging<P: Clone + Send + Sync + 'static>(ctx: &Context, area_id: Id) -> bool {
    ctx.data(|data| data.get_temp::<Drag<P>>(area_id.with("drag")).is_some() || data.get_temp::<GroupDrag<P>>(area_id.with("group-drag")).is_some())
}

/// Discard pending tab/group gestures and an actively dragged renderer-owned control before
/// replacing a workspace. Body/custom-header/menu gestures and other areas are untouched.
/// Ownership is recorded as controls render; before an initial render or drag threshold there
/// may be no active egui drag to cancel. Keyboard focus is unchanged. The host owns the
/// replacement snapshot, so this does not restore a group's previous rectangle.
pub fn cancel_drag<P: Clone + Send + Sync + 'static>(ctx: &Context, area_id: Id) {
    let dragged = ctx.dragged_id();
    let owned = ctx.data_mut(|d| {
        d.remove::<Drag<P>>(area_id.with("drag"));
        d.remove::<GroupDrag<P>>(area_id.with("group-drag"));
        let ids = d.remove_temp::<Vec<Id>>(area_id.with("owned-interactions"));
        let resize = d.get_temp::<Id>(area_id.with("pointer-resize"));
        d.remove::<Id>(area_id.with("pointer-resize"));
        for id in resize.iter().chain(ids.iter().flatten()) {
            d.remove::<AnchoredResize>(id.with("pointer-anchor"));
        }
        dragged.is_some_and(|id| resize == Some(id) || ids.is_some_and(|ids| ids.contains(&id)))
    });
    if owned {
        ctx.stop_dragging();
    }
}

/// One mutable application adapter owns bodies, menus, shaped text and per-panel policy,
/// avoiding competing mutable closures. Logical labels are used for tab accessibility.
/// [`DockArea::show_content`] reads every policy from here; the closures passed to the older
/// `show*` functions override `logical_label`, `permissions` and `limits`.
pub trait DockContent<P> {
    fn body(&mut self, ui: &mut Ui, panel: &P);
    /// Plain text name for accessibility, menus and the default visual label.
    fn logical_label(&self, panel: &P) -> String
    where
        P: Debug,
    {
        format!("{panel:?}")
    }
    fn permissions(&self, _panel: &P) -> Permissions {
        Permissions::default()
    }
    fn limits(&self, _panel: &P) -> PanelLimits {
        PanelLimits::default()
    }
    /// Preferred floating size, independent of the current dock geometry. `None` falls back
    /// to the panel limits.
    fn floating_size(&self, _panel: &P) -> Option<Vec2> {
        None
    }
    fn icon(&self, _panel: &P) -> Option<egui::ImageSource<'static>> {
        None
    }
    /// Whether the panel may move to a native OS window (feature `native-windows`).
    fn allow_native(&self, _panel: &P) -> bool {
        false
    }
    /// A lone bare panel draws no tab strip and no drag bar, like a lone protected panel. It
    /// still moves by command and menu.
    fn bare(&self, _panel: &P) -> bool {
        false
    }
    /// Host-owned drop resolution for targets the renderer cannot know about. Returning
    /// feedback consumes the gesture, including no-op targets; the renderer then emits no move.
    fn resolve_drag(&mut self, _ctx: &Context, _style: &DockStyle, _request: &DropRequest<'_, P>) -> Option<DropFeedback> {
        None
    }
    fn label(&mut self, _ui: &Ui, _panel: &P, logical: &str, _font: &FontId, _color: Color32) -> egui::WidgetText {
        logical.into()
    }
    /// Shape stock Float/Move/Close action text independently of panel identity.
    /// The renderer preserves `logical` for accessibility, even when the visual text differs.
    fn action_label(&mut self, _ui: &Ui, logical: &str, _font: &FontId, _color: Color32) -> egui::WidgetText {
        logical.into()
    }
    fn panel_menu(&mut self, _ui: &mut Ui, _panel: &P) {}
    fn group_header_width(&self, _panels: &[P], _active: &P, _floating: bool) -> f32 {
        0.0
    }
    fn group_header(&mut self, _ui: &mut Ui, _panels: &[P], _active: &P, _floating: bool) {}
}

/// Host-owned drop resolution request, see [`DockContent::resolve_drag`].
pub struct DropRequest<'a, P> {
    pub panel: &'a P,
    pub pointer: Pos2,
    pub targets: &'a [DropTarget<P>],
    pub floating_rect: Option<Rect>,
    pub whole_group: bool,
    pub released: bool,
    pub workspace: Rect,
    pub screen: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct DropFeedback {
    pub preview: Option<Rect>,
    pub insertion: Option<Rect>,
}

struct BodyContent<F>(F);
impl<P, F: FnMut(&mut Ui, &P)> DockContent<P> for BodyContent<F> {
    fn body(&mut self, ui: &mut Ui, panel: &P) {
        (self.0)(ui, panel);
    }
}

/// The older `show*` closures in front of an application's content.
struct Overrides<'a, P, C: ?Sized> {
    label: &'a dyn Fn(&P) -> String,
    permissions: &'a dyn Fn(&P) -> Permissions,
    limits: &'a dyn Fn(&P) -> PanelLimits,
    inner: &'a mut C,
}

impl<P, C: DockContent<P> + ?Sized> DockContent<P> for Overrides<'_, P, C> {
    fn body(&mut self, ui: &mut Ui, panel: &P) {
        self.inner.body(ui, panel);
    }
    fn logical_label(&self, panel: &P) -> String {
        (self.label)(panel)
    }
    fn permissions(&self, panel: &P) -> Permissions {
        (self.permissions)(panel)
    }
    fn limits(&self, panel: &P) -> PanelLimits {
        (self.limits)(panel)
    }
    fn floating_size(&self, panel: &P) -> Option<Vec2> {
        self.inner.floating_size(panel)
    }
    fn icon(&self, panel: &P) -> Option<egui::ImageSource<'static>> {
        self.inner.icon(panel)
    }
    fn allow_native(&self, panel: &P) -> bool {
        self.inner.allow_native(panel)
    }
    fn bare(&self, panel: &P) -> bool {
        self.inner.bare(panel)
    }
    fn resolve_drag(&mut self, ctx: &Context, style: &DockStyle, request: &DropRequest<'_, P>) -> Option<DropFeedback> {
        self.inner.resolve_drag(ctx, style, request)
    }
    fn label(&mut self, ui: &Ui, panel: &P, logical: &str, font: &FontId, color: Color32) -> egui::WidgetText {
        self.inner.label(ui, panel, logical, font, color)
    }
    fn action_label(&mut self, ui: &Ui, logical: &str, font: &FontId, color: Color32) -> egui::WidgetText {
        self.inner.action_label(ui, logical, font, color)
    }
    fn panel_menu(&mut self, ui: &mut Ui, panel: &P) {
        self.inner.panel_menu(ui, panel);
    }
    fn group_header_width(&self, panels: &[P], active: &P, floating: bool) -> f32 {
        self.inner.group_header_width(panels, active, floating)
    }
    fn group_header(&mut self, ui: &mut Ui, panels: &[P], active: &P, floating: bool) {
        self.inner.group_header(ui, panels, active, floating);
    }
}

/// Optional renderer for applications without an existing docking surface. Apps with custom
/// chrome can instead use the model, target resolver and resize/tab primitives independently.
pub struct DockArea<P = ()> {
    id: Id,
    activation: Option<P>,
    blockers: Vec<Rect>,
}

impl<P: Clone + Eq + Hash + Debug + Send + Sync + 'static> DockArea<P> {
    pub fn new(id: Id) -> Self {
        Self { id, activation: None, blockers: Vec::new() }
    }

    /// Whether keyboard focus belongs to a rendered docking control in this layout.
    /// Panel bodies and application-provided header/menu widgets are excluded. Hosts may
    /// route nonconflicting global shortcuts through chrome while retaining widget ownership
    /// of activation, traversal, resize and cancellation keys. Invalid layouts return false.
    pub fn chrome_has_focus(&self, ctx: &egui::Context, layout: &Layout<P>) -> bool {
        let Some(focused) = ctx.memory(|memory| memory.focused()) else { return false };
        if layout.validate().is_err() || ctx.read_response(focused).is_none() {
            return false;
        }
        let matches = |id: Id| id == focused;
        let group = |panels: &[P]| {
            panels.iter().any(|panel| matches(self.id.with(("tab", panel))) || matches(self.id.with(("overflow", panel))))
                || ["move-group", "drag-bar", "drag-border"].iter().any(|name| matches(self.id.with((*name, panels.first()))))
        };
        for floating in &layout.floating {
            if floating.groups.iter().any(|g| group(&g.panels))
                || floating
                    .panels()
                    .next()
                    .is_some_and(|first| RESIZE_HANDLES.iter().any(|name| matches(self.id.with(("floating", first)).with(*name))))
            {
                return true;
            }
        }
        let mut nodes = layout.root.as_ref().map(|root| vec![(root, Vec::<bool>::new())]).unwrap_or_default();
        while let Some((node, path)) = nodes.pop() {
            match node {
                Node::Tabs { panels, .. } if group(panels) => return true,
                Node::Dock { columns, .. } if columns.iter().flat_map(|c| c.groups.iter()).any(|g| group(&g.panels)) => return true,
                Node::Stack { entries } => {
                    if entries.iter().any(|entry| {
                        matches(self.id.with(("tab", &entry.panel)))
                            || matches(self.id.with(("collapse", &entry.panel)))
                            || matches(self.id.with(("stack-resize", &entry.panel)))
                    }) {
                        return true;
                    }
                }
                Node::Split { first, second, .. } => {
                    if matches(self.id.with(("split", &path))) {
                        return true;
                    }
                    let mut second_path = path.clone();
                    second_path.push(true);
                    let mut first_path = path;
                    first_path.push(false);
                    nodes.push((second, second_path));
                    nodes.push((first, first_path));
                }
                _ => {}
            }
        }
        false
    }

    /// Raise and focus a panel's owning floating area on this frame. Apply Activate through
    /// the application's command path as well to select its tab.
    pub fn raise_panel(mut self, panel: P) -> Self {
        self.activation = Some(panel);
        self
    }

    /// Host overlays that must reject docking gestures, including floating fallback.
    pub fn blocked_rects(mut self, rects: &[Rect]) -> Self {
        self.blockers = rects.to_vec();
        self
    }

    /// Render a validated snapshot and return requested actions for the app's command handler.
    /// No action is applied here. Panel contents get a global stable egui ID across moves.
    /// Floating groups are egui areas inside this viewport; this does not create native windows.
    pub fn show(
        self,
        ui: &mut Ui,
        layout: &Layout<P>,
        style: &DockStyle,
        label: impl Fn(&P) -> String,
        permissions: impl Fn(&P) -> Permissions,
        body: impl FnMut(&mut Ui, &P),
    ) -> DockOutput<P> {
        self.show_with_limits(ui, layout, style, label, permissions, |_| PanelLimits::default(), body)
    }

    /// Like [`Self::show`], with per-panel size policies supplied by the application.
    #[allow(clippy::too_many_arguments)]
    pub fn show_with_limits(
        self,
        ui: &mut Ui,
        layout: &Layout<P>,
        style: &DockStyle,
        label: impl Fn(&P) -> String,
        permissions: impl Fn(&P) -> Permissions,
        limits: impl Fn(&P) -> PanelLimits,
        body: impl FnMut(&mut Ui, &P),
    ) -> DockOutput<P> {
        self.show_customized(ui, layout, style, label, permissions, limits, &mut BodyContent(body))
    }

    /// Render with application-owned chrome and shaped labels. Header callbacks queue app
    /// commands; structural changes are applied after rendering, like DockOutput actions.
    #[allow(clippy::too_many_arguments)]
    pub fn show_customized(
        self,
        ui: &mut Ui,
        layout: &Layout<P>,
        style: &DockStyle,
        label: impl Fn(&P) -> String,
        permissions: impl Fn(&P) -> Permissions,
        limits: impl Fn(&P) -> PanelLimits,
        content: &mut impl DockContent<P>,
    ) -> DockOutput<P> {
        let mut content = Overrides { label: &label, permissions: &permissions, limits: &limits, inner: content };
        self.show_content(ui, layout, style, &mut content)
    }

    /// Render with every label, policy and size from `content`.
    pub fn show_content(self, ui: &mut Ui, layout: &Layout<P>, style: &DockStyle, content: &mut dyn DockContent<P>) -> DockOutput<P> {
        let rect = ui.available_rect_before_wrap();
        if let Err(error) = layout.validate() {
            cancel_drag::<P>(ui.ctx(), self.id);
            return DockOutput { error: Some(error), ..Default::default() };
        }
        if !style.valid() || !super::targets::usable(rect) || layout.panels().into_iter().any(|p| !content.limits(p).valid()) {
            cancel_drag::<P>(ui.ctx(), self.id);
            return DockOutput { error: Some(DockError::InvalidGeometry), ..Default::default() };
        }
        let mut renderer = Renderer {
            id: self.id,
            style,
            content,
            activation: self.activation,
            blockers: self.blockers,
            layout,
            workspace: rect,
            targets: Vec::new(),
            owned_interactions: Vec::new(),
            output: DockOutput::default(),
        };
        if let Some(root) = &layout.root {
            renderer.node(ui, root, rect, &mut Vec::new());
            ui.allocate_rect(rect, Sense::hover());
        }
        renderer.floating(ui.ctx());
        renderer.finish_group_drag(ui.ctx());
        renderer.finish_drag(ui.ctx());
        ui.ctx().data_mut(|d| d.insert_temp(self.id.with("owned-interactions"), renderer.owned_interactions));
        renderer.output
    }
}
