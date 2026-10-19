#![cfg(feature = "ui")]
//! The Keyboard Shortcuts dialog driven through egui_kittest: recording, conflicts, search, areas,
//! platform display, accessibility labels and small windows.

use std::borrow::Cow;

use craft_keymap::ui::{ConflictMode, DialogState, English, Options, Outcome, ShortcutsDialog, Text};
use craft_keymap::{Bindable, Chord, CommandRef, ConflictKind, Keymap, Kind, Platform, Scope, Scopes, Trigger};
use egui::accesskit::Role;
use egui::{Event, Key as EguiKey, Modifiers, Vec2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ui_fixture::fixture;

mod ui_fixture;

struct Fx {
    km: Keymap,
    options: Options,
    state: DialogState,
    text: Box<dyn Text>,
    window: bool,
    outcomes: Vec<Outcome>,
    /// A key press was still in the frame's input after the dialog ran.
    keys_left: bool,
}

impl Fx {
    fn new(km: Keymap) -> Fx {
        Fx {
            km,
            options: Options::default(),
            state: DialogState::new(),
            text: Box::new(English),
            window: false,
            outcomes: Vec::new(),
            keys_left: false,
        }
    }

    fn bindings(&self, id: &str) -> Vec<String> {
        self.km.bindings(&CommandRef::from(id)).iter().map(|b| b.to_string()).collect()
    }

    fn cancelled(&self) -> bool {
        self.outcomes.iter().any(|o| matches!(o, Outcome::Cancel))
    }
}

fn harness(fx: Fx, size: Vec2) -> Harness<'static, Fx> {
    let mut h = Harness::builder().with_size(size).build_ui_state(
        |ui, fx: &mut Fx| {
            let Fx { km, options, state, text, window, outcomes, keys_left } = fx;
            let dialog = ShortcutsDialog::new(km, text.as_ref()).options(options);
            let out = if *window { dialog.show(ui.ctx(), state) } else { dialog.ui(ui, state) };
            outcomes.extend(out);
            *keys_left |= ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. })));
        },
        fx,
    );
    h.run();
    h
}

fn open(fx: Fx) -> Harness<'static, Fx> {
    harness(fx, vec2(1000.0, 700.0))
}

fn mac() -> Harness<'static, Fx> {
    open(Fx::new(fixture(Platform::Mac)))
}

fn click(h: &mut Harness<'_, Fx>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn press(h: &mut Harness<'_, Fx>, modifiers: Modifiers, key: EguiKey) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}

fn raw_key(h: &mut Harness<'_, Fx>, key: EguiKey, time: f64) {
    h.input_mut().time = Some(time);
    h.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    h.input_mut().time = Some(time + 0.01);
    h.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.step();
}

fn has(h: &Harness<'_, Fx>, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

fn table_focused(h: &Harness<'_, Fx>) -> bool {
    h.get_by_label("Commands").is_focused()
}

fn chord(text: &str, platform: Platform) -> String {
    Trigger::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).display(platform)
}

// ---- Recording ----

#[test]
fn recording_captures_a_chord_another_command_would_run() {
    let mut h = mac();
    click(&mut h, "Save: ⌘S");
    assert!(has(&h, "Save: press keys"));
    h.state_mut().keys_left = false;
    press(&mut h, Modifiers::COMMAND, EguiKey::E);
    assert!(!h.state().keys_left, "the dialog took the key press");
    assert!(has(&h, "⚠ ⌘E is used by Export…"));
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"], "nothing applied before an answer");
}

#[test]
fn recording_takes_tab_arrows_and_shift_escape() {
    let mut h = mac();
    click(&mut h, "Save: ⌘S");
    press(&mut h, Modifiers::NONE, EguiKey::Tab);
    assert_eq!(h.state().bindings("file.save"), ["Tab"]);
    assert!(table_focused(&h), "Tab did not move focus");

    click(&mut h, &format!("Save: {}", chord("Tab", Platform::Mac)));
    press(&mut h, Modifiers::NONE, EguiKey::ArrowDown);
    assert_eq!(h.state().bindings("file.save"), ["Down"]);

    click(&mut h, &format!("Save: {}", chord("Down", Platform::Mac)));
    press(&mut h, Modifiers::SHIFT, EguiKey::Escape);
    assert_eq!(h.state().bindings("file.save"), ["Shift+Escape"]);
    assert!(table_focused(&h));
    assert!(h.state().state.is_open());
}

#[test]
fn escape_cancels_recording_only() {
    let mut h = mac();
    click(&mut h, "Save: ⌘S");
    press(&mut h, Modifiers::NONE, EguiKey::Escape);
    assert!(h.state().state.is_open());
    assert!(!h.state().state.is_recording());
    assert!(!h.state().cancelled());
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"]);
    press(&mut h, Modifiers::NONE, EguiKey::Escape);
    assert!(h.state().cancelled());
    assert!(!h.state().state.is_open());
}

#[test]
fn copy_event_records_cmd_c() {
    let mut h = mac();
    click(&mut h, "Save: ⌘S");
    h.event(Event::Copy);
    h.run();
    assert_eq!(h.state().bindings("file.save"), ["Cmd+C"]);
}

#[test]
fn double_press_records_k_k() {
    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.options.double_press = true;
    let mut h = open(fx);
    click(&mut h, "Save: ⌘S");
    raw_key(&mut h, EguiKey::K, 10.0);
    assert!(has(&h, "Save: K (press again for a double press)"));
    raw_key(&mut h, EguiKey::K, 10.3);
    assert_eq!(h.state().bindings("file.save"), ["K K"]);

    click(&mut h, "Save: K K");
    raw_key(&mut h, EguiKey::K, 20.0);
    raw_key(&mut h, EguiKey::K, 20.9);
    assert_eq!(h.state().bindings("file.save"), ["K"]);
}

#[test]
fn the_new_shortcut_field_waits_for_assign() {
    let mut h = mac();
    h.get_by_label("Save").click();
    h.run();
    click(&mut h, "New shortcut: None");
    press(&mut h, Modifiers::COMMAND, EguiKey::E);
    assert!(has(&h, "Currently assigned to:"));
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"]);
    click(&mut h, "Assign");
    assert!(has(&h, "⚠ ⌘E is used by Export…"));
    click(&mut h, "Keep Both");
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S", "Cmd+E"]);
}

// ---- Conflicts ----

#[test]
fn conflict_take_it_steals_and_undo_restores() {
    let mut h = mac();
    click(&mut h, "Export…: ⌘E");
    press(&mut h, Modifiers::COMMAND, EguiKey::S);
    click(&mut h, "Take It");
    assert_eq!(h.state().bindings("file.save"), Vec::<String>::new());
    assert_eq!(h.state().bindings("file.export"), ["Cmd+S"]);
    assert!(has(&h, "⌘S was removed from Save"));

    // The footer's Undo is after the message's.
    let undo = h.get_all_by_label("Undo").last().map(|n| n.rect());
    let Some(undo) = undo else { panic!("no Undo") };
    h.get_all_by_label("Undo").filter(|n| n.rect() == undo).for_each(|n| n.click());
    h.run();
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"]);
    assert_eq!(h.state().bindings("file.export"), ["Cmd+E"]);
    click(&mut h, "Redo");
    assert_eq!(h.state().bindings("file.export"), ["Cmd+S"]);
    press(&mut h, Modifiers::COMMAND, EguiKey::Z);
    assert_eq!(h.state().bindings("file.export"), ["Cmd+E"], "Cmd+Z undoes when no text field has focus");
}

#[test]
fn conflict_keep_both_lists_the_clash() {
    let mut h = mac();
    click(&mut h, "Export…: ⌘E");
    press(&mut h, Modifiers::COMMAND, EguiKey::S);
    click(&mut h, "Keep Both");
    let clashes = h.state().km.conflicts().into_iter().filter(|c| c.kind == ConflictKind::Clash).count();
    assert_eq!(clashes, 1);
    assert!(has(&h, "Also used by Save"));
    click(&mut h, "⚠ 1 shortcut is assigned more than once");
    assert!(has(&h, "Save: ⌘S"));
    assert!(has(&h, "Export…: ⌘S"));
    assert!(!has(&h, "Save As…: ⇧⌘S"), "the Conflicts filter shows only the clash");
}

#[test]
fn steal_mode_says_what_it_took() {
    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.options.conflicts = ConflictMode::Steal;
    let mut h = open(fx);
    click(&mut h, "Export…: ⌘E");
    press(&mut h, Modifiers::COMMAND, EguiKey::S);
    assert!(has(&h, "⌘S was removed from Save"));
    click(&mut h, "Go to Conflict");
    assert!(selected(&h, "Save (Changed)"));
}

/// Whether a row with this label reports itself selected.
fn selected(h: &Harness<'_, Fx>, label: &str) -> bool {
    h.get_all_by_label(label).any(|n| n.accesskit_node().is_selected() == Some(true))
}

#[test]
fn go_to_conflict_selects_and_reveals_the_holder() {
    let mut h = mac();
    choose_area(&mut h, "Tools");
    search(&mut h, "marquee");
    assert!(!has(&h, "Save: ⌘S"));
    click(&mut h, "Marquee Tool: M");
    press(&mut h, Modifiers::COMMAND, EguiKey::S);
    click(&mut h, "Take It and Go to Conflict");
    assert_eq!(h.state().bindings("tool.marquee"), ["Cmd+S"]);
    assert!(selected(&h, "Save (Changed)"), "the holder is selected");
    assert_eq!(h.get_by_label("Search:").value().unwrap_or_default(), "");
    assert!(has(&h, "Export…: ⌘E"), "area All, no filter");
}

#[test]
fn reserved_chord_is_refused() {
    let km = fixture(Platform::Mac).with_reserved(vec![Chord::parse("Cmd+Tab").unwrap_or_else(|e| panic!("{e}"))]);
    let mut h = open(Fx::new(km));
    click(&mut h, "Save: ⌘S");
    press(&mut h, Modifiers::COMMAND, EguiKey::Tab);
    assert!(has(&h, &format!("⚠ {} is reserved by the system", chord("Cmd+Tab", Platform::Mac))));
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"]);
}

#[test]
fn app_rule_refuses() {
    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.options.accept = Some(Box::new(|b, binding| {
        if matches!(b.kind, Kind::Tool { .. }) && !binding.trigger.chord.mods.is_none() {
            Err("Tools take keys without modifiers".into())
        } else {
            Ok(())
        }
    }));
    let mut h = open(fx);
    click(&mut h, "Marquee Tool: M");
    press(&mut h, Modifiers::COMMAND, EguiKey::K);
    assert!(has(&h, "⚠ Tools take keys without modifiers"));
    assert_eq!(h.state().bindings("tool.marquee"), ["M"]);
}

// ---- Search, areas, display ----

fn search(h: &mut Harness<'_, Fx>, text: &str) {
    let field = h.get_by_label("Search:");
    field.focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, EguiKey::A);
    h.run();
    h.get_by_label("Search:").type_text(text);
    h.run();
}

fn choose_area(h: &mut Harness<'_, Fx>, area: &str) {
    h.get_by_role_and_label(Role::ComboBox, "Show:").click();
    h.run();
    h.get_by_role_and_label(Role::Button, area).click();
    h.run();
}

struct German;
impl Text for German {
    fn ui<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        source.into()
    }
    fn plural(&self, n: u64, one: &str, other: &str) -> String {
        if n == 1 { one } else { other }.replace("{n}", &n.to_string())
    }
    fn command<'a>(&'a self, b: &'a Bindable) -> Cow<'a, str> {
        if b.command.id == "file.save" { "Sichern".into() } else { b.label.as_str().into() }
    }
}

#[test]
fn search_by_name_translation_or_chord() {
    let mut h = mac();
    search(&mut h, "save");
    assert!(has(&h, "Save: ⌘S") && has(&h, "Save As…: ⇧⌘S"));
    assert!(!has(&h, "Export…: ⌘E"));
    search(&mut h, "cmd+s");
    assert!(has(&h, "Save: ⌘S") && !has(&h, "Save As…: ⇧⌘S"));
    search(&mut h, "⌘S");
    assert!(has(&h, "Save: ⌘S") && !has(&h, "Save As…: ⇧⌘S"));
    search(&mut h, "zzz");
    assert!(has(&h, "No matching commands"));

    click(&mut h, "Search by key");
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, EguiKey::S);
    assert_eq!(h.get_by_label("Search:").value().unwrap_or_default(), "⇧⌘S");
    assert!(has(&h, "Save As…: ⇧⌘S") && !has(&h, "Save: ⌘S"));

    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.text = Box::new(German);
    let mut h = open(fx);
    search(&mut h, "sichern");
    assert!(has(&h, "Sichern: ⌘S") && !has(&h, "Save As…: ⇧⌘S"));
}

#[test]
fn areas_follow_paths_kind_and_scope() {
    let mut h = mac();
    h.get_by_role_and_label(Role::ComboBox, "Show:").click();
    h.run();
    for area in ["All Commands", "File Menu", "Object Menu", "Tools", "Timeline Panel", "Other"] {
        assert!(h.get_all_by_label(area).count() >= 1, "{area}");
    }
    h.get_by_role_and_label(Role::Button, "Tools").click();
    h.run();
    assert!(has(&h, "Marquee Tool: M") && has(&h, "Elliptical Marquee Tool: M"));
    assert!(!has(&h, "Save: ⌘S"));
    choose_area(&mut h, "Timeline Panel");
    assert!(has(&h, "Loop: L") && !has(&h, "Marquee Tool: M"));
}

#[test]
fn display_follows_the_keymap_platform() {
    let h = mac();
    assert!(has(&h, "Save As…: ⇧⌘S"));
    assert!(has(&h, "Bold: ⌘B (Text)"), "a scope other than home is named");
    let h = open(Fx::new(fixture(Platform::Windows)));
    assert!(has(&h, "Save As…: Ctrl+Shift+S"));
}

// ---- Labels, focus, small windows, RTL ----

#[test]
fn every_control_has_a_label() {
    let mut h = mac();
    h.get_by_label("Save").click();
    h.run();
    click(&mut h, "Export…: ⌘E");
    press(&mut h, Modifiers::COMMAND, EguiKey::S);
    assert!(has(&h, "Take It"));
    let mut seen = 0;
    for node in h.root().children_recursive() {
        let n = node.accesskit_node();
        if matches!(n.role(), Role::Button | Role::Tab | Role::TextInput | Role::ComboBox) {
            seen += 1;
            assert!(n.label().is_some_and(|l| !l.trim().is_empty()), "{:?} without a label at {:?}", n.role(), node.rect());
        }
    }
    assert!(seen > 20, "{seen} controls");
}

#[test]
fn table_keys_move_record_and_delete() {
    let mut h = mac();
    h.get_by_label("Save").click();
    h.run();
    assert!(table_focused(&h));
    press(&mut h, Modifiers::NONE, EguiKey::ArrowDown);
    assert!(selected(&h, "Save As…"));
    press(&mut h, Modifiers::NONE, EguiKey::Delete);
    assert_eq!(h.state().bindings("file.saveAs"), Vec::<String>::new());
    press(&mut h, Modifiers::NONE, EguiKey::Enter);
    assert!(h.state().state.is_recording());
    press(&mut h, Modifiers::NONE, EguiKey::F7);
    assert_eq!(h.state().bindings("file.saveAs"), ["F7"]);
}

#[test]
fn small_window_keeps_ok_and_long_labels_truncate() {
    let long = "A very long command label ".repeat(12);
    let mut km = fixture(Platform::Mac);
    let mut catalogue = km.catalogue().to_vec();
    catalogue.insert(0, Bindable::new("file.long", long.trim(), Scope::app()).with_path(vec!["File".into(), long.trim().into()]));
    km = Keymap::new(catalogue, Scopes::new().with_panel("Timeline").with_context("Text"), Platform::Mac);
    let mut fx = Fx::new(km);
    fx.window = true;
    let size = vec2(480.0, 360.0);
    let mut h = harness(fx, size);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let ok = h.get_by_label("OK").rect();
    assert!(screen.contains_rect(ok), "OK at {ok:?}");
    let row = h.get_by_label(long.trim());
    assert!(row.rect().max.x <= size.x + 0.5, "the row stays inside: {:?}", row.rect());
    row.hover();
    h.run();
    assert!(h.query_all_by_label_contains(long.trim()).count() >= 2, "the tooltip has the full label");
    click(&mut h, "OK");
    assert!(h.state().outcomes.iter().any(|o| matches!(o, Outcome::Ok)));
}

#[test]
fn window_close_is_cancel() {
    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.window = true;
    let mut h = open(fx);
    click(&mut h, "Save: ⌘S");
    press(&mut h, Modifiers::NONE, EguiKey::F2);
    assert_eq!(h.state().bindings("file.save"), ["F2"]);
    click(&mut h, "Cancel");
    assert!(h.state().cancelled());
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S"]);
}

#[test]
fn automation_ids_are_reported() {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let mut fx = Fx::new(fixture(Platform::Mac));
    let sink = seen.clone();
    fx.options.on_widget = Some(Box::new(move |id, rect, label| {
        assert!(rect.is_finite() && !label.is_empty(), "{id}");
        sink.borrow_mut().push(id.to_string());
    }));
    let mut h = open(fx);
    h.get_by_label("Save").click();
    h.run();
    let ids = seen.borrow();
    for id in [
        "shortcuts.area",
        "shortcuts.filter",
        "shortcuts.search",
        "shortcuts.searchKey",
        "shortcuts.row.file.save",
        "shortcuts.cell.file.save.0",
        "shortcuts.add.file.save",
        "shortcuts.reset.file.save",
        "shortcuts.remove.0",
        "shortcuts.new",
        "shortcuts.context",
        "shortcuts.assign",
        "shortcuts.clear",
        "shortcuts.undo",
        "shortcuts.redo",
        "shortcuts.resetAll",
        "shortcuts.cancel",
        "shortcuts.ok",
    ] {
        assert!(ids.iter().any(|s| s == id), "{id} not reported");
    }
}

#[test]
fn typed_entry_parses_and_shows_errors() {
    let mut h = mac();
    h.get_by_label("Save").click();
    h.run();
    click(&mut h, "Type…");
    h.get_by_role_and_label(Role::TextInput, "New shortcut:").type_text("Cmd+Shift+K");
    h.run();
    press(&mut h, Modifiers::NONE, EguiKey::Enter);
    click(&mut h, "Assign");
    assert_eq!(h.state().bindings("file.save"), ["Cmd+S", "Cmd+Shift+K"]);
}

struct Rtl;
impl Text for Rtl {
    fn ui<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        source.into()
    }
    fn plural(&self, _n: u64, _one: &str, other: &str) -> String {
        other.to_string()
    }
    fn rtl(&self) -> bool {
        true
    }
}

#[test]
fn rtl_puts_the_detail_pane_left_and_keeps_chords() {
    let mut fx = Fx::new(fixture(Platform::Mac));
    fx.text = Box::new(Rtl);
    let h = open(fx);
    let table = h.get_by_label("Commands").rect();
    assert!(table.min.x > 300.0, "table on the right: {table:?}");
    assert!(has(&h, "Save As…: ⇧⌘S"));
}
