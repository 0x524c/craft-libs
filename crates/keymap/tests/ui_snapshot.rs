#![cfg(feature = "snapshot")]
//! Screenshots of the Keyboard Shortcuts dialog for review, not a golden comparison. With
//! `CRAFT_KEYMAP_SNAPSHOT_DIR` set, writes PNGs of a few states in light and dark at 1× and 2×;
//! without it the test does nothing.
//!
//! `CRAFT_KEYMAP_SNAPSHOT_DIR=/tmp/shots cargo test -p craft-keymap --features snapshot --test ui_snapshot`

use std::path::PathBuf;

use craft_keymap::ui::{DialogState, English, Options, ShortcutsDialog};
use craft_keymap::{ContextRule, Flavor, ForeignEntry, ForeignKey, ForeignSet, ImportOptions, Keymap, Overrides, Platform, Profiles};
use egui::{Key as EguiKey, Modifiers, Theme, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ui_fixture::fixture;

mod ui_fixture;

struct Fx {
    km: Keymap,
    options: Options,
    state: DialogState,
    sets: Option<(Profiles, Vec<(&'static str, Overrides)>)>,
}

impl Fx {
    fn dialog(&mut self) -> (ShortcutsDialog<'_>, &mut DialogState) {
        let Fx { km, options, state, sets } = self;
        let d = ShortcutsDialog::new(km, &English).options(options);
        let d = match sets {
            Some((p, b)) => d.sets(p, b),
            None => d,
        };
        (d, state)
    }
}

/// A named state of the dialog: window size and what to do before the picture.
struct Case {
    name: &'static str,
    size: Vec2,
    platform: Platform,
    /// Show named sets.
    sets: bool,
    /// Show the on-screen keyboard.
    keyboard: bool,
    act: fn(&mut Harness<'_, Fx>),
}

fn foreign_review(h: &mut Harness<'_, Fx>) {
    let read = |text: &str| ForeignKey::read(Some("Default"), text, Flavor::Windows);
    let set = ForeignSet {
        name: Some("Theirs".into()),
        entries: vec![
            ForeignEntry::new(Some("File"), ["Save"]).with_key(read("Ctrl+Alt+S")).at_line(3),
            ForeignEntry::new(Some("File"), ["Frobnicate"]).with_key(read("Ctrl+F")).at_line(4),
            ForeignEntry::new(Some("File"), ["Export…"]).with_key(read("Ctrl+Blorp")).at_line(5),
            ForeignEntry::new(Some("Object"), ["Arrange", "Bring to Front"]).with_key(read("Num 7")).at_line(6),
            ForeignEntry::new(Some("File"), ["Sve Az"]).with_key(read("Ctrl+F9")).at_line(7),
        ],
    };
    let options = ImportOptions { contexts: vec![("Default".into(), ContextRule::Home)], suggest: true, ..ImportOptions::default() };
    let (d, state) = h.state_mut().dialog();
    d.review_import(state, set, options);
    h.run();
}

fn click(h: &mut Harness<'_, Fx>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

const CASES: &[Case] = &[
    Case {
        name: "selected",
        size: Vec2::new(1000.0, 640.0),
        platform: Platform::Mac,
        sets: false,
        keyboard: false,
        act: |h| {
            h.get_by_label("Save As…").click();
            h.run();
        },
    },
    Case {
        name: "conflict",
        size: Vec2::new(1000.0, 640.0),
        platform: Platform::Mac,
        sets: false,
        keyboard: false,
        act: |h| {
            click(h, "Export…: ⌘E");
            h.key_press_modifiers(Modifiers::COMMAND, EguiKey::S);
            h.run();
        },
    },
    Case {
        name: "recording",
        size: Vec2::new(1000.0, 640.0),
        platform: Platform::Windows,
        sets: false,
        keyboard: false,
        act: |h| click(h, "Save: Ctrl+S"),
    },
    Case {
        name: "narrow",
        size: Vec2::new(560.0, 640.0),
        platform: Platform::Mac,
        sets: false,
        keyboard: false,
        act: |h| {
            h.get_by_label("Save").click();
            h.run();
        },
    },
    Case {
        name: "sets",
        size: Vec2::new(1000.0, 640.0),
        platform: Platform::Mac,
        sets: true,
        keyboard: false,
        act: |h| {
            click(h, "Save: ⌘S");
            h.key_press(EguiKey::F2);
            h.run();
            click(h, "New Set…");
        },
    },
    Case { name: "sets-narrow", size: Vec2::new(560.0, 640.0), platform: Platform::Mac, sets: true, keyboard: false, act: |h| click(h, "More") },
    Case {
        name: "keyboard",
        size: Vec2::new(1100.0, 720.0),
        platform: Platform::Mac,
        sets: false,
        keyboard: true,
        act: |h| {
            click(h, "Save As…");
            click(h, "⌘ Command");
            click(h, "⌘S: Save");
        },
    },
    Case {
        name: "keyboard-windows",
        size: Vec2::new(860.0, 640.0),
        platform: Platform::Windows,
        sets: false,
        keyboard: true,
        act: |h| click(h, "L: Loop (Timeline)"),
    },
    Case { name: "review", size: Vec2::new(1000.0, 640.0), platform: Platform::Mac, sets: true, keyboard: false, act: foreign_review },
];

#[test]
fn dialog_screenshots() {
    let Some(dir) = std::env::var_os("CRAFT_KEYMAP_SNAPSHOT_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    // `CRAFT_KEYMAP_SNAPSHOT_ONLY=keyboard` renders only the cases whose name starts with it.
    let only = std::env::var("CRAFT_KEYMAP_SNAPSHOT_ONLY").unwrap_or_default();
    for case in CASES.iter().filter(|c| c.name.starts_with(&only)) {
        for (theme_name, theme) in [("light", Theme::Light), ("dark", Theme::Dark)] {
            for scale in [1.0, 2.0] {
                let sets = case.sets.then(|| (Profiles::default(), vec![("Fixture Defaults", Overrides::new())]));
                let options = Options { keyboard: case.keyboard, ..Options::default() };
                let fx = Fx { km: fixture(case.platform), options, state: DialogState::new(), sets };
                let mut h = Harness::builder().with_size(case.size).with_pixels_per_point(scale).with_theme(theme).wgpu().build_ui_state(
                    |ui, fx: &mut Fx| {
                        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                            let (d, state) = fx.dialog();
                            d.ui(ui, state);
                        });
                    },
                    fx,
                );
                h.run();
                (case.act)(&mut h);
                let image = h.render().unwrap_or_else(|e| panic!("render {}: {e}", case.name));
                let path = dir.join(format!("shortcuts-{}-{theme_name}@{scale}x.png", case.name));
                image.save(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
        }
    }
}
