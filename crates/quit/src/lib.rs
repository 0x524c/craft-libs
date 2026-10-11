#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Quit a Craft app the platform's own way.
//!
//! On macOS an eframe app must not leave by closing its window. eframe drops the window while
//! AppKit's run loop is still running, and in the next display cycle AppKit's Touch Bar finder
//! tries to remove a KVO observer from the view that is gone
//! (`-[_NSTouchBarFinderObservation invalidate]`). That is an uncaught Objective-C exception and
//! the app crashes (seen on macOS 27 on a Touch Bar Mac).
//! `-[NSApplication terminate:]`, what ⌘Q does in winit's default menu, ends the process before
//! another display cycle runs.
//!
//! [`quit`] is that quit. With the `egui` feature, [`on_close_request`] routes a window close
//! through it. Call it last in the frame, after all of the app's own close handling: eframe calls
//! `logic` before `ui`, so an app that handles closes in `ui` calls it at the end of `ui`, not in
//! `logic`, or it quits before the app's unsaved-changes check has run:
//!
//! ```ignore
//! fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
//!     // ... the app's UI ...
//!     let ctx = ui.ctx();
//!     if ctx.input(|i| i.viewport().close_requested()) && self.has_unsaved_changes() {
//!         ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
//!         self.ask_to_save(); // sends `ViewportCommand::Close` once answered
//!     }
//!     craft_quit::on_close_request(ctx); // last, so it sees the app's `CancelClose`
//! }
//! ```
//!
//! The app still saves what it keeps in `eframe::App::on_exit`: winit's
//! `applicationWillTerminate:` runs eframe's exit handling before the process ends. The process
//! ends there, so code after `eframe::run_native` in `main` does not run; move it to `on_exit`.

#[cfg(feature = "egui")]
mod close;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(feature = "egui")]
pub use close::on_close_request;

/// Quit through the platform's own path, where leaving by closing the window is not safe.
///
/// macOS: schedules `-[NSApplication terminate:]` and returns `true`; the caller keeps its window
/// open and the process ends shortly after, once the current event has been handled. Must be
/// called while the app's event loop is running (any eframe callback is). Return to the event loop
/// before blocking: a blocking native dialog opened in the same callback would run the quit while
/// winit is still handling the event, which panics.
///
/// Other platforms: does nothing and returns `false`; the caller closes its window as usual.
pub fn quit() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::terminate_later();
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
