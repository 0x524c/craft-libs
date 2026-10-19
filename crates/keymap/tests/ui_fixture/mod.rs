//! The catalogue the dialog tests and screenshots use.

use craft_keymap::{Bindable, Binding, Keymap, Kind, Platform, Scope, Scopes};

fn binding(text: &str) -> Binding {
    Binding::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

pub fn fixture(platform: Platform) -> Keymap {
    let tool = Kind::Tool { group: Some("Marquee".into()) };
    let path = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let timeline = Scope::new("Timeline").unwrap_or_else(Scope::app);
    let catalogue = vec![
        Bindable::new("file.save", "Save", Scope::app()).with_path(path(&["File", "Save"])).with_default(binding("Cmd+S")),
        Bindable::new("file.saveAs", "Save As…", Scope::app()).with_path(path(&["File", "Save As…"])).with_default(binding("Cmd+Shift+S")),
        Bindable::new("file.export", "Export…", Scope::app()).with_path(path(&["File", "Export…"])).with_default(binding("Cmd+E")),
        Bindable::new("object.front", "Bring to Front", Scope::app()).with_path(path(&["Object", "Arrange", "Bring to Front"])),
        Bindable::new("tool.marquee", "Marquee Tool", Scope::app()).with_kind(tool.clone()).with_default(binding("M")),
        Bindable::new("tool.ellipse", "Elliptical Marquee Tool", Scope::app()).with_kind(tool).with_default(binding("M")),
        Bindable::new("timeline.loop", "Loop", timeline).with_default(binding("L")),
        Bindable::new("type.bold", "Bold", Scope::app()).with_default(binding("Text: Cmd+B")),
    ];
    Keymap::new(catalogue, Scopes::new().with_panel("Timeline").with_context("Text"), platform)
}
