#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Embed the optional [craft-fonts](https://github.com/storytold/craft-fonts) faces from an app's
//! `build.rs`.
//!
//! Apps never depend on craft-fonts in `Cargo.toml` (Cargo would fetch the font repository for
//! every build). The build reads a local checkout named by `CRAFT_FONTS_DIR` instead:
//!
//! - **Unset or empty:** the table is empty and the app uses its own and system fonts.
//! - **Set:** every selected face in `fonts/manifest.txt` is embedded with `include_bytes!`.
//! - **Set, but not readable as a checkout:** a `cargo::warning` and an empty table.
//! - **`CRAFT_FONTS_REQUIRED=1`** (release builds; any other value is ignored): a missing or
//!   unreadable checkout, no selected face, or a requested script without a face is an [`Error`].
//!
//! Add this crate both as a build-dependency (for [`embed`]) and as a dependency (for
//! [`EmbeddedFace`]). In `build.rs`'s `main`:
//!
//! ```no_run
//! println!("cargo::rerun-if-changed=build.rs");
//! let wasm = craft_fonts_build::target_is_wasm();
//! let embedded = craft_fonts_build::Embed::new()
//!     // CJK faces as their UI subsets, cut to the characters the translation catalogs use.
//!     .for_ui()
//!     // Web builds have a size budget: embed only the Japanese UI face there.
//!     .filter(move |face| !wasm || face.has_script("Jpan"))
//!     .write("craft_fonts.rs");
//! if let Err(e) = embedded {
//!     println!("cargo::error={e}");
//! }
//! ```
//!
//! and in the crate:
//!
//! ```ignore
//! use craft_fonts_build::EmbeddedFace;
//!
//! pub static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));
//! ```
//!
//! The generated file is a slice expression of `EmbeddedFace { .. }` literals, so any struct with
//! the same fields and that name in scope works too.

mod cmap;
mod embed;
mod manifest;
#[cfg(test)]
mod tests;

pub use embed::{Embed, Error, embed, target_is_wasm};
pub use manifest::{CJK_SCRIPTS, ManifestEntry, ParseError, UI_ROLE, parse_manifest};

/// A craft-fonts face embedded by [`embed`]: one line of `fonts/manifest.txt`.
#[derive(Clone, Copy)]
pub struct EmbeddedFace {
    /// Family name, e.g. `"Noto Sans Arabic"`.
    pub family: &'static str,
    /// Style, e.g. `"Regular"` or `"Bold"`.
    pub style: &'static str,
    /// Script tags: ISO 15924 codes such as `Arab`, `Jpan`, `Hans`, `Hant`, optionally with a
    /// region subtag (`Hant-HK` for Hong Kong forms, which have no ISO 15924 code).
    pub scripts: &'static [&'static str],
    /// Face index in a font collection (`.ttc`); `0` for a single font.
    pub index: u32,
    pub bytes: &'static [u8],
}

impl EmbeddedFace {
    /// `"<family> <style>"`, e.g. `"Noto Sans Arabic Regular"`.
    pub fn name(&self) -> String {
        format!("{} {}", self.family, self.style)
    }

    /// Whether any script tag matches `tag`: `Hant` matches `Hant` and `Hant-HK`.
    pub fn has_script(&self, tag: &str) -> bool {
        self.scripts.iter().any(|s| manifest::script_matches(s, tag))
    }

    /// CSS/OpenType weight implied by the style name (`Bold` → 700); 400 when it names none.
    pub fn weight(&self) -> u16 {
        let style = self.style.to_ascii_lowercase().replace([' ', '-'], "");
        const WEIGHTS: [(&str, u16); 14] = [
            ("extralight", 200),
            ("ultralight", 200),
            ("semibold", 600),
            ("demibold", 600),
            ("extrabold", 800),
            ("ultrabold", 800),
            ("thin", 100),
            ("light", 300),
            ("medium", 500),
            ("bold", 700),
            ("black", 900),
            ("heavy", 900),
            ("regular", 400),
            ("book", 400),
        ];
        WEIGHTS.iter().find(|(name, _)| style.contains(name)).map_or(400, |(_, weight)| *weight)
    }
}

impl std::fmt::Debug for EmbeddedFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedFace")
            .field("family", &self.family)
            .field("style", &self.style)
            .field("scripts", &self.scripts)
            .field("index", &self.index)
            .field("bytes", &format_args!("[{} bytes]", self.bytes.len()))
            .finish()
    }
}
