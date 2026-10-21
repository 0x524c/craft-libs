# craft-fonts-build

Embeds the optional [craft-fonts](https://github.com/storytold/craft-fonts) faces from an app's
`build.rs`, and defines `EmbeddedFace`, the plain data type of the generated table. No
dependencies and no egui, so crates below the UI layer (text engines, exporters) can use it.

Apps never list craft-fonts in `Cargo.toml`: Cargo would fetch the font repository for every
build. The build reads a local checkout named by `CRAFT_FONTS_DIR` instead.

| Environment | Result |
|---|---|
| `CRAFT_FONTS_DIR` unset or empty | Empty table. |
| `CRAFT_FONTS_DIR` = a craft-fonts checkout | Every selected face of `fonts/manifest.txt`, embedded with `include_bytes!`. |
| `CRAFT_FONTS_DIR` set, but no manifest or a missing font file | `cargo::warning`, empty table. |
| `CRAFT_FONTS_REQUIRED=1` (exactly `1`; other values are ignored) | Error instead when the directory is unset, unreadable, has no selected face, or has no face for a script passed to `scripts`. |

Give `CRAFT_FONTS_DIR` as an absolute path: Cargo runs build scripts in the package's directory.

## Using it

`Cargo.toml` (same `rev` as the app's other craft-libs crates):

```toml
[dependencies]
craft-fonts-build = { git = "https://github.com/storytold/craft-libs", rev = "…" }

[build-dependencies]
craft-fonts-build = { git = "https://github.com/storytold/craft-libs", rev = "…" }
```

`build.rs` (FilmCraft's `crates/text/build.rs`, in full):

```rust
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let wasm = craft_fonts_build::target_is_wasm();
    let embedded = craft_fonts_build::Embed::new()
        .for_ui()
        // Web builds have a size budget: embed only the Japanese UI face there.
        .filter(move |face| !wasm || face.has_script("Jpan"))
        .write("craft_fonts.rs");
    if let Err(e) = embedded {
        println!("cargo::error={e}");
    }
}
```

The crate:

```rust
use craft_fonts_build::EmbeddedFace;

/// craft-fonts faces; empty unless built with `CRAFT_FONTS_DIR`.
pub static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));
```

`craft_fonts_build::embed("craft_fonts.rs")` embeds every face. `Embed` narrows the set:

- `scripts(&["Jpan", "Arab", "Hant"])` keeps faces with any of these script tags. A tag matches
  itself and its region forms (`Hant` matches `Hant-HK`). With `CRAFT_FONTS_REQUIRED=1`, each tag
  must match at least one face.
- `filter(|entry| …)` keeps faces by any manifest field (family, style, licence, index).
- `for_ui()` keeps CJK faces only as their UI subsets (see below).
- `check_text(paths)` warns about characters in these files that no selected face covers.
- `dir(path)` and `required(bool)` override the two environment variables.

`write` returns the embedded `ManifestEntry`s, for build-time checks such as "the UI face is
present". The generated file is a slice expression of `EmbeddedFace { family, style, scripts,
index, bytes }` literals; any struct with those fields and that name in scope works too.

## UI subsets

The full CJK faces take several megabytes each. For the interface, craft-fonts also ships
Regular-only UI subsets cut to the characters the translation catalogs use: `Noto Sans SC UI`
(`Hans`), `Noto Sans TC UI` (`Hant`), `Noto Sans HK UI` (`Hant-HK`), `Noto Sans KR UI` (`Kore`) and
`BIZ UDPGothic UI` (`Jpan`), about 4 MB together. Their manifest lines have the role `ui`.

`for_ui()` drops every face with a CJK script tag (`Hans`, `Hant`, `Hant-HK`, `Jpan`, `Kore`,
`Hani`) that is not a UI subset, and keeps the rest. Apps embed their interface faces this way;
document text (FigCraft, DeckCraft) uses the full faces. With `CRAFT_FONTS_REQUIRED=1`, a UI subset
satisfies `scripts(&["Hant"])`.

A character the subsets lack still shows: craft-ui's system font provider draws it with an
installed font. `check_text` makes the build say so. It reads the character maps of the selected
faces and prints one `cargo::warning` per file that has characters from U+2E80 up that none of
them covers, with the count and the first few. It also reruns the build script when a file
changes. The subsets are regenerated in craft-fonts with `tools/ui-chars` and `tools/ui-subset`.

```rust
// The catalogs `CRAFT_I18N_DIR` holds for this app, when it is set.
let locales = std::env::var_os("CRAFT_I18N_DIR").map(|dir| std::path::Path::new(&dir).join("locales"));
let catalogs: Vec<_> = locales
    .iter()
    .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
    .flat_map(|locale| ["common.tsv", "filmcraft.tsv"].map(|file| locale.path().join(file)))
    .filter(|path| path.is_file())
    .collect();
let embedded = craft_fonts_build::Embed::new().for_ui().check_text(catalogs).write("craft_fonts.rs");
```

## The manifest

One face per line, eight or nine fields separated by `" | "`; craft-fonts' `docs/integration.md`
defines them. Lines starting with `#` are comments. A file field ending in `#<n>` names face `n` of
a font collection (`fonts/noto-sans-cjk/NotoSansCJK.ttc#2`); without it the index is 0. The ninth
field, `role`, is `ui` on UI subsets and absent elsewhere. Fields after the ninth are ignored.
