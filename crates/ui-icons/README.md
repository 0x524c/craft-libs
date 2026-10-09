# Craft UI icons

`craft-ui-icons` prepares embedded SVG icon catalogs for egui frontends. It extracts the common
PhotoCraft/VectorCraft icon-loading path while keeping asset choice, visual style and tool behavior
in each app. No icons, fonts or application engines are bundled.

```rust
use std::sync::OnceLock;
use craft_ui_icons::{SvgIconSet, SvgStyle};

// Supply the application's own attributed assets here.
static ICONS: &[(&str, &[u8])] = &[];

fn icons() -> &'static SvgIconSet {
    static ICONS_SET: OnceLock<SvgIconSet> = OnceLock::new();
    ICONS_SET.get_or_init(|| SvgIconSet::new(
        ICONS,
        SvgStyle { current_color: "#ffffff", stroke_width: Some("1.75") },
        "bytes://my-app/icons/",
        "square",
    ))
}

fn icon(name: &str) -> egui::Image<'static> {
    egui::Image::new(icons().source(name))
}
```

The application installs `egui_extras::install_image_loaders` with its `svg` feature before drawing.
It sets tint and size on the returned image and retains its existing interaction/tooltip code.
The library does not install loaders or an egui theme.

The catalog is immutable. SVGs are prepared once and held in `Arc<[u8]>`; lookups share those bytes.
The fallback uses the requested name in its URI, matching PhotoCraft and VectorCraft. Missing both
the requested name and fallback produces empty bytes for the loader to report as an error, without
indexed lookups or a panic. `contains` and `canonical_name` inspect only actual catalog entries.

Use a different URI prefix for every catalog/style combination used in the same egui context.
egui caches by URI; supplying different bytes under one URI would reuse stale content. Apps with
their own contexts can retain their current prefixes. Unknown names do not add entries to this
catalog, though the egui loader may cache a requested URI.

This is an adapter for trusted, compiled-in monochrome assets, not an SVG sanitizer. It performs
literal `currentColor` and optional `stroke-width="2"` substitutions, keeps other widths and explicit
colors untouched, and uses the existing lossy UTF-8 behavior. General SVG parsing and rasterization
remain the responsibility of the existing loader.

The crate uses egui 0.36 and Rust 1.95 or later. Its production dependency disables egui's default
features so fonts and rendering features remain the application's choice. See the
[suite audit](../../docs/ui-components-audit.md) for candidates, policies and adoption order.

Validation:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p craft-ui-icons
```
