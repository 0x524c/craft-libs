//! `DockCommand`: every documented shape parses and maps to atomic layout actions.
#![cfg(feature = "serde")]

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::*;
use crate::layout::{SplitAxis, SplitSize};

type P = String;

fn s(panel: &str) -> P {
    panel.to_owned()
}

fn permissions(panel: &P) -> Permissions {
    if panel == "canvas" { Permissions::PROTECTED } else { Permissions::default() }
}

/// A start dock with one column of `[layers, channels]` and `[history]` beside the canvas, a
/// floating `color`, and `swatches` hidden from history's group.
fn workspace() -> Workspace<P> {
    let groups = vec![Group::new(vec![s("layers"), s("channels")], 0), Group::new(vec![s("history"), s("swatches")], 0)];
    let dock = Node::Dock { side: Side::Start, columns: vec![Column::new(groups, 240.0)] };
    let canvas = Node::Tabs { panels: vec![s("canvas")], active: 0 };
    let layout = Layout {
        root: Some(Node::Split { axis: SplitAxis::Horizontal, size: SplitSize::FixedFirst(240.0), first: Box::new(dock), second: Box::new(canvas) }),
        floating: vec![Floating::single(vec![s("color")], 0, [300.0, 200.0, 200.0, 150.0])],
    };
    let mut workspace = Workspace::new(layout);
    workspace.apply(Action::Close { panel: s("swatches") }, permissions).unwrap();
    workspace
}

fn parse(params: Value) -> Result<DockCommand<P>, serde_json::Error> {
    serde_json::from_value(params)
}

/// The actions `params` makes on a fresh workspace.
fn actions(params: Value) -> Vec<Action<P>> {
    let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
    command.to_actions(&workspace(), false).unwrap_or_else(|e| panic!("{params}: {e}"))
}

/// Apply `params` to a fresh workspace; return it and the panels closed.
fn applied(params: Value) -> (Workspace<P>, Vec<P>) {
    let mut workspace = workspace();
    let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
    let closed = command.apply(&mut workspace, false, permissions).unwrap_or_else(|e| panic!("{params}: {e}"));
    (workspace, closed)
}

/// `params` parses but applying it fails with `error` and changes nothing.
fn rejected(params: Value, error: DockError) {
    let mut workspace = workspace();
    let before = workspace.clone();
    let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
    assert_eq!(command.apply(&mut workspace, false, permissions), Err(error), "{params}");
    assert_eq!(workspace, before, "{params}");
}

fn tab(before: Option<&str>) -> Placement<P> {
    Placement::Tab { before: before.map(s) }
}

#[test]
fn dock_commands_parse_every_documented_shape_and_map_to_atomic_actions() {
    let mut seen = BTreeSet::new();
    let mut id = |params: Value| {
        seen.insert(params["command"].as_str().unwrap().to_owned());
        params
    };

    // The raw action form, in both protocols.
    let raw = json!({"MoveFloating": {"panel": "color", "rect": [44, 66, 360, 450]}});
    let moved = vec![Action::MoveFloating { panel: s("color"), rect: [44.0, 66.0, 360.0, 450.0] }];
    assert_eq!(actions(id(json!({"command": "window.panel.layout", "action": raw}))), moved);
    assert_eq!(actions(id(json!({"command": "ui.dock", "action": raw}))), moved);

    // move: tab by default, before a tab or appended, zones, a serialized placement, a group.
    let mv = |placement| vec![Action::Move { panel: s("history"), anchor: s("layers"), placement }];
    let base = json!({"command": "window.panel.move", "panel": "history", "anchor": "layers"});
    let with = |extra: Value| {
        let mut params = base.clone();
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        params
    };
    assert_eq!(actions(id(base.clone())), mv(tab(None)));
    assert_eq!(actions(with(json!({"before": "channels"}))), mv(tab(Some("channels"))));
    assert_eq!(actions(with(json!({"zone": "center", "before": null}))), mv(tab(None)));
    assert_eq!(actions(with(json!({"zone": "center"}))), mv(Placement::Split(Zone::Center)));
    assert_eq!(actions(with(json!({"zone": "Left"}))), mv(Placement::Column { after: false }));
    assert_eq!(actions(with(json!({"zone": "below"}))), mv(Placement::Group { below: true, height: None }));
    assert_eq!(actions(with(json!({"placement": {"Column": {"after": true}}}))), mv(Placement::Column { after: true }));
    assert_eq!(
        actions(with(json!({"group": true, "zone": "above"}))),
        vec![Action::MoveGroup {
            panel: s("history"),
            to: Target::At { anchor: s("layers"), placement: Placement::Group { below: false, height: None } }
        }]
    );

    // float: a docked panel, a lone floating panel, a group and a column.
    assert_eq!(
        actions(id(json!({"command": "window.panel.float", "panel": "layers", "x": 10, "y": 20}))),
        vec![Action::Float { panel: s("layers"), rect: [10.0, 20.0, 260.0, 320.0] }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.float", "panel": "color", "width": 320})),
        vec![Action::MoveFloating { panel: s("color"), rect: [300.0, 200.0, 320.0, 150.0] }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.float", "panel": "layers", "group": true})),
        vec![Action::MoveGroup { panel: s("layers"), to: Target::Float { rect: None } }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.float", "panel": "history", "column": true})),
        vec![Action::MoveColumn { panel: s("history"), to: Target::Float { rect: None } }]
    );

    // dock: to an anchor, to a side, back into the dock, and a hidden panel to its last place.
    assert_eq!(
        actions(id(json!({"command": "window.panel.dock", "panel": "color", "anchor": "history", "zone": "tab"}))),
        vec![Action::Move { panel: s("color"), anchor: s("history"), placement: tab(None) }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.dock", "panel": "color", "side": "End"})),
        vec![Action::MoveGroup { panel: s("color"), to: Target::NewDock { side: Side::End } }]
    );
    let new_column = Target::At { anchor: s("layers"), placement: Placement::Column { after: true } };
    assert_eq!(
        actions(json!({"command": "window.panel.dock", "panel": "color", "column": true, "side": "Start"})),
        vec![Action::MoveWindow { panel: s("color"), to: new_column.clone() }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.dock", "panel": "history", "group": true, "side": "Start"})),
        vec![Action::MoveGroup { panel: s("history"), to: new_column }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.dock", "panel": "color"})),
        vec![Action::Move { panel: s("color"), anchor: s("layers"), placement: tab(None) }]
    );
    assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "layers"})), vec![]);
    let (shown, _) = applied(json!({"command": "window.panel.dock", "panel": "swatches"}));
    assert!(shown.layout.contains(&s("swatches")) && shown.hidden.is_empty());

    // activate shows a hidden panel; close actions report what they closed.
    let (shown, _) = applied(id(json!({"command": "window.panel.activate", "panel": "swatches"})));
    assert!(shown.layout.contains(&s("swatches")));
    assert_eq!(actions(json!({"command": "window.panel.activate", "panel": "channels"})), vec![Action::Activate { panel: s("channels") }]);
    assert_eq!(applied(id(json!({"command": "window.panel.close", "panel": "channels"}))).1, vec![s("channels")]);
    assert_eq!(applied(id(json!({"command": "window.panel.closeGroup", "panel": "channels"}))).1, vec![s("layers"), s("channels")]);
    assert_eq!(applied(id(json!({"command": "window.panel.closeOthers", "panel": "channels"}))).1, vec![s("layers")]);

    // minimize and iconic toggle without a value.
    assert_eq!(
        actions(id(json!({"command": "window.panel.minimize", "panel": "layers"}))),
        vec![Action::SetMinimized { panel: s("layers"), minimized: true }]
    );
    assert_eq!(
        actions(json!({"command": "window.panel.minimize", "panel": "layers", "minimized": false})),
        vec![Action::SetMinimized { panel: s("layers"), minimized: false }]
    );
    assert_eq!(
        actions(id(json!({"command": "window.panel.iconic", "panel": "history"}))),
        vec![Action::SetIconic { panel: s("history"), iconic: true }]
    );

    // window: a floating window goes native at its rect; a docked group floats first.
    assert_eq!(
        actions(id(json!({"command": "window.panel.window", "panel": "color"}))),
        vec![Action::SetNative { panel: s("color"), rect: Some([300.0, 200.0, 200.0, 150.0]) }]
    );
    let docked = actions(json!({"command": "window.panel.window", "panel": "layers"}));
    assert!(
        matches!(docked.as_slice(), [Action::MoveGroup { to: Target::Float { rect: None }, .. }, Action::SetNative { rect: Some(_), .. }]),
        "{docked:?}"
    );
    assert_eq!(
        actions(json!({"command": "window.panel.window", "panel": "color", "native": false})),
        vec![Action::SetNative { panel: s("color"), rect: None }]
    );

    // Named workspaces and Reset belong to the application.
    for params in [
        json!({"command": "window.workspace", "name": "Painting"}),
        json!({"command": "window.workspace.new", "name": "Mine"}),
        json!({"command": "window.workspace.delete", "name": "Mine"}),
        json!({"command": "window.workspace.reset"}),
        json!({"command": "ui.dock.reset"}),
    ] {
        let command = parse(id(params.clone())).unwrap();
        assert!(command.is_app_owned(), "{params}");
        rejected(params, DockError::NotAllowed);
    }
    assert_eq!(parse(json!({"command": "ui.dock.reset"})).unwrap(), DockCommand::ResetWorkspace {});

    // Lock toggles or sets.
    assert!(applied(id(json!({"command": "window.workspace.lock"}))).0.locked);
    assert!(!applied(json!({"command": "window.workspace.lock", "locked": false})).0.locked);

    // The small apps' ui.dock operations.
    let op = |params: Value| {
        let mut params = params;
        params["command"] = json!("ui.dock");
        params
    };
    let (shown, _) = applied(op(json!({"operation": "open", "panel": "swatches"})));
    assert!(shown.layout.contains(&s("swatches")));
    assert_eq!(actions(op(json!({"operation": "close", "panel": "layers"}))), vec![Action::Close { panel: s("layers") }]);
    assert_eq!(actions(op(json!({"operation": "activate", "panel": "channels"}))), vec![Action::Activate { panel: s("channels") }]);
    assert_eq!(
        actions(op(json!({"operation": "float", "panel": "layers", "rect": [1, 2, 300, 200]}))),
        vec![Action::Float { panel: s("layers"), rect: [1.0, 2.0, 300.0, 200.0] }]
    );
    assert_eq!(
        actions(op(json!({"operation": "moveFloating", "panel": "color", "rect": [1, 2, 300, 200]}))),
        vec![Action::MoveFloating { panel: s("color"), rect: [1.0, 2.0, 300.0, 200.0] }]
    );
    let to_layers = |placement| vec![Action::Move { panel: s("color"), anchor: s("layers"), placement }];
    assert_eq!(actions(op(json!({"operation": "move", "panel": "color", "target": "layers"}))), to_layers(Placement::Split(Zone::Center)));
    assert_eq!(
        actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "zone": "left"}))),
        to_layers(Placement::Column { after: false })
    );
    assert_eq!(actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "before": null}))), to_layers(tab(None)));
    assert_eq!(
        actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "before": "channels"}))),
        to_layers(tab(Some("channels")))
    );
    assert_eq!(
        actions(op(json!({"operation": "setStackOpen", "panel": "layers", "open": false}))),
        vec![Action::SetStackOpen { panel: s("layers"), open: false }]
    );
    assert_eq!(
        actions(op(json!({"operation": "resizeStack", "panel": "layers", "height": null}))),
        vec![Action::ResizeStack { panel: s("layers"), height: None }]
    );
    assert_eq!(
        actions(op(json!({"operation": "resizeStack", "panel": "layers", "height": 120}))),
        vec![Action::ResizeStack { panel: s("layers"), height: Some(120.0) }]
    );
    assert_eq!(
        actions(op(json!({"operation": "resizeSplit", "path": [], "size": {"Ratio": 0.35}}))),
        vec![Action::ResizeSplit { path: vec![], size: SplitSize::Ratio(0.35) }]
    );

    let ids: BTreeSet<String> = DockCommand::<P>::IDS.iter().map(|id| (*id).to_owned()).collect();
    assert_eq!(seen, ids, "one case per id");

    // Bad params: parse errors, or a DockError with nothing changed.
    for params in [
        json!({"command": "window.panel.unknown", "panel": "layers"}),
        json!({"command": "window.panel.move", "panel": "layers"}),
        json!({"command": "window.panel.move", "panel": "layers", "anchor": "history", "zone": "middle"}),
        json!({"command": "window.panel.minimize", "panel": "layers", "minimized": "yes"}),
        op(json!({"operation": "spin", "panel": "layers"})),
        op(json!({"operation": "float", "panel": "layers", "rect": [1, 2, 3]})),
        op(json!({"operation": "resizeStack", "panel": "layers"})),
        op(json!({"operation": "setStackOpen", "panel": "layers"})),
        op(json!({"operation": "move", "panel": "layers"})),
    ] {
        assert!(parse(params.clone()).is_err(), "{params}");
    }
    rejected(json!({"command": "window.panel.move", "panel": "layers", "anchor": "nowhere"}), DockError::MissingTarget);
    rejected(json!({"command": "window.panel.move", "panel": "nowhere", "anchor": "layers"}), DockError::MissingPanel);
    rejected(
        json!({"command": "window.panel.move", "panel": "layers", "anchor": "history", "zone": "left", "before": null}),
        DockError::InvalidPlacement,
    );
    rejected(json!({"command": "window.panel.float", "panel": "layers", "width": 1e30}), DockError::InvalidGeometry);
    rejected(json!({"command": "window.panel.move", "panel": "canvas", "anchor": "layers"}), DockError::NotAllowed);
    rejected(op(json!({"operation": "moveFloating", "panel": "color", "rect": [0, 0, -5, 10]})), DockError::InvalidGeometry);
    rejected(
        op(json!({"operation": "move", "panel": "color", "target": "layers", "zone": "top", "before": "channels"})),
        DockError::InvalidPlacement,
    );
    rejected(op(json!({"operation": "resizeSplit", "path": [true, true, true], "size": {"Ratio": 0.5}})), DockError::MissingTarget);
    // A docked group cannot float into a native window when locked.
    let mut locked = workspace();
    locked.locked = true;
    let before = locked.clone();
    assert!(parse(json!({"command": "window.panel.window", "panel": "layers"})).unwrap().apply(&mut locked, false, permissions).is_err());
    assert_eq!(locked, before);
}

#[test]
fn side_zones_map_to_the_placement_the_anchor_container_takes_in_either_direction() {
    let command =
        |zone: &str, anchor: &str| parse(json!({"command": "window.panel.move", "panel": "history", "anchor": anchor, "zone": zone})).unwrap();
    let placed = |zone: &str, anchor: &str, rtl: bool| match command(zone, anchor).to_actions(&workspace(), rtl) {
        Ok(actions) => match actions.as_slice() {
            [Action::Move { placement, .. }] => Ok(placement.clone()),
            other => panic!("{other:?}"),
        },
        Err(error) => Err(error),
    };
    // A start dock is on the left edge left to right, on the right edge right to left.
    assert_eq!(placed("right", "layers", false), Ok(Placement::Column { after: true }));
    assert_eq!(placed("right", "layers", true), Ok(Placement::Column { after: false }));
    assert_eq!(placed("left", "layers", true), Ok(Placement::Column { after: true }));
    assert_eq!(placed("top", "layers", false), Ok(Placement::Group { below: false, height: None }));
    assert_eq!(placed("bottom", "color", false), Ok(Placement::Group { below: true, height: None }));
    assert_eq!(placed("left", "color", false), Err(DockError::InvalidPlacement));
    assert_eq!(placed("left", "canvas", true), Ok(Placement::Split(Zone::Left)));

    let (workspace, _) = applied(json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "zone": "right"}));
    let Some(Node::Split { first, .. }) = &workspace.layout.root else { panic!() };
    let Node::Dock { columns, .. } = &**first else { panic!() };
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[1].groups[0].panels, vec![s("history")]);
}
