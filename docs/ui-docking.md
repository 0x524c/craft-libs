# Panel docking

`craft_ui::docking` lets the user arrange an app's panels: tab groups, docks with columns,
splits, floating windows and iconic columns. craft-ui owns the layout model, the renderer and the
gestures. The app owns panel contents, commands, document state and where layouts are saved.

Office apps that draw their own document with a few task panes beside it use
[`craft_ui::panes`](ui-panes.md) instead.

## Profiles

`DockStyle::profile` picks one of two arrangements. Both use the same model and actions.
An app builds its style with `DockStyle::from_ui`, `from_visuals` or `default` and then sets
fields; new fields can be added, so the struct has no literal form outside the crate.

| Profile | Arrangement | Apps |
|---|---|---|
| `Profile::Dock` (default) | Docks on the window's start and end edges. A dock has columns; a column has stacked groups and can collapse to icons. Frames (tab groups and splits) fill the space between the docks. | Creative apps with a canvas and panel docks |
| `Profile::Frame` | A split tree of tab frames. Frame menus offer Maximize Frame, Undock and Close Other Panels. Ctrl/Cmd + tab drag undocks. Drops on all four outer edges split at the root. | Apps whose whole window is frames |

Sides are logical: `Side::Start` is the left edge, or the right edge when
`DockStyle::right_to_left` is set. egui has no app-wide direction, so the app sets it from its
locale. Right to left, tab strips also mirror: the first tab is rightmost, the group header and
drag bar are at the left, and drop insertion slots follow. Accordion headers and floating title
bars (`chrome::TitleBar`, through `ChromeStyle::right_to_left`) mirror too. Frame splits keep
their sides.

## Model

`Layout<P>` holds an optional root `Node<P>` and a list of `Floating` windows.

| Node | Holds |
|---|---|
| `Tabs` | A tab group: panels and the active index. |
| `Split` | Two children, horizontal or vertical, with a ratio or a fixed first or second size. |
| `Stack` | Accordion entries, each open or closed, with an optional height. |
| `Dock` | A side and its columns. A `Column` has `Group`s, a width, and `iconic`. |

A `Floating` window has one or more stacked groups, a rect, and an optional native-window rect.
`Floating::single` makes a window with one group; `panels()` and `contains()` read it.

`P` is the app's panel id: stable, not localized, `Clone + Eq + Hash`. The renderer also needs
`Debug + Send + Sync + 'static`. A panel occurs at most once in a layout.

Change a layout only through `Action`s:

```rust
use craft_ui::docking::{Action, Floating, Layout, Node, Placement};

let mut layout = Layout {
    root: Some(Node::Tabs { panels: vec!["layers", "properties"], active: 0 }),
    floating: vec![Floating::single(vec!["brushes"], 0, [40.0, 80.0, 240.0, 300.0])],
};
layout.apply(Action::Move {
    panel: "brushes",
    anchor: "layers",
    placement: Placement::Tab { before: Some("properties") },
})?;
# Ok::<(), craft_ui::docking::DockError>(())
```

- `Move`, `MovePanel`, `Float` and `Close` act on one panel. `MoveGroup`, `MoveColumn` and
  `MoveWindow` move a group, a dock column or a floating window, named by any panel in it.
- A `Target` is `At { anchor, placement }`, `Float { rect }` or `NewDock { side }`.
- A `Placement` is a tab position, a split zone, a new group above or below (with an optional
  preferred `height`), a new column, or a new frame on an outer edge.
- Containers take different placements: a dock column takes groups and columns, not splits.
  `Placement::for_zone(layout, anchor, zone, right_to_left)` maps a left, right, top, bottom or
  center zone onto what the anchor's container takes: a split in the frame tree, a new column on
  that side or a new group above or below in a dock, a group above or below in a floating window.
- Other actions select, minimize, collapse to icons, resize splits, columns and groups, and move
  a floating window to its own OS window (`SetNative`, feature `native-windows`).

Every action checks the whole result before it commits. A bad target, a duplicate id, a stale
split path or invalid geometry returns a `DockError` and leaves the layout unchanged. The source
panel is never lost. `apply`, `apply_with_permissions` and `apply_all` return the panels the
actions closed, so the app can release their state; `closed_by` lists them before applying.

`Permissions` limit what may happen to a panel: `movable`, `floatable`, `closable`,
`accepts_tabs` and `accepts_splits`. `apply_with_permissions` enforces them for every action,
whoever issues it. `Permissions::PROTECTED` keeps a canvas docked and open and takes neither tabs
nor side splits; a group dropped on it floats there. An app that opens panels beside its canvas
uses `Permissions { accepts_splits: true, ..Permissions::PROTECTED }`.

## Workspace

`Workspace<P>` is what an app saves: the layout, the hidden panels with where each one goes back,
an optional saved arrangement for Reset, and `locked`.

| Method | Does |
|---|---|
| `apply`, `apply_all` | Apply actions all or none, with the app's permissions. Closed panels are recorded in `hidden`. While `locked`, moving, floating and closing are refused. |
| `set_visible(panel, visible, fallback, permissions)` | Hide a panel that may close, or show it at its recorded place, else beside `fallback`. |
| `reconcile(registered, permissions, default)` | Fit a loaded workspace to the panels the app has now. Unknown panels move to `hidden`, so they come back when registered again. A registered panel that cannot close, such as the canvas, is always shown. |
| `reset(default)` | Return to the saved arrangement, or to `default` for a built-in workspace. |
| `validate` | Check a loaded workspace before use. |

With the `serde` feature, `Workspace`, `Layout` and `Action` serialize. Files without a version
are v1 and load unchanged. craft-ui does not choose where files go; the app does.

## Rendering

`DockArea::new(id).show_content(ui, &layout, &style, &mut content)` draws a layout snapshot and
returns a `DockOutput`. It does not change the layout. The app applies `output.actions` through
its own command path, so pointer, keyboard and control commands share one path.

`DockContent<P>` is the app's adapter. It draws each panel body and gives its label, icon,
permissions, size limits, preferred floating size, panel menu and group header. The group header
sits at the end of the tab strip; `group_header_width` gets the strip width, so a header that
draws its own tabs can size them to the room left. `bare` hides the tab strip of a lone panel. An icon is an image (`icon`), or drawn in code by `paint_icon`, which
gets a painter, the icon rect and the tint color and returns true when it painted. Panel bodies keep a stable egui id when they move, so widget memory
survives.

`DockOutput` also has:

- `focused`: the panel the user focused this frame: a clicked tab, a drop, or a press of any
  pointer button in a panel body (bare and protected panels included) or in the group header,
  which focuses the group's shown panel. Commands, `raise_panel` and
  workspace resets never set it; a host that tracks the last focused panel clears it when a reset
  or command hides, closes or moves that panel.
- `events`: requests outside the layout, such as `DockEvent::ToggleMaximize` (Frame) and
  `DockEvent::ToggleAutoCollapse` (Dock), and `DockEvent::FlyoutChanged { panel }` (Dock): the
  flyout opened, switched or closed (`None`). It comes once per change, whatever made it: an
  icon, Escape, a press outside, a raised panel, `open_flyout` or `close_flyout`.
- `error`: a rejected gesture.

Builder options on `DockArea`:

| Option | Use |
|---|---|
| `maximized(panel)` | Maximize Frame: that frame fills the area. |
| `hide_panels(HidePanels)` | Hide panels for Tab and Shift+Tab. Protected panels stay. |
| `auto_collapse_iconic(on)` | Close a flyout on Escape or a click outside (default on). |
| `raise_panel(panel)` | Raise a panel's floating window, or open its flyout, this frame. It moves no keyboard focus and does not set `DockOutput::focused`. |
| `blocked_rects(rects)` | App overlays that docking gestures must ignore. |
| `float_bounds(rect)` | Where floating windows, flyouts and drag ghosts may go (default: the whole viewport). Pass the viewport below a title bar drawn under a transparent OS title bar, so no floating window sits where a press moves the app window. |

`DockStyle::close_menu_items` and `DockStyle::close_button` hide the built-in Close menu items
and the floating window ×. They hide controls only: a panel that stays `closable` still closes
by the app's own Close items and commands.

`DockArea::chrome_has_focus` tells whether keyboard focus is on docking chrome (any control a
`Hit` names) rather than on a panel body. Focus follows only the user's input: a tab keeps keyboard focus until a command or a
workspace reset hides, closes or moves its panel, and then gives it up. A divider or a floating
window's resize grip holds focus only while the pointer drags it, so the app's shortcuts and
canvas arrow keys work right after; one reached with Tab or F6 keeps its focus, and arrow keys
resize it. Hovering focuses nothing. `begin_drag` hands a drag that started in the app's own chrome to the area.
`docking::chrome` has the floating frame, the title bar and `drop_preview` (the drop preview
docking and `craft_ui::panes` both draw) for apps that draw their own windows.

## Tab strips

Every shown tab carries its panel's full name. Tabs that do not fit go to the "…" button at the
end of the strip, whose menu lists the hidden panels; choosing one makes it active. The active tab
is always shown, in its logical place: when it lies past the tabs that fit, it takes the place of
the last of them. Only an active tab wider than the room beside the "…" button, or a lone tab
wider than the strip, is cut with "…", and its tooltip names it in full. The drag bar always keeps
its reserved width: tabs go to the overflow menu before it narrows, and only a strip narrower than
the bar itself shrinks it. A tab dropped on the "…" button goes to the end of the group.

## Floating size

A panel or group floated by a drag, a menu or a command without a rect takes its content height:
the title bar, the tab strip and the height the active panel's content used when it was last
drawn (the content ui's `min_rect`), at least 120 points. The most it takes is the height of
`DockContent::floating_size`, or 320 points for a panel leaving a dock or frame and a group
leaving a dock. A short panel therefore floats as a short window. Content that reaches the bottom
of its body (a panel that allocates its whole rect) has no height of its own: it floats at the
`floating_size` height, else at 320 points. A panel's width is its `floating_size` width, else
its docked width; a group keeps its docked width.
A flyout fits its content the same way, without the title bar.

- A panel the area has not drawn floats at the `floating_size` height, else the docked height
  capped at 320, or at a `FloatSpace::with_panel_size` size for commands.
- A group from a frame without a `floating_size` keeps its frame size. A dock column of several
  groups floats at the tallest `floating_size` height, else its column height capped at 320.
- A rect from a command and a window the user resized keep their size; floating windows are not
  refitted while open.
- Docked layout does not change: a column's last group still fills the column.

## Gestures

- A tab drags one panel. A group's drag bar (the space beside its tabs) or its top border drags
  the whole group. A column bar drags a column. A floating window's title bar drags the window.
- A movable floating window resizes from every edge and corner. The top edge and corners are a
  4 pt band above the title bar (`chrome::RESIZE_BAND`), so they take no moves and no clicks on
  the title bar's buttons. A resize keeps the opposite edges in place and stays within the size
  limits and the screen. A focused resize handle moves its edge 8 pt per arrow key.
- Over a frame, the tab strip and the middle join its tabs, and the nearest side splits it. A
  side zone is a quarter of the frame deep, at least 32 pt and at most half the frame, so a narrow
  tool strip keeps both side targets. A drop in the gap between two frames splits beside the
  nearer one.
- Drop previews show the destination: an outline for tabs, a line for a new group, column or dock,
  a filled half for a split. Escape or losing window focus cancels a drag.
- Double-click a tab or drag bar to minimize a group to its strip.
- Right-click a tab for its menu, or a drag bar for the group menu; Enter or Space on a focused
  drag bar opens it too. A left click on the drag bar only focuses it.
- In the Frame profile, the mouse wheel over a tab strip selects the next or previous tab, and a
  middle click on a closable panel's tab closes it. `DockStyle::wheel_selects_tabs` and
  `middle_click_closes_tabs` turn either on or off in any profile.
- A column's »/« button collapses it to icons, one icon per panel with a separator between groups.
  Widen an iconic column to show panel names. Click an icon to open its group as a flyout on the
  canvas side. Drag an icon to move its panel, or an icon group's top edge (the separator line above it) to move the group.

## Commands

With the `serde` feature, `DockCommand<P>` is the shared control vocabulary. The app puts the
command id under `"command"` in the params and deserializes them. craft-ui has no `serde_json`
dependency.

```rust,ignore
let mut params = params.clone();
params["command"] = json!(id);
let command: DockCommand<PanelId> = serde_json::from_value(params)?;
if command.is_app_owned() {
    return app.workspace_command(command);
}
let space = float_space(ctx, DOCK_ID).unwrap_or_else(|| FloatSpace::new(ctx.content_rect()));
let closed = command.apply(&mut app.workspace, style.right_to_left, &space, |panel| app.permissions(panel))?;
```

| Id | Params (`?` is optional) |
|---|---|
| `window.panel.layout` | `{action}`: a serialized `Action` |
| `window.panel.move` | `{panel, anchor, zone?, before?, group?, placement?}` |
| `window.panel.float` | `{panel, x?, y?, width?, height?, group?, column?}` |
| `window.panel.dock` | `{panel, anchor?, zone?, before?, group?, column?, side?}` |
| `window.panel.activate`, `.close`, `.closeGroup`, `.closeOthers` | `{panel}` |
| `window.panel.minimize` | `{panel, minimized?}` |
| `window.panel.iconic` | `{panel, iconic?}` |
| `window.panel.window` | `{panel, native?}` |
| `window.workspace`, `.new`, `.delete` | `{name}` |
| `window.workspace.reset`, `ui.dock.reset` | `{}` |
| `window.workspace.lock` | `{locked?}` |
| `ui.dock` | `{action}`, or `{operation, …}` |

- `zone` is `tab`, `center`, `left`, `right`, `top`, `bottom`, `above` or `below`. `before` names
  the tab to insert before; `null` appends. `left`, `right`, `top` and `bottom` follow
  `Placement::for_zone`, so beside a docked panel they make a new column or group. `apply` and
  `to_actions` take the layout's `right_to_left` for that.
- `group` moves the panel's whole group; `column` moves its dock column, or its whole window when
  it floats. `side` is `Start` or `End`: what moves becomes a new last column of that dock, or
  a new dock; when it already is that dock's last column, the command does nothing.
- `window.panel.dock` with `left`, `right`, `top` or `bottom` and no `anchor` or `side` makes a
  new frame on that outer edge (`Placement::Edge`). A hidden panel is shown first.
- `window.panel.float` without a rect floats what it moves at the size a drag floats it at (see
  Floating size). It goes
  inside the dock area, 24 points from its top-left corner, so it stays clear of the menu bar.
  The area records that geometry in a `FloatSpace` each frame; `float_space(ctx, id)` reads it
  and `apply` and `to_actions` take it. Before the area has drawn, `FloatSpace::new(area)` floats
  a panel at 260x320 and a group or column at its column width by 320.
  `FloatSpace::with_panel_size(panel, size)` gives a panel the area has not drawn (a hidden
  panel, a headless run) the host's own size; a size the area recorded wins. `x`, `y`, `width` and
  `height` replace the matching values, and a group or column that is already a whole floating
  window keeps its rect.
- Without a value, `minimized`, `iconic` and `locked` toggle.
- `ui.dock` operations are those of LightCraft, PDFCraft, CADCraft and SoundCraft: `open`,
  `close`, `activate`, `float {rect}`, `moveFloating {rect}`, `move {target, zone?, before?}`,
  `setStackOpen {open}`, `resizeStack {height}` and `resizeSplit {path, size}`. `rect` is
  `[x, y, width, height]`.

`to_actions` returns the actions a command makes without applying them. `apply` applies them all
or none and returns the panels it closed, so the app can release their state. Showing a hidden
panel and Lock change the workspace directly. Named workspaces and Reset belong to the app:
`is_app_owned` is true for them and `apply` refuses them. `DockCommand::IDS` lists every id for
the app's command registry. App-specific aliases stay in the app.

## Labels

Every visible and accessibility label is a `*_label` field of `DockStyle`, with an English
default: title case for menu items, sentence case for names only screen readers announce. `DockStyle::localized` sets them all from one keyed lookup:

```rust,ignore
let style = DockStyle::from_ui(ui).localized(|key| app.strings.get(key).cloned());
```

Keys are `dock.close`, `dock.float`, `dock.collapseToIcons` and so on; the rustdoc of `localized`
lists them all. `DockContent::label` and `action_label` can shape panel and action text further;
the logical label stays the accessibility name.

## Testing

`DockArea::hit_rects(ctx, id)` lists every gesture source drawn last frame as a `Hit` with its
rect: tabs, drag bars, borders, column bars, »/« buttons, title bars, close buttons, icons, icon
grips, resize edges and dividers. It is always compiled, so an app can return it from a
`ui.docking` control method next to the layout and the focused panel.

`DockArea::hit_id(id, &hit)` is the egui widget id of a control, for tests and automation that
focus it or read its response. `DockArea::floating_layer(id, &window)` is a floating window's
egui layer: compare its place in `memory.layer_ids()` with another layer to tell whether the
window covers it, and read its rect with `memory.area_rect(layer.id)`. A window the area does not
draw (its panels hidden, or a frame maximized) keeps its last rect there, so check
`memory.areas().is_visible(&layer)` first.

The `testing` feature adds `docking::testing::DockDriver`. It runs a `DockArea` in a headless
egui context with real pointer and key events, and applies the actions each frame emits.

```rust,ignore
let mut driver = DockDriver::new(app_layout(), Panels::default());
let bar = driver.hit(&Hit::DragBar(PanelId::Layers)).unwrap();
driver.drag(bar.center(), egui::pos2(2.0, 300.0));
driver.settle();
assert!(driver.hit(&Hit::ColumnBar(PanelId::Layers)).is_some());
```

It also has `press`, `release`, `click`, `hold`, `key`, `focus`, `hits`, and checks for painted
previews (`filled`, `outlined`).

craft-ui's own tests in `crates/ui/tests/docking_workflows.rs`:

- Load every app's v1 layout fixture, dock groups into a new dock, a bottom band and a seam,
  collapse and expand the column, float it by command and drag it back, then check the layout
  reads back as written.
- Render Layers, Properties and Brushes panels with real content, and merge, detach and redock
  Brushes by pointer at two widths, two scales and both themes. Set `CRAFT_UI_DOCK_VISUAL_DIR`
  to keep the captures:

```sh
CRAFT_UI_DOCK_VISUAL_DIR=/absolute/output/path cargo test -p craft-ui --test docking_workflows --all-features
```

An adopting app still tests its own commands, contents, saved workspaces and themes.

## What stays in the app

- The panel registry, panel contents and document state.
- Named workspaces, Reset, and when and where layouts are saved.
- Menus and key bindings outside the docking chrome.
- Native window ownership on platforms without egui viewports.
