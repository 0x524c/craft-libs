# Panel docking

`craft_ui::docking` lets the user arrange an app's panels: tab groups, docks with columns,
splits, floating windows and iconic columns. craft-ui owns the layout model, the renderer and the
gestures. The app owns panel contents, commands, document state and where layouts are saved.

Office apps that draw their own document with a few task panes beside it use
[`craft_ui::panes`](ui-panes.md) instead.

## Profiles

`DockStyle::profile` picks one of two arrangements. Both use the same model and actions.

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

- `Move`, `Float` and `Close` act on one panel. `MoveGroup`, `MoveColumn` and `MoveWindow` move a
  group, a dock column or a floating window, named by any panel in it.
- A `Target` is `At { anchor, placement }`, `Float { rect }` or `NewDock { side }`.
- A `Placement` is a tab position, a split zone, a new group above or below (with an optional
  preferred `height`), a new column, or a new frame on an outer edge.
- Containers take different placements: a dock column takes groups and columns, not splits.
  `Placement::for_zone(layout, anchor, zone, right_to_left)` maps a left, right, top, bottom or
  centre zone onto what the anchor's container takes: a split in the frame tree, a new column on
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
| `set_visible(panel, visible, fallback)` | Hide a panel, or show it at its recorded place, else beside `fallback`. |
| `reconcile(registered, default)` | Fit a loaded workspace to the panels the app has now. Unknown panels move to `hidden`, so they come back when registered again. |
| `reset(default)` | Return to the saved arrangement, or to `default` for a built-in workspace. |
| `validate` | Check a loaded workspace before use. |

With the `serde` feature, `Workspace`, `Layout` and `Action` serialize. Files without a version
are v1 and load unchanged. craft-ui does not choose where files go; the app does.

## Rendering

`DockArea::new(id).show_content(ui, &layout, &style, &mut content)` draws a layout snapshot and
returns a `DockOutput`. It does not change the layout. The app applies `output.actions` through
its own command path, so pointer, keyboard and control commands share one path.

`DockContent<P>` is the app's adapter. It draws each panel body and gives its label, icon,
permissions, size limits, preferred floating size, panel menu and group header. `bare` hides the
tab strip of a lone panel. Panel bodies keep a stable egui id when they move, so widget memory
survives.

`DockOutput` also has:

- `focused`: the panel the user focused this frame: a clicked tab, a drop, a raised window, or a
  press of any pointer button in a panel body, bare and protected panels included.
- `events`: requests outside the layout, such as `DockEvent::ToggleMaximize` (Frame) and
  `DockEvent::ToggleAutoCollapse` (Dock).
- `error`: a rejected gesture.

Builder options on `DockArea`:

| Option | Use |
|---|---|
| `maximized(panel)` | Maximize Frame: that frame fills the area. |
| `hide_panels(HidePanels)` | Hide panels for Tab and Shift+Tab. Protected panels stay. |
| `auto_collapse_iconic(on)` | Close a flyout on Escape or a click outside (default on). |
| `raise_panel(panel)` | Raise and focus a panel's floating window this frame. |
| `blocked_rects(rects)` | App overlays that docking gestures must ignore. |

`DockStyle::close_menu_items` and `DockStyle::close_button` hide the built-in Close menu items
and the floating window ×. They hide controls only: a panel that stays `closable` still closes
by the app's own Close items and commands.

`DockArea::chrome_has_focus` tells whether keyboard focus is on docking chrome rather than on a
panel body. `begin_drag` hands a drag that started in the app's own chrome to the area.
`docking::chrome` has the floating frame and title bar for apps that draw their own windows.

## Gestures

- A tab drags one panel. A group's drag bar (the space beside its tabs) or its top border drags
  the whole group. A column bar drags a column. A floating window's title bar drags the window.
- Drop previews show the destination: an outline for tabs, a line for a new group, column or dock,
  a filled half for a split. Escape or losing window focus cancels a drag.
- Double-click a tab or drag bar to minimize a group to its strip.
- A column's »/« button collapses it to icons, one icon per panel with a separator between groups.
  Widen an iconic column to show panel names. Click an icon to open its group as a flyout on the
  canvas side. Drag an icon to move its panel, or the grip above an icon group to move the group.

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
let closed = command.apply(&mut app.workspace, style.right_to_left, |panel| app.permissions(panel))?;
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
  it floats. `side` is `Start` or `End`.
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
default. `DockStyle::localized` sets them all from one keyed lookup:

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
