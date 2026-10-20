# Craft Keymap

`craft-keymap` holds the data and logic for keyboard bindings in the Craft apps: chords and their
text forms, bindings from chords to commands, scopes, conflicts, user overrides, and an import
pipeline for other applications' shortcut sets. Each application supplies its command catalog,
its file-format readers and its UI.

## Boundaries

- Data and logic only. The apps keep the command catalog (ids, labels, menu paths, defaults),
  the readers for other applications' files, the shortcut editor UI and running the commands.
- Never crashes: no panics, no `unsafe`. Loading settings never fails; what cannot be used is
  kept or reported (`Keymap::problems`, the import report).
- Hostile input is capped: commands, bindings per command, text lengths, import entries, keys,
  path segments and fuzzy suggestions (the `MAX_*` constants). What is cut is counted.
- Reserved chords are opt-in. A keymap reserves no chord unless the app passes a list to
  `Keymap::with_reserved`, e.g. `reserved::system(platform)` for the operating system's window
  switching and closing chords. A reserved chord cannot be assigned or bound.
- Dependencies: `serde` and `serde_json`; `egui` with the `egui` feature.

## A catalog and dispatch

A command with parameters has the key `window.panel {"panel":"layers"}` in settings and reports.
Tools of one group share a key and cycle. A binding can name a scope other than the command's
home scope.

```rust
use craft_keymap::{Active, Bindable, Binding, Chord, CommandRef, Keymap, Kind, Lookup, Platform, Scope, Scopes};
use serde_json::json;

let key = |text: &str| Binding::parse(text).unwrap();
let layers = CommandRef::with_params("window.panel", json!({"panel": "layers"}));
let select = Kind::Tool { group: Some("select".into()) };
let catalog = vec![
    Bindable::new("file.save", "Save", Scope::app()).with_path(vec!["File".into(), "Save".into()]).with_default(key("Cmd+S")),
    Bindable::new(layers.clone(), "Layers", Scope::app()).with_path(vec!["Window".into(), "Layers".into()]).with_default(key("F7")),
    Bindable::new("tool.marquee", "Marquee Tool", Scope::app()).with_kind(select.clone()).with_default(key("M")),
    Bindable::new("tool.ellipse", "Elliptical Marquee Tool", Scope::app()).with_kind(select).with_default(key("M")),
    // Home scope: the Timeline panel, where it wins over the app's L.
    Bindable::new("timeline.loop", "Loop", Scope::new("Timeline").unwrap()).with_default(key("L")),
    Bindable::new("layer.lock", "Lock Layer", Scope::app()).with_default(key("L")),
    // An app command whose binding works only in the Text context.
    Bindable::new("type.bold", "Bold", Scope::app()).with_default(key("Text: Cmd+B")),
];
let keymap = Keymap::new(catalog, Scopes::new().with_panel("Timeline").with_context("Text"), Platform::Mac);

// Dispatch: the chord pressed, whether it is a double press, and what has the keyboard.
let press = |text: &str, active: &Active| keymap.lookup(&Chord::parse(text).unwrap(), false, active);
assert_eq!(press("F7", &Active::NONE), Lookup::Run(layers));
assert_eq!(press("L", &Active::panel("Timeline")), Lookup::Run("timeline.loop".into()));
assert_eq!(press("L", &Active::NONE), Lookup::Run("layer.lock".into()));
assert_eq!(press("M", &Active::NONE), Lookup::Cycle(vec!["tool.marquee".into(), "tool.ellipse".into()]));
let editing = Active { contexts: &["Text"], typing: true, ..Active::NONE };
assert_eq!(press("Cmd+B", &editing), Lookup::Run("type.bold".into()));
assert_eq!(press("M", &editing), Lookup::None, "plain keys go to the text field");
```

While a text field has the keyboard (`Active::typing`), the keymap's `TypingRule` decides which
app and panel bindings wait: plain keys (the default), everything without the command modifier,
or everything. Bindings in an active context always fire, so `Text: Cmd+B` works while typing.

With the `egui` feature, `egui::Dispatcher` does this for a frame's key events (see Features).

## Chords

A [`Chord`](src/chord.rs) is a key plus the modifiers `cmd`, `ctrl`, `alt` and `shift`. `cmd` is
the command modifier: ⌘ on macOS, Ctrl on Windows and Linux. `ctrl` is the Mac Control key; off
macOS it folds into `cmd` (`Chord::effective`).

- Canonical text, as stored and sent: `Cmd+Ctrl+Alt+Shift+K`, `Cmd++` for Plus, `A A` for a
  double press.
- `Chord::parse` also reads common aliases (`⇧⌘Z`, `opt+left`, `Ctrl+Cmd+M`).
- `Chord::display(Platform)` gives `⇧⌘Z` on macOS and `Ctrl+Shift+Z` elsewhere.
- `chord::foreign::read_foreign` reads key text written by other applications (Windows or Mac
  modifier names, numeric keypad keys).

## Storing overrides

`Overrides` is a field of the app's settings type. It is stored as a map of one text per command
(`{ "file.save": "Cmd+Alt+S", "edit.copy": "" }`, `""` for no shortcut) while no command has more
than one binding, so apps that stored that map before keep reading it in older builds. Once a
command has several bindings it is stored as an array of `{ command, keys }` tables
(`[{ command = "file.save", keys = ["Cmd+Alt+S", "F12"] }]`). Both forms load, and in the map form
a list of texts loads too.

In a craft-settings type, put `#[serde(with = "craft_keymap::keymap::overrides_list")]` on the
field, which always writes the array: craft-settings merges tables key by key with what the file
holds but replaces an array whole, so only the array lets a reset command (an entry removed) stay
removed after saving. Hand-written `[[shortcuts]]` blocks load as the array.

Texts the keymap cannot use (a key name from a newer version, an unknown scope, a reserved chord)
stay in the entry when the command is edited, unless the edit leaves it without bindings.
`Keymap::problems` lists them.

```rust
use craft_keymap::{Binding, Keymap, Overrides, Platform, Policy, Scopes};
use craft_settings::{Location, Settings, SettingsStore};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default)]
struct Prefs {
    #[serde(with = "craft_keymap::keymap::overrides_list")]
    shortcuts: Overrides,
}

impl Settings for Prefs {
    const APP_ID: &'static str = "acmecraft";
    const SCHEMA_VERSION: u32 = 1;
}

// `catalog` is the app's Vec<Bindable>.
let (mut store, _report) = SettingsStore::<Prefs>::open(Location::Auto);
let mut keymap = Keymap::new(catalog, Scopes::new(), Platform::current()).with_overrides(store.get().shortcuts.clone());
// Entries for unknown commands and texts that do not parse are kept and listed here.
let _problems = keymap.problems();

// After each change in the shortcut editor:
keymap.set_bindings(&"file.save".into(), vec![Binding::parse("Cmd+Alt+S").unwrap()], Policy::Steal).ok();
store.update(|p| p.shortcuts = keymap.overrides().clone());
```

## Editing

- `set_bindings(&command, bindings, policy)` replaces a command's bindings. An editor with one
  shortcut per command calls it with the one binding (`vec![binding]`); `assign` adds a binding
  to those the command has.
- `remove` takes one binding away, `clear` leaves the command without shortcuts, `reset` gives it
  its defaults back and `reset_all` does so for every command.
- `Policy` says what happens to other commands holding the chord: `Steal` takes it from them,
  `Keep` lets both keep it and reports the clash, `Refuse` changes nothing and returns
  `AssignError::Taken`.
- `conflicts()` lists every pair of commands on one chord; `conflicts_with(&command)` lists the
  commands that share one command's chords, with that command as `a`.

## Importing a shortcut set

The app's reader turns a file (an InDesign Show Set, an Illustrator `.kys`) into an
[`import::ForeignSet`](src/import.rs): entries with a menu, a path and keys read by
`ForeignKey::read`. `import::plan` matches each entry to one command and changes nothing;
`ImportPlan::apply` applies the plan and returns an `ImportReport` (serde, camelCase). Report
rows carry the entry's `item` (its path) and `line`.

```rust
use craft_keymap::{Bindable, Binding, ContextRule, Flavor, ForeignEntry, ForeignKey, ForeignSet, ImportOptions, Keymap, Platform};
use craft_keymap::{Scope, Scopes, plan};

let save = Bindable::new("file.save", "Save", Scope::app())
    .with_path(vec!["File".into(), "Save".into()])
    .with_default(Binding::parse("Cmd+S").unwrap());
let mut keymap = Keymap::new(vec![save], Scopes::new(), Platform::Windows);

let read = |text: &str| ForeignKey::read(Some("Default"), text, Flavor::Windows);
let set = ForeignSet {
    name: Some("My Set".into()),
    entries: vec![
        ForeignEntry::new(Some("File"), ["Save"]).with_key(read("Ctrl+Alt+S")).at_line(12),
        ForeignEntry::new(Some("File"), ["Frobnicate"]).with_key(read("Ctrl+F")).at_line(13),
    ],
};
let options = ImportOptions { contexts: vec![("Default".into(), ContextRule::Home)], ..ImportOptions::default() };
let planned = plan(&set, &keymap, &options);
// A review UI can show planned.commands and planned.report here.
let report = planned.apply(&mut keymap);
assert_eq!(report.changed[0].to, ["Cmd+Alt+S"]);
assert_eq!((report.unmatched[0].item.as_str(), report.unmatched[0].line), ("File > Frobnicate", Some(13)));
```

- Matching, most specific first, each step needing exactly one command: the user's remembered
  answers, the full menu path, the joined submenu segments as a label or alias in that menu, the
  label in that menu, the joined segments as a label or alias anywhere, the label anywhere. An
  entry with no menu and several segments matches only by its full path, unless the reader marks
  it `in_panel_menu()`. Tool entries match only tools.
- Names are compared after `import::normalize_label`: case, `…`, `...`, `&`, colons and extra
  spaces aside, so the alias `Preferences: General…` matches `Edit > Preferences > General...`.
  Aliases (`Bindable::with_alias`) count wherever labels do.
- Panel menus: sources list panel-menu items with the panel's name in front (InDesign's Panel
  Menus area writes `Character: All Caps`; Illustrator and Photoshop sets have similar areas).
  The reader splits the name into segments and marks the entry `ForeignEntry::in_panel_menu()`.
  Such an entry also matches a catalog path that ends with its segments (with the command's
  label or the path's last segment last), and its last segment alone as a label. The last
  segment alone counts only when the first segment, the panel, is a segment of some catalog
  path, so `History: New Document` matches nothing in an app without a History panel. An entry
  without keys that matched only these ways leaves the command's shortcuts alone and is listed
  in `matched_without_change` (reason `PanelMenu`).
- Show/Hide: a source often lists one toggle as two items, `Show Rulers` and `Hide Rulers`. Names
  are matched as written first. When no step finds a command, the steps run again on commands
  marked `Bindable::toggle()`, with a leading `Show `, `Hide ` or `Show/Hide ` dropped from the
  entry's label and from the command's label, aliases and path labels. A command not marked as a
  toggle never matches through a dropped prefix, so `Hide Conveyor` does not match `Conveyor`.
- `ImportOptions.contexts` says what each source context becomes (home scope, another scope, the
  home scope only without a home entry, or skipped).
- The set wins: a chord it gives to a command is taken from commands outside it (except twins
  sharing it as a default), a later entry takes a chord from an earlier one (as when the entries
  are applied in file order; the earlier key is listed in `skipped`), and an entry without keys
  can clear its command. It clears it only when no entry matched to the same command has keys,
  whatever the order of the entries.
- Keys read approximately (a numeric keypad key bound as the main key) are listed in
  `approximated`.
- With `ImportOptions.suggest`, unmatched entries get fuzzy candidates (`import::namematch`);
  they are reported, never applied. Only the first 200 unmatched entries with keys get them, and
  only the first 200 characters of a name are compared. Keys in `ContextRule::Skip` contexts do
  not count: an unmatched entry with only such keys counts in `unmatched_without_shortcut`.

## Features

- `egui`: conversions to and from egui keys and modifiers; `egui::matches`, which accepts the
  shifted key egui reports for a Shift binding (`Shift+;` matches `:`); and `egui::Dispatcher`,
  which runs a frame's key presses against the keymap:

  ```rust,ignore
  let hits = ctx.input_mut(|i| dispatcher.dispatch(i, &keymap, &active));
  for hit in hits {
      app.run(&hit.command);
  }
  ```

  A press that runs a command is removed from egui's events, with the text it typed, so widgets
  do not see it too. Each `Hit` carries the `trigger` that matched and `exact`, which is false
  when the press matched through a stand-in (`:` for `Shift+;`, `+` for `=`).
  `dispatch_filtered` takes a predicate that can decline a hit, for example a chord the native
  menu has already handled: the press and its text then stay in egui's events, in their place.
  While a text field has the keyboard, clipboard key presses and presses that typed text in the
  same frame stay with it. That covers AltGr, which a browser on Windows reports as Ctrl+Alt
  followed by the text; a Ctrl+Alt press that types nothing is a command chord.
  `Dispatcher::new().with_repeat(true)` also runs commands on key repeats (held arrows).

  egui's Copy, Cut and Paste events stay with the app by default. On the desktop, egui-winit
  sends them in place of the Cmd+C, Cmd+X and Cmd+V presses (Ctrl off macOS), so a binding on
  those chords runs only with `with_clipboard_events(true)`. Then the events count as their
  chords outside text fields, and a Paste event's text moves to `Hit::pasted`. The web build
  sends the key press and then the event; the press runs the command and takes the event, with
  its text, along. An app whose
  paste command asks egui for a paste gets a Paste event back, which runs that command again;
  such an app keeps the default.
