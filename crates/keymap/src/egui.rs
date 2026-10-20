//! Conversions between the crate's keys and egui's, matching egui key presses to chords, and a
//! [`Dispatcher`] that turns a frame's key events into the commands they run. [`platform`] gives
//! the platform to build a keymap for, the browser's operating system included on the web.

use ::egui::os::OperatingSystem;
use ::egui::{Context, Event, InputState, Key as EguiKey, KeyboardShortcut, Modifiers};

use crate::chord::{Chord, Key, Mods, Platform, Trigger};
use crate::command::CommandRef;
use crate::keymap::{Keymap, Lookup};
use crate::scope::Active;

/// The platform to build a keymap for in this context. Unlike [`Platform::current`], a web build
/// gets the browser's operating system, which eframe reads from the user agent, so a Mac browser
/// shows and matches Cmd shortcuts. eframe sets it before the first frame; an app that builds its
/// keymap earlier rebuilds it when this differs.
pub fn platform(ctx: &Context) -> Platform {
    Platform::from(ctx.os())
}

/// iOS counts as Mac; Android and unknown systems count as Linux.
impl From<OperatingSystem> for Platform {
    fn from(os: OperatingSystem) -> Platform {
        match os {
            OperatingSystem::Mac | OperatingSystem::IOS => Platform::Mac,
            OperatingSystem::Windows => Platform::Windows,
            OperatingSystem::Nix | OperatingSystem::Android | OperatingSystem::Unknown => Platform::Linux,
        }
    }
}

// Both matches are generated from one list so that a key added to `Key` without an egui
// counterpart fails to compile.
macro_rules! egui_keys {
    ($($ours:ident = $theirs:ident),* ; $($same:ident),* $(,)?) => {
        impl Key {
            /// The key for an egui key. `None` for modifier keys, Copy, Cut, Paste, BrowserBack
            /// and IntlBackslash.
            pub fn from_egui(key: EguiKey) -> Option<Key> {
                match key {
                    $(EguiKey::$theirs => Some(Key::$ours),)*
                    $(EguiKey::$same => Some(Key::$same),)*
                    _ => None,
                }
            }

            pub fn to_egui(self) -> EguiKey {
                match self {
                    $(Key::$ours => EguiKey::$theirs,)*
                    $(Key::$same => EguiKey::$same,)*
                }
            }
        }
    };
}

egui_keys! {
    Up = ArrowUp, Down = ArrowDown, Left = ArrowLeft, Right = ArrowRight;
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20,
    F21, F22, F23, F24, F25, F26, F27, F28, F29, F30, F31, F32, F33, F34, F35,
    Escape, Tab, Backspace, Enter, Space, Insert, Delete, Home, End, PageUp, PageDown,
    Minus, Equals, Plus, OpenBracket, CloseBracket, Backslash, Semicolon, Quote, Backtick,
    Comma, Period, Slash, Colon, Pipe, Questionmark, Exclamationmark, OpenCurlyBracket, CloseCurlyBracket,
}

impl Mods {
    /// The modifiers egui reports, read for `platform`. On macOS cmd is ⌘ and ctrl the Control
    /// key; elsewhere Ctrl is cmd and ctrl stays false.
    pub fn from_egui(mods: &Modifiers, platform: Platform) -> Mods {
        if platform.is_mac() {
            // egui sets `command` together with `mac_cmd` on macOS; synthetic events may set
            // only one of them.
            Mods { cmd: mods.mac_cmd || mods.command, ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift }
        } else {
            Mods { cmd: mods.ctrl || mods.command, ctrl: false, alt: mods.alt, shift: mods.shift }
        }
    }
}

impl Chord {
    /// The egui shortcut for this chord, for egui's own shortcut text and `consume_shortcut`.
    /// cmd becomes egui's `command` and ctrl its `ctrl`, which egui folds the same way off
    /// macOS.
    pub fn to_egui_shortcut(&self) -> KeyboardShortcut {
        let m = self.mods;
        KeyboardShortcut::new(Modifiers { alt: m.alt, ctrl: m.ctrl, shift: m.shift, mac_cmd: false, command: m.cmd }, self.key.to_egui())
    }
}

/// Base keys and the key egui reports for them with Shift held on a US layout.
const SHIFTED: &[(Key, Key)] = &[
    (Key::Semicolon, Key::Colon),
    (Key::Slash, Key::Questionmark),
    (Key::Backslash, Key::Pipe),
    (Key::OpenBracket, Key::OpenCurlyBracket),
    (Key::CloseBracket, Key::CloseCurlyBracket),
    (Key::Equals, Key::Plus),
    (Key::Num1, Key::Exclamationmark),
];

fn without_shift(m: Mods) -> Mods {
    Mods { shift: false, ..m }
}

/// Whether a key press egui reported triggers `binding` on `platform`.
///
/// Besides an exact match:
/// - Shift with a base key matches the shifted key egui reports (`Shift+;` matches `:`).
/// - A shifted symbol bound without Shift matches when Shift is held to type it (`?`).
/// - A binding on `=` matches `+` with or without Shift, and a binding on `+` matches
///   `Shift+=`, so zoom keys work with and without Shift.
pub fn matches(binding: &Chord, key: EguiKey, mods: &Modifiers, platform: Platform) -> bool {
    let Some(pressed) = Key::from_egui(key) else { return false };
    matches_press(binding, pressed, Mods::from_egui(mods, platform), platform)
}

fn matches_press(binding: &Chord, pressed: Key, held: Mods, platform: Platform) -> bool {
    let want = binding.effective(platform);
    if pressed == want.key && held == want.mods {
        return true;
    }
    let same_but_shift = without_shift(held) == without_shift(want.mods);
    if !same_but_shift {
        return false;
    }
    let shifted_of = |base: Key| SHIFTED.iter().find(|(b, _)| *b == base).map(|&(_, s)| s);
    let is_shifted_symbol = SHIFTED.iter().any(|&(_, s)| s == want.key);
    match (want.key, pressed) {
        (Key::Equals, Key::Plus) => !want.mods.shift || held.shift,
        (Key::Plus, Key::Equals) => held.shift && !want.mods.shift,
        (w, p) if w == p => is_shifted_symbol && !want.mods.shift && held.shift,
        (w, p) => want.mods.shift && held.shift && shifted_of(w) == Some(p),
    }
}

/// The chords a press can stand for, most specific first: the press itself, then those that keep
/// Shift (`Shift+;` for `:`), then those without it (`=` for `+`). Each one [`matches`] the press.
fn candidates(pressed: Key, held: Mods, platform: Platform) -> Vec<Chord> {
    let base = SHIFTED.iter().find(|&&(_, s)| s == pressed).map(|&(b, _)| b);
    let swap = match pressed {
        Key::Plus => Some(Key::Equals),
        Key::Equals => Some(Key::Plus),
        _ => None,
    };
    let mut out = Vec::with_capacity(6);
    for mods in [held, without_shift(held)] {
        for key in [Some(pressed), base, swap].into_iter().flatten() {
            let chord = Chord::new(mods, key);
            if !out.contains(&chord) && matches_press(&chord, pressed, held, platform) {
                out.push(chord);
            }
        }
    }
    out
}

/// Seconds within which a second press of the same chord counts as a double press.
pub const DOUBLE_PRESS: f64 = 0.6;

/// A command a key press runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The command to run; for tools sharing the chord, the first of `cycle`.
    pub command: CommandRef,
    /// Tools of one group sharing the chord, in catalog order (the app picks the next one);
    /// empty for a plain command.
    pub cycle: Vec<CommandRef>,
    /// The chord whose binding matched, with `double` set when the double binding ran.
    pub trigger: Trigger,
    /// Whether `trigger` is the press itself. `false` when the press matched through a stand-in:
    /// the shifted key for a Shift binding (`:` for `Shift+;`), Shift held to type a symbol, or
    /// `+` and `=` standing for each other.
    pub exact: bool,
    /// The text of the Paste event that ran the command, or on the web of the Paste event that
    /// came with the press that ran it.
    pub pasted: Option<String>,
}

impl Hit {
    fn from_lookup(lookup: Lookup, trigger: Trigger, exact: bool) -> Option<Hit> {
        let (command, cycle) = match lookup {
            Lookup::None => return None,
            Lookup::Run(command) => (command, Vec::new()),
            Lookup::Cycle(cycle) => (cycle.first()?.clone(), cycle),
        };
        Some(Hit { command, cycle, trigger, exact, pasted: None })
    }
}

#[derive(Clone, Debug)]
struct LastPress {
    chord: Chord,
    at: f64,
    ran_double: bool,
    /// The focused panel: a press in another panel never completes the pair.
    panel: Option<String>,
}

/// Runs a keymap against egui's input, one call per frame before the UI reads its keys.
///
/// It reads key presses (repeats only when [`with_repeat`](Self::with_repeat) is on). A press
/// that runs a command is removed from the events, together with the text it typed when it has
/// no command modifier, so no widget sees it too. Other events stay.
///
/// On the desktop, egui-winit sends egui's Copy, Cut and Paste events in place of the key press
/// for any chord of Cmd with C, X or V (Ctrl off macOS; on Windows also Shift+Delete, Ctrl+Insert
/// and Shift+Insert). The web build sends the key press and then the event. For the plain
/// clipboard chords (Cmd+C, Cmd+X, Cmd+V, Cmd+Insert, Shift+Delete, Shift+Insert), the events and
/// their presses stay with the app unless [`with_clipboard_events`](Self::with_clipboard_events)
/// is on, so bindings on those chords run only with it on. An event with more modifiers held
/// counts as its chord, read from the frame's modifiers (Cmd+Shift+V), and runs that chord's
/// binding; without one, it stays. On Windows, egui-winit also sends Cut for Ctrl+Shift+Delete
/// and Copy for Ctrl+Shift+Insert, which read as Cmd+Shift+X and Cmd+Shift+C.
///
/// egui-winit sends no Paste event when the clipboard is empty or holds no text (an image), so a
/// binding on Cmd+V, or on Cmd and V with more modifiers, cannot run then on the desktop.
///
/// While [`Active::typing`], the text field keeps, without lookup:
/// - the plain clipboard chords' events and presses (other clipboard chords are looked up);
/// - a press with a Text event of its own in the frame, since it typed that text. This covers
///   AltGr: the browser on Windows reports AltGr+Q (`@` on a German layout) as Ctrl+Alt+Q, and
///   the web build's text agent sends the `@` after it. egui-winit reports AltGr with neither Ctrl
///   nor Alt. A Ctrl+Alt press without text is a command chord.
///
/// Other presses are looked up; the keymap's [`TypingRule`](crate::TypingRule) and
/// [`text_editing_chords`](crate::text_editing_chords) leave plain keys and the text-editing chords
/// (Cmd+Z, word moves) to the field.
#[derive(Clone, Debug)]
pub struct Dispatcher {
    last: Option<LastPress>,
    repeat: bool,
    clipboard: bool,
    /// Whether a press's Text event comes before it when no text field has focus, as eframe's
    /// web build sends them.
    text_first: bool,
}

impl Default for Dispatcher {
    fn default() -> Dispatcher {
        Dispatcher { last: None, repeat: false, clipboard: false, text_first: cfg!(target_arch = "wasm32") }
    }
}

impl Dispatcher {
    pub fn new() -> Dispatcher {
        Dispatcher::default()
    }

    /// Whether held keys run their command again on each repeat (off by default), so a held
    /// arrow can nudge. Repeats never count toward a double press.
    pub fn with_repeat(mut self, on: bool) -> Dispatcher {
        self.repeat = on;
        self
    }

    /// Whether egui's Copy, Cut and Paste events for the plain clipboard chords count as those
    /// chords (Cmd+C, Cmd+X, Cmd+V; Shift+Delete and Shift+Insert for Cut and Paste with Shift
    /// alone) outside text fields. Off by default: the events stay in egui's input for the app's
    /// own handlers. With it on, an event that runs a command is removed, and a Paste event's text
    /// moves to [`Hit::pasted`]. On the web, the key press runs the command and its clipboard
    /// event goes with it the same way; when the press runs nothing, or the hit is declined, both
    /// stay. An app that asks egui for a paste (`ViewportCommand::RequestPaste`) to run its paste
    /// command gets a Paste event back, which with this on runs that command again.
    pub fn with_clipboard_events(mut self, on: bool) -> Dispatcher {
        self.clipboard = on;
        self
    }

    /// The commands this frame's key presses run with `active` focus, in press order.
    ///
    /// A chord pressed again within [`DOUBLE_PRESS`] in the same panel runs its double binding
    /// when it has one; the first press has already run the single binding. The press after a
    /// double starts over.
    pub fn dispatch(&mut self, input: &mut InputState, keymap: &Keymap, active: &Active) -> Vec<Hit> {
        self.dispatch_filtered(input, keymap, active, |_| true)
    }

    /// [`dispatch`](Self::dispatch), with `accept` deciding each hit before its event is removed.
    /// A declined hit is left out of the result and its event, with any text it typed, stays in
    /// egui's input in its place. A declined press still counts toward a double press.
    pub fn dispatch_filtered(&mut self, input: &mut InputState, keymap: &Keymap, active: &Active, mut accept: impl FnMut(&Hit) -> bool) -> Vec<Hit> {
        let platform = keymap.platform();
        let now = input.time;
        let frame_mods = Mods::from_egui(&input.modifiers, platform);
        let mut hits = Vec::new();
        let mut kept = Vec::with_capacity(input.events.len());
        let events = std::mem::take(&mut input.events);
        let text_of = text_of_press(&events, self.text_first && !active.typing);
        let mut drop = vec![false; events.len()];
        // The web build sends both a clipboard key press and the clipboard event it causes. The
        // press decides: its event goes with it when it runs a command and stays otherwise.
        let causes = clipboard_event_follows(&events, platform);
        let mut caused_taken: Vec<Option<bool>> = vec![None; events.len()];
        for (i, event) in events.into_iter().enumerate() {
            match &event {
                Event::Key { key, pressed: true, repeat, modifiers, .. } if !*repeat || self.repeat => {
                    let held = Mods::from_egui(modifiers, platform);
                    if let Some(key) = Key::from_egui(*key) {
                        let plain_clip = is_plain_clipboard(key, held);
                        let keep = if active.typing {
                            text_of[i].is_some() || plain_clip
                        } else {
                            !self.clipboard && plain_clip && causes[i].is_some()
                        };
                        if keep {
                            if let Some((j, _)) = causes[i] {
                                caused_taken[j] = Some(false);
                            }
                            kept.push((i, event));
                            continue;
                        }
                        let caused = causes[i].clone();
                        let hit = self.press(key, held, *repeat, now, keymap, active).map(|mut hit| {
                            hit.pasted = caused.as_ref().and_then(|(_, text)| text.clone());
                            hit
                        });
                        let hit = hit.filter(&mut accept);
                        if let Some((j, _)) = caused {
                            caused_taken[j] = Some(hit.is_some());
                        }
                        if let Some(hit) = hit {
                            hits.push(hit);
                            if let Some(j) = text_of[i].filter(|_| !(held.cmd || held.ctrl)) {
                                drop[j] = true;
                            }
                            continue;
                        }
                    }
                }
                // A repeat that does not run leaves its clipboard event in place with it.
                Event::Key { pressed: true, repeat: true, .. } => {
                    if let Some((j, _)) = causes[i] {
                        caused_taken[j] = Some(false);
                    }
                }
                // Desktop clipboard events, and web ones whose press stayed or ran nothing.
                Event::Copy | Event::Cut | Event::Paste(_) => match caused_taken[i] {
                    Some(true) => continue,
                    Some(false) => {}
                    None => {
                        let (key, held) = clipboard_chord(&event, frame_mods);
                        // A focused text field copies and pastes on the plain chords itself.
                        let look = !is_plain_clipboard(key, held) || (self.clipboard && !active.typing);
                        let hit = if look { self.press(key, held, false, now, keymap, active) } else { None };
                        if let Some(mut hit) = hit {
                            if let Event::Paste(text) = &event {
                                hit.pasted = Some(text.clone());
                            }
                            if accept(&hit) {
                                hits.push(hit);
                                continue;
                            }
                        }
                    }
                },
                _ => {}
            }
            kept.push((i, event));
        }
        input.events = kept.into_iter().filter(|(i, _)| !drop[*i]).map(|(_, e)| e).collect();
        hits
    }

    fn press(&mut self, key: Key, held: Mods, repeat: bool, now: f64, keymap: &Keymap, active: &Active) -> Option<Hit> {
        let chords = candidates(key, held, keymap.platform());
        let first = *chords.first()?;
        let pressed = Chord::new(held, key);
        let find = |double: bool| {
            chords.iter().find_map(|&chord| Hit::from_lookup(keymap.lookup(&chord, double, active), Trigger { chord, double }, chord == pressed))
        };
        if repeat {
            return find(false);
        }
        let again = self.last.as_ref().is_some_and(|l| {
            l.chord == first && !l.ran_double && l.panel.as_deref() == active.panel && now >= l.at && now - l.at < DOUBLE_PRESS
        });
        let double = if again { find(true) } else { None };
        self.last = Some(LastPress { chord: first, at: now, ran_double: double.is_some(), panel: active.panel.map(str::to_owned) });
        double.or_else(|| find(false))
    }
}

/// A clipboard event: Copy, Cut or Paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clip {
    Copy,
    Cut,
    Paste,
}

/// For each clipboard key press whose clipboard event comes later in the frame, as the web build
/// sends them: the index of that event, and the text of a Paste event.
fn clipboard_event_follows(events: &[Event], platform: Platform) -> Vec<Option<(usize, Option<String>)>> {
    let mut out = vec![None; events.len()];
    let mut ahead: [Option<usize>; 3] = [None; 3];
    for (i, event) in events.iter().enumerate().rev() {
        if let Some(seen) = event_clip(event).and_then(|c| ahead.get_mut(c as usize)) {
            *seen = Some(i);
        } else if let Event::Key { key, pressed: true, modifiers, .. } = event {
            let clip = Key::from_egui(*key).and_then(|k| clipboard_kind(k, Mods::from_egui(modifiers, platform)));
            if let Some(j) = clip.and_then(|c| ahead.get_mut(c as usize)).and_then(Option::take) {
                let text = if let Some(Event::Paste(text)) = events.get(j) { Some(text.clone()) } else { None };
                out[i] = Some((j, text));
            }
        }
    }
    out
}

/// For each key press, the index of the Text event it typed. egui-winit, and eframe's web text
/// agent while a text field has focus, send the Text after the press, before the next one.
/// With `text_first`, eframe's web build with no text field focused, the Text comes right before
/// its press, and only for a press without Ctrl or Cmd.
fn text_of_press(events: &[Event], text_first: bool) -> Vec<Option<usize>> {
    let is_text = |e: Option<&Event>| matches!(e, Some(Event::Text(t)) if !t.is_empty());
    let mut out = vec![None; events.len()];
    if text_first {
        for (i, event) in events.iter().enumerate().skip(1) {
            if let Event::Key { pressed: true, modifiers, .. } = event
                && !(modifiers.ctrl || modifiers.command || modifiers.mac_cmd)
                && is_text(events.get(i - 1))
            {
                out[i] = Some(i - 1);
            }
        }
        return out;
    }
    let mut text_ahead = None;
    for (i, event) in events.iter().enumerate().rev() {
        match event {
            Event::Key { pressed: true, .. } => out[i] = text_ahead.take(),
            _ if is_text(Some(event)) => text_ahead = Some(i),
            _ => {}
        }
    }
    out
}

fn event_clip(event: &Event) -> Option<Clip> {
    match event {
        Event::Copy => Some(Clip::Copy),
        Event::Cut => Some(Clip::Cut),
        Event::Paste(_) => Some(Clip::Paste),
        _ => None,
    }
}

/// The clipboard event a backend sends for a press, if any.
fn clipboard_kind(key: Key, held: Mods) -> Option<Clip> {
    match key {
        Key::C if held.cmd => Some(Clip::Copy),
        Key::X if held.cmd => Some(Clip::Cut),
        Key::V if held.cmd => Some(Clip::Paste),
        Key::Delete if held.shift => Some(Clip::Cut),
        Key::Insert if held.shift => Some(Clip::Paste),
        Key::Insert if held.cmd || held.ctrl => Some(Clip::Copy),
        _ => None,
    }
}

/// Whether a chord is one of the plain clipboard chords, which egui's text fields act on: Cmd+C,
/// Cmd+X, Cmd+V, Cmd+Insert, Shift+Delete and Shift+Insert.
fn is_plain_clipboard(key: Key, held: Mods) -> bool {
    match key {
        Key::C | Key::X | Key::V => held == Mods::CMD,
        Key::Insert => held == Mods::CMD || held == Mods::SHIFT,
        Key::Delete => held == Mods::SHIFT,
        _ => false,
    }
}

/// The press a clipboard event stands for, with the modifiers held. Off macOS egui-winit also
/// sends Cut for Shift+Delete and Paste for Shift+Insert.
fn clipboard_chord(event: &Event, held: Mods) -> (Key, Mods) {
    let with_cmd = Mods { cmd: true, ..held };
    let shift_only = held.shift && !held.cmd;
    match event {
        Event::Cut if shift_only => (Key::Delete, held),
        Event::Cut => (Key::X, with_cmd),
        Event::Paste(_) if shift_only => (Key::Insert, held),
        Event::Paste(_) => (Key::V, with_cmd),
        _ => (Key::C, with_cmd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bindable, Binding, Scope, Scopes};

    #[test]
    fn web_text_before_its_press_goes_with_that_press() {
        let km = Keymap::new(
            vec![Bindable::new("tool.marquee", "Marquee", Scope::app()).with_default(Binding::parse("M").unwrap())],
            Scopes::new(),
            Platform::Windows,
        );
        let press = |key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut input = InputState::default();
        input.events = vec![Event::Text("m".into()), press(EguiKey::M), Event::Text("q".into()), press(EguiKey::Q)];
        let mut d = Dispatcher { text_first: true, ..Dispatcher::new() };
        let hits = d.dispatch(&mut input, &km, &Active::NONE);
        assert_eq!(hits.len(), 1);
        assert_eq!(input.events, [Event::Text("q".into()), press(EguiKey::Q)]);
    }
}
