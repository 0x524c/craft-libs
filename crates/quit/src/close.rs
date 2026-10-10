//! Route an egui window close through the platform quit.

use egui::{Context, ViewportCommand, ViewportId};

/// Call once per frame, after the app has dealt with unsaved work: when the root window was asked
/// to close (close button, the app's own `ViewportCommand::Close`, the OS) and this platform has
/// its own quit (see [`crate::quit`]), cancels the close and quits that way instead. The window
/// stays for the quit to close.
///
/// Returns whether it took the close over. `false` means there was no close request, the app had
/// already cancelled it this frame, or the platform has no quit of its own, and eframe goes on as
/// it would without this call.
pub fn on_close_request(ctx: &Context) -> bool {
    take_over_close(ctx, crate::quit)
}

/// [`on_close_request`] with the platform quit passed in. `quit` returns whether it quits.
fn take_over_close(ctx: &Context, quit: impl FnOnce() -> bool) -> bool {
    if ctx.viewport_id() != ViewportId::ROOT || !ctx.input(|i| i.viewport().close_requested()) {
        return false;
    }
    // The app's unsaved-changes prompt (or anything else) is holding the window open.
    if ctx.viewport(|v| v.commands.contains(&ViewportCommand::CancelClose)) {
        return false;
    }
    if !quit() {
        return false;
    }
    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
    true
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use egui::{RawInput, ViewportEvent};

    use super::*;

    /// One frame of the root window, with its close button pressed or not. `app` runs before the
    /// take-over, like the app's own unsaved-changes check. Returns whether the close was taken
    /// over, how often the quit ran and the frame's commands for the root window.
    fn frame(close: bool, quits: bool, app: impl Fn(&Context)) -> (bool, u32, Vec<ViewportCommand>) {
        let ctx = Context::default();
        let mut input = RawInput::default();
        if close {
            input.viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
        }
        let (taken, quit_calls) = (Cell::new(false), Cell::new(0));
        let mut out = ctx.run_ui(input, |ui| {
            app(ui.ctx());
            taken.set(take_over_close(ui.ctx(), || {
                quit_calls.set(quit_calls.get() + 1);
                quits
            }));
        });
        out.textures_delta.clear(); // not painted
        let commands = out.viewport_output.remove(&ViewportId::ROOT).map(|o| o.commands).unwrap_or_default();
        (taken.get(), quit_calls.get(), commands)
    }

    fn cancels(commands: &[ViewportCommand]) -> bool {
        commands.contains(&ViewportCommand::CancelClose)
    }

    #[test]
    fn a_close_request_quits_and_keeps_the_window() {
        let (taken, quit_calls, commands) = frame(true, true, |_| {});
        assert!(taken);
        assert_eq!(quit_calls, 1);
        assert!(cancels(&commands), "the window stays for the quit to close");
    }

    #[test]
    fn nothing_happens_without_a_close_request() {
        let (taken, quit_calls, commands) = frame(false, true, |_| {});
        assert!(!taken);
        assert_eq!(quit_calls, 0);
        assert!(!cancels(&commands));
    }

    #[test]
    fn a_close_the_app_cancelled_does_not_quit() {
        let (taken, quit_calls, _) = frame(true, true, |ctx| ctx.send_viewport_cmd(ViewportCommand::CancelClose));
        assert!(!taken);
        assert_eq!(quit_calls, 0, "unsaved work asks first");
    }

    #[test]
    fn without_a_platform_quit_the_window_closes_as_usual() {
        let (taken, quit_calls, commands) = frame(true, false, |_| {});
        assert!(!taken);
        assert_eq!(quit_calls, 1);
        assert!(!cancels(&commands), "eframe closes the window");
    }
}
