//! The Keyboard Shortcuts dialog (feature `ui`): a table of the catalogue with recording,
//! conflicts, undo, named sets and import review, over [`Keymap`] and optionally [`Profiles`].
//!
//! The app keeps a [`DialogState`] while the dialog is open and calls [`ShortcutsDialog::show`]
//! (or [`ShortcutsDialog::ui`] inside its own container) every frame. While
//! [`DialogState::is_open`] is true the app skips its own key dispatch; the dialog takes the
//! frame's key events itself while it records. The app saves on [`Outcome::Ok`] (or on
//! [`Outcome::Changed`] for live saving) and drops the state once it is closed.

use std::collections::BTreeMap;

use crate::chord::Trigger;
use crate::command::CommandRef;
use crate::import::{ForeignSet, ImportOptions};
use crate::keymap::{Bindable, Binding, Keymap};
use crate::profiles::{Builtins, ProfileError, Profiles};

// The view draws with these; until it calls every action some stay unused.
#[expect(dead_code, reason = "actions and row fields the view calls")]
mod capture;
#[expect(dead_code, reason = "actions and row fields the view calls")]
mod rows;
#[expect(dead_code, reason = "actions and row fields the view calls")]
mod state;
mod text;

pub use state::DialogState;
pub use text::{English, I18nText, PLURALS, STRINGS, Text, trigger_text};

use state::Env;

/// What the dialog reports for a frame.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Closed with OK: save `Profiles` (with sets) or `keymap.overrides()`.
    Ok,
    /// Closed with Cancel: the keymap and profiles are back as they were when it opened.
    Cancel,
    /// An edit this frame (for live menus and live saving).
    Changed,
    /// The user asked to import: the app opens a file of one of these formats and calls
    /// [`ShortcutsDialog::import_json`] or [`ShortcutsDialog::review_import`].
    Import { formats: Vec<&'static str> },
    /// A set as craft-keymap JSON, for the app to write.
    Export { name: String, json: String },
    /// A set in one of [`Options::export_formats`], for the app to write.
    ExportAs { format: &'static str, name: String, keymap: Box<Keymap> },
    /// The shortcut summary, for the app to show, save or print.
    Summary { name: String, html: String, text: String },
    /// Import answers the user gave (item → command), for the app to keep for later imports.
    Remembered(BTreeMap<String, CommandRef>),
}

/// What happens when a recorded chord is held by other commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictMode {
    /// Ask with the conflict bar: Take It, Take It and Go to Conflict, Keep Both, Cancel.
    #[default]
    Ask,
    /// Take the chord from the other commands, then say so.
    Steal,
    /// Keep both bindings, then say so.
    Keep,
}

/// An app's own Show entry.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub id: &'static str,
    pub label: &'static str,
    pub rows: fn(&Bindable) -> bool,
}

/// A read-only row for a key outside the catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedRow {
    pub group: String,
    pub label: String,
    /// The keys as shown.
    pub keys: String,
    /// The trigger, when the row has one, so a catalogue binding on it can be pointed out.
    pub chord: Option<Trigger>,
}

/// A file format an app reads or writes itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub id: &'static str,
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

/// The dialog's colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogStyle {
    pub accent: egui::Color32,
    pub warning: egui::Color32,
    pub dim: egui::Color32,
    pub changed: egui::Color32,
    pub app_key: egui::Color32,
    pub scoped_key: egui::Color32,
    pub both_key: egui::Color32,
    pub modifier_key: egui::Color32,
    pub free_key: egui::Color32,
}

impl DialogStyle {
    pub fn from_visuals(v: &egui::Visuals) -> DialogStyle {
        let accent = v.selection.bg_fill;
        DialogStyle {
            accent,
            warning: v.warn_fg_color,
            dim: v.weak_text_color(),
            changed: accent,
            app_key: accent,
            scoped_key: v.warn_fg_color,
            both_key: v.error_fg_color,
            modifier_key: v.widgets.active.bg_fill,
            free_key: v.widgets.inactive.bg_fill,
        }
    }
}

/// A rule an app adds for which bindings a command may get; `Err` carries the message shown.
pub type Accept = Box<dyn Fn(&Bindable, &Binding) -> Result<(), String>>;
/// Called for every control once per frame with (automation id, rect, label).
pub type OnWidget = Box<dyn Fn(&str, egui::Rect, &str)>;

/// How the dialog looks and behaves. [`Options::default`] gives the defaults noted on each field.
pub struct Options {
    /// "Keyboard Shortcuts".
    pub title: &'static str,
    /// true; false shows the table read-only with Close and Summarize.
    pub editable: bool,
    /// "Custom": the name edits of a built-in set are saved under.
    pub custom_set_name: &'static str,
    /// Ask.
    pub conflicts: ConflictMode,
    /// true: Keep Both is offered.
    pub allow_keep: bool,
    /// false: a recorded chord waits [`DOUBLE_PRESS`](crate::DOUBLE_PRESS) for a second press.
    pub double_press: bool,
    /// false: Backspace or Delete without modifiers removes the binding being recorded over.
    pub backspace_clears: bool,
    /// false: Enter when idle is OK.
    pub enter_is_ok: bool,
    /// false: rows sort by translated label instead of catalogue order.
    pub sort_by_label: bool,
    /// false: Show as tabs instead of a dropdown.
    pub area_tabs: bool,
    /// The app's own Show entries.
    pub areas: Vec<Area>,
    /// false: the on-screen keyboard pane.
    pub keyboard: bool,
    pub fixed: Vec<FixedRow>,
    pub import_formats: Vec<Format>,
    pub export_formats: Vec<Format>,
    /// true: Summarize is offered.
    pub summary: bool,
    pub accept: Option<Accept>,
    pub on_widget: Option<OnWidget>,
    /// `None`: [`DialogStyle::from_visuals`].
    pub style: Option<DialogStyle>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            title: "Keyboard Shortcuts",
            editable: true,
            custom_set_name: "Custom",
            conflicts: ConflictMode::Ask,
            allow_keep: true,
            double_press: false,
            backspace_clears: false,
            enter_is_ok: false,
            sort_by_label: false,
            area_tabs: false,
            areas: Vec::new(),
            keyboard: false,
            fixed: Vec::new(),
            import_formats: Vec::new(),
            export_formats: Vec::new(),
            summary: true,
            accept: None,
            on_widget: None,
            style: None,
        }
    }
}

/// The format id of craft-keymap's own JSON in [`Outcome::Import`].
pub const JSON_FORMAT: &str = "craft-keymap";

/// The dialog for one frame: what it edits, borrowed, and how it looks.
pub struct ShortcutsDialog<'a> {
    keymap: &'a mut Keymap,
    text: &'a dyn Text,
    sets: Option<(&'a mut Profiles, &'a Builtins<'a>)>,
    options: Option<&'a Options>,
}

impl<'a> ShortcutsDialog<'a> {
    pub fn new(keymap: &'a mut Keymap, text: &'a dyn Text) -> Self {
        ShortcutsDialog { keymap, text, sets: None, options: None }
    }

    /// Shows and edits named sets.
    pub fn sets(mut self, profiles: &'a mut Profiles, builtins: &'a Builtins<'a>) -> Self {
        self.sets = Some((profiles, builtins));
        self
    }

    pub fn options(mut self, options: &'a Options) -> Self {
        self.options = Some(options);
        self
    }

    fn with_env<R>(self, f: impl FnOnce(&mut Env) -> R) -> R {
        let fallback;
        let options = match self.options {
            Some(o) => o,
            None => {
                fallback = Options::default();
                &fallback
            }
        };
        let mut env = Env { keymap: self.keymap, sets: self.sets, options, text: self.text };
        f(&mut env)
    }

    /// A window with title, body and Cancel/OK.
    pub fn show(self, ctx: &egui::Context, state: &mut DialogState) -> Option<Outcome> {
        let title = self.text.ui(self.options.map_or("Keyboard Shortcuts", |o| o.title)).into_owned();
        egui::Window::new(title).collapsible(false).show(ctx, |ui| self.ui(ui, state)).and_then(|r| r.inner).flatten()
    }

    /// The body only, for a host's own modal, preferences page or backstage.
    pub fn ui(self, ui: &mut egui::Ui, state: &mut DialogState) -> Option<Outcome> {
        self.with_env(|env| {
            state.begin(env);
            state.input(env, ui.ctx());
            state.finish()
        })
    }

    /// OK, for hosts that draw their own button.
    pub fn ok(self, state: &mut DialogState) {
        self.with_env(|env| {
            state.begin(env);
            state.ok(env);
        });
    }

    /// Cancel, for hosts that draw their own button.
    pub fn cancel(self, state: &mut DialogState) {
        self.with_env(|env| {
            state.begin(env);
            state.cancel(env);
        });
    }

    /// Reads a craft-keymap JSON set the app loaded after [`Outcome::Import`]. With sets it becomes
    /// a new set, which is loaded; without, its overrides replace the keymap's as one undo step.
    /// Returns the set's name.
    pub fn import_json(self, state: &mut DialogState, text: &str) -> Result<String, ProfileError> {
        self.with_env(|env| {
            state.begin(env);
            state.import_json(env, text)
        })
    }

    /// Opens the review of a foreign set the app read: what will change, what did not match and
    /// why, with suggestions to pick from.
    pub fn review_import(self, state: &mut DialogState, set: ForeignSet, options: ImportOptions) {
        self.with_env(|env| {
            state.begin(env);
            state.review_import(env, set, options);
        });
    }
}
