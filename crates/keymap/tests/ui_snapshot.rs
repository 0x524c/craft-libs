#![cfg(feature = "snapshot")]
//! Screenshots of the Keyboard Shortcuts dialog for review, not a golden comparison. With
//! `CRAFT_KEYMAP_SNAPSHOT_DIR` set, writes PNGs of a few states in light and dark at 1× and 2×;
//! without it the test does nothing.
//!
//! `CRAFT_KEYMAP_SNAPSHOT_DIR=/tmp/shots cargo test -p craft-keymap --features snapshot --test ui_snapshot`

use std::path::PathBuf;

use craft_keymap::ui::{DialogState, English, Options, ShortcutsDialog};
use craft_keymap::{Keymap, Platform};
use egui::{Key as EguiKey, Modifiers, Theme, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use ui_fixture::fixture;

mod ui_fixture;

struct Fx {
    km: Keymap,
    options: Options,
    state: DialogState,
}

/// A named state of the dialog: window size and what to do before the picture.
struct Case {
    name: &'static str,
    size: Vec2,
    platform: Platform,
    act: fn(&mut Harness<'_, Fx>),
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
        act: |h| {
            h.get_by_label("Save As…").click();
            h.run();
        },
    },
    Case {
        name: "conflict",
        size: Vec2::new(1000.0, 640.0),
        platform: Platform::Mac,
        act: |h| {
            click(h, "Export…: ⌘E");
            h.key_press_modifiers(Modifiers::COMMAND, EguiKey::S);
            h.run();
        },
    },
    Case { name: "recording", size: Vec2::new(1000.0, 640.0), platform: Platform::Windows, act: |h| click(h, "Save: Ctrl+S") },
    Case {
        name: "narrow",
        size: Vec2::new(560.0, 640.0),
        platform: Platform::Mac,
        act: |h| {
            h.get_by_label("Save").click();
            h.run();
        },
    },
];

#[test]
fn dialog_screenshots() {
    let Some(dir) = std::env::var_os("CRAFT_KEYMAP_SNAPSHOT_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for case in CASES {
        for (theme_name, theme) in [("light", Theme::Light), ("dark", Theme::Dark)] {
            for scale in [1.0, 2.0] {
                let fx = Fx { km: fixture(case.platform), options: Options::default(), state: DialogState::new() };
                let mut h = Harness::builder().with_size(case.size).with_pixels_per_point(scale).with_theme(theme).wgpu().build_ui_state(
                    |ui, fx: &mut Fx| {
                        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                            ShortcutsDialog::new(&mut fx.km, &English).options(&fx.options).ui(ui, &mut fx.state);
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
