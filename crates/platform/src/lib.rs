#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! Shared lifecycle mechanics; application state, theme choices and file policy stay in hosts.
pub mod dialog_outcome;
#[cfg(all(feature = "dialogs", not(target_arch = "wasm32")))]
pub mod dialogs;
