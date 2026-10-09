# Shared UI component audit

The best first extraction is the embedded SVG icon catalog used by PhotoCraft, VectorCraft and PdfCraft. It has a small boundary, real consumers and little dependence on application behavior. The pilot puts that machinery in `craft-ui::icons` and adopts it in all three apps. A second component, `craft-ui::buttons::IconButton`, is adopted by PhotoCraft and PdfCraft. The pane extension adds tab fitting and interaction in PhotoCraft/VectorCraft, split geometry in DesignCraft/PhotoCraft, and split geometry plus resize handles in FilmCraft/EffectCraft.

The broader opportunity is a collection of reusable UI components. All twelve Rust Craft apps use egui 0.36, but their themes, document models, numeric fields and docking behavior have evolved differently. Shared components should accept small style values and return interactions; each app should continue to choose its layout, command, localization and document semantics.

This covers item 3 of [craft-libs issue 2](https://github.com/storytold/craft-libs/issues/2). The [earlier four-app comparison](https://github.com/storytold/vectorcraft/issues/647) is useful background, but the findings below come from the pinned source revisions listed at the end. This is a source audit, not a claim that every application has passed interactive or cross-platform acceptance.

## Scope and current libraries

The inventory covers the twelve Rust editor apps plus ArtCraft and ArtCraftX. ArtCraft's service repository, fonts, test corpora, archived predecessors and unrelated organization repositories are outside the UI scope.

| App | Current UI and useful source | Behavior a shared component must preserve |
| --- | --- | --- |
| PhotoCraft | egui/eframe 0.36; [tokens][pc-theme], [widgets][pc-widgets], [SVG icons][pc-icons], [dock][pc-dock], [rulers][pc-rulers] | Five themes, raster units and resolution, foreground/background colors, grouped panels, tool cursors and command coalescing. |
| VectorCraft | egui/eframe 0.36; [tokens][vc-theme], [widgets][vc-widgets], [SVG icons][vc-icons], [dock][vc-dock], [canvas rulers][vc-canvas] | Four brightness levels, point-based units, icon rails and flyouts, named/spot/gradient swatches, tool-name aliases. |
| DesignCraft | egui/eframe 0.36; [tokens][dc-theme], [widgets][dc-widgets], [painter icons][dc-icons], [dock][dc-dock], [canvas][dc-canvas] | Four brightness levels plus high contrast, page/spread coordinates, mixed measurements, numeric spinners, floating panels and RTL text. |
| PdfCraft | egui/eframe 0.36; [tokens][pdf-theme], [widgets][pdf-widgets], [SVG icons][pdf-icons], [dialogs][pdf-dialogs] | System/light/dark preferences, tool catalog, accessible button labels, PDF page coordinates and progress cancellation. |
| LightCraft | egui/eframe 0.36; [tokens][lc-theme], [widgets][lc-widgets], [menus][lc-menu] | Develop control specifications, reset/fine-adjust gestures, catalog selection and label colors, preview-only explanations. |
| FilmCraft | egui/eframe 0.36; [tokens][fc-theme], [widgets][fc-widgets], [dock][fc-dock] | Three themes, configurable highlights/contrast, split/tab workspaces, compact tool/meter panels, timecode and media focus. |
| EffectCraft | egui/eframe 0.36; [tokens][ec-theme], [widgets][ec-widgets], [dock][ec-dock] | Three themes plus brightness adjustment, stacked panels and floating layouts, panel-local Tab navigation, composition/viewer identity. |
| WordCraft | egui/eframe 0.36; [tokens][wc-theme], [ribbon widgets][wc-widgets], [canvas rulers][wc-canvas] | Light/dark appearance, contextual command availability and shortcuts, document theme colors, paragraph indents and tabs. |
| GridCraft | egui/eframe 0.36; [tokens][gc-theme], [ribbon widgets][gc-widgets], [dialogs][gc-dialogs] | Light/dark appearance, spreadsheet cell styles and theme tints, formula/selection focus, command-backed dialogs. |
| DeckCraft | egui/eframe 0.36; [tokens][deck-theme], [ribbon widgets][deck-widgets], [canvas rulers][deck-canvas] | Light/dark appearance, split buttons and galleries, slide theme references, contextual ribbon tabs and shape editing. |
| CADCraft | egui/eframe 0.36; [tokens][cad-theme], [chrome][cad-chrome], [palettes][cad-palettes], [dialogs][cad-dialogs] | Drafting toggles, command-line state, model/layout tabs, CAD property types and multicolor painter icons. |
| SoundCraft | egui/eframe 0.36; [tokens][sc-theme], [widgets][sc-widgets], [toolbar][sc-toolbar], [edit window][sc-edit] | Edit/mix controls, nonlinear dB meters, pan/fader interaction, record state and selectable audio rulers. |
| ArtCraft | React 18.3.1/Tauri; [frontend dependencies][ac-package], [component packages][ac-components], [Tailwind theme][ac-theme] | DOM focus and portals, draggable/resizable dialogs, media and generation state, hotkey ownership and service boundaries. |
| ArtCraftX | React 18.3.1/Tauri; [frontend dependencies][ax-package], [component packages][ax-components], [Tailwind theme][ax-theme] | A related component family with different visual tokens and icon wrappers; Rust egui components are not a direct adoption path. |

The Rust applications keep egui at the frontend layer. The pilot follows that rule: only their UI crates depend on `craft-ui`; engine, document and rendering crates do not. Their layer checkers reject a direct dependency on the shared crate below the UI layer: L6 in PhotoCraft/VectorCraft/DesignCraft, L5 in FilmCraft/EffectCraft and L7 in PdfCraft. The crate depends on egui without enabling its default features. Apps retain their existing `egui_extras` SVG loader and renderer setup.

ArtCraft and ArtCraftX already use Headless UI, Radix Dialog/Tooltip, Lucide React and resizable panels. Their component reuse should be evaluated within that stack. A shared visual vocabulary can cross the two toolkit families; Rust widget types cannot.

## Extraction priorities

These are proposed boundaries and adoption pairs, not a commitment to build a framework or migrate every app at once.

| Order | Component | Type of sharing | Initial consumers | Reference and reason |
| --- | --- | --- | --- | --- |
| 1 | Embedded SVG catalog | Extract with two small style policies | PhotoCraft, VectorCraft, PdfCraft | PhotoCraft's owned `Arc` bytes avoid leaked allocations; the existing loaders already have almost the same responsibilities. Implemented in the pilot. |
| 2 | Small controls and component styles | Share components | PdfCraft and PhotoCraft for buttons; GridCraft and DeckCraft for ribbon pieces | PdfCraft exposes accessibility metadata on its painted controls. Keep the caller's label, geometry, colors, enabled state and command handling. Icon buttons implemented; ribbon pieces remain proposed. |
| 3 | Numeric editing state | Consolidate selected behavior | FilmCraft and EffectCraft, then LightCraft | EffectCraft adds finite-value checks and panel-local Tab behavior; LightCraft has explicit cancel/reset and step semantics. These are useful references, not interchangeable implementations. |
| 4 | Swatch grid and color-field painting | Share components | WordCraft/GridCraft/DeckCraft for grids; PhotoCraft/VectorCraft for fields | The visual primitives overlap, while the values returned and color-management requirements differ. |
| 5 | Menu viewport and navigation helpers | Consolidate selected behavior | PhotoCraft, LightCraft, VectorCraft | Scrolling, overflow and focus are reusable. Menu catalogs, native menus, shortcut resolution and dispatch remain app-owned. |
| 6 | Ruler tick layout and painting | Standardize a primitive API | VectorCraft and DesignCraft, then PhotoCraft/WordCraft/DeckCraft | Share bounded tick placement and painting with caller-supplied labels and coordinates. Keep units, guide commands and paragraph markers outside. |
| 7 | Pane controls | Share geometry and interactions; keep layout models in apps | PhotoCraft, VectorCraft, DesignCraft, FilmCraft, EffectCraft | Tabs, bounded splits and resize handles are implemented. A shared dock tree or persisted-layout migration remains separate work. |

Theme roles belong beside the components that consume them. `IconButtonStyle` contains fills, borders, icon and focus colors for the implemented button. That does not require all apps to replace their `Tokens` structs or serialize the same theme enum. Panels, inspectors and dialogs can then reuse those controls while retaining their application-specific contents.

## Icons and the first extraction

PhotoCraft embeds 118 SVGs, VectorCraft 259, and PdfCraft 184 through their `icon_data.rs` catalogs. PhotoCraft and PdfCraft have identical `white_icons` preparation functions at these revisions. VectorCraft uses the same replacement approach but leaks each prepared buffer to obtain a static slice. The pilot takes the owned-byte approach from PhotoCraft.

| Policy | PhotoCraft | VectorCraft | PdfCraft |
| --- | --- | --- | --- |
| `currentColor` replacement | `#ffffff` | `white` | `#ffffff` |
| Exact two-unit stroke replacement | `1.75` | `1.6` | `1.75` |
| Fallback icon | `square` | `square-dashed` | `square` |
| Existing URI prefix | `bytes://icons/` | `bytes://icon/` | `bytes://icons/` |
| Name used in fallback URI | Requested name | Requested name | Resolved fallback name |
| Existing prepared storage | Shared `Arc` | Leaked static slice | Shared `Arc` |

`SvgIconSet` owns prepared bytes and returns egui image sources. Each app supplies its embedded catalog, literal SVG substitutions, URI prefix and fallback. Tool aliases, tint, size, labels and tool cursors remain in the app; the button component accepts the app's hover/selected/pressed style policy. SVG parsing and rasterization stay with `egui_extras`.

The pilot retains all three consumers' cache keys and exact SVG bytes. Its characterization tests cover all 561 catalog entries, tool aliases and missing-name fallbacks. Shared-crate tests cover both style policies, repeated shared-byte lookups, an absent fallback, duplicate names, lossy UTF-8 compatibility and actual SVG rasterization at 16, 24 and 48 pixels. See the [crate guide](../crates/ui/README.md) for the API and limits.

The SVG substitution is deliberately narrow: it preserves the existing literal replacements rather than becoming a general SVG/CSS processor. The catalog is trusted compiled-in data. A new set in the same egui context must use a distinct URI prefix, because egui caches by URI. The existing app prefixes remain appropriate when each app owns its context.

PdfCraft resolves the fallback name before requesting a source, preserving its existing URI policy. This replaces its indexed fallback lookup with the shared non-panicking lookup; the shared absent-fallback test covers a catalog without that entry. No icon assets move.

Most other Rust apps draw icons with egui's painter. Converting them all to SVG would be a separate visual change with different cost and fidelity questions. Common icon assets are a future candidate after checking file hashes, ownership and each app's asset manifest; ArtCraft trademarks remain outside such a library.

## Themes and common controls

The `Tokens` name does not imply a common schema. PhotoCraft combines palette, layout radii, layer-label colors and histogram styling. DesignCraft includes high contrast and page-layout chrome. FilmCraft and EffectCraft include timeline/panel colors; GridCraft's theme also covers grid headers and selection. CADCraft and SoundCraft use domain-specific colors for drafting or transport states. The [theme sources above](#scope-and-current-libraries) show why replacing them with a union of every field would increase coupling.

The implemented icon button uses an explicit style struct and a supplied icon painter. It returns `egui::Response` so the app owns dispatch. A future split button should return distinct main/arrow responses. Prefer stable IDs supplied by the caller to IDs derived from translated labels. Keep dimensions configurable where they already differ. Use PdfCraft's [button and split-pill accessibility metadata][pdf-widgets] and LightCraft's [control descriptions and tests][lc-widgets] as references; appearance alone is insufficient.

An icon button or ribbon split button should be accepted only after mouse, keyboard activation, disabled behavior, focus indication, accessible label and main-versus-arrow hit regions are covered. The icon button deliberately adds a visible keyboard-focus outline in PhotoCraft and PdfCraft, and an accessible label/selection state in PhotoCraft. Existing hover, selected and pressed treatments remain app-specific. Tests cover click cancellation, keyboard activation, disabled state, label translation without losing identity, duplicate labels and Tab order, focus painting, and invalid geometry. Ribbon split buttons remain proposed.

Inspectors remain app-owned. A layer inspector, a CAD property palette, a spreadsheet format panel and an audio mixer describe different document data and editing transactions. They can share labels, rows, numeric fields, sections and color chips without sharing a panel enum or pulling an engine into a UI utility crate. Similarly, common dialog framing can be separated from save confirmation, color sampling, export settings and progress/cancellation semantics.

## Numeric controls

[VectorCraft's fields][vc-widgets] parse document units and expressions, scrub labels, select text on focus and handle wheel/key steps with modifier policies. [DesignCraft's `NumField`][dc-widgets] carries measure/number kinds, optional mixed values and spinner controls. Both need their own unit conversion and commit rules.

[FilmCraft's `hot_number`][fc-widgets] and [EffectCraft's `hot_number_at`][ec-widgets] are a more focused comparison. EffectCraft supports panel-local Tab order, decimal commas, cancellation and finite-value validation, and returns drag-completion information. FilmCraft's simpler helper does not expose the same completion contract. [LightCraft's slider][lc-widgets] uses `ControlSpec`, stepped/clamped parsing, Shift for fine adjustment and double-click reset. Shift multiplies the film/effect scrub step instead. Sharing those gestures without a policy would change behavior.

A useful shared layer would manage the edit buffer, focus transitions and commit/cancel/drag lifecycle, with caller-supplied parse/format functions and step/modifier policy. It should return a value plus an edit phase; the caller groups undo history. Keep timecode, percentages, picas and color channels in adapters. Tests need non-finite values, invalid or inverted ranges, empty/mixed values, overflow, Escape, focus loss, Tab, repeated keys, wheel ownership and exactly one undo transaction per drag. Do not extract the current helpers wholesale before those differences are characterized.

## Colors and swatches

WordCraft's [color grid][wc-widgets] returns a hex string from RGB colors. GridCraft's [palette][gc-widgets] applies the spreadsheet model's tint function and also returns hex strings. DeckCraft's [grid][deck-widgets] returns `ColorRef`, preserving theme-slot references; it distinguishes no selection from an explicit no-color result. A shared grid should receive already-resolved display colors plus opaque IDs and return an ID. It must not flatten DeckCraft's theme reference into RGB or impose one tint formula on the three apps.

PhotoCraft's [picker][pc-picker] has component fields, Lab/CMYK/hex inputs, gamut/web warnings and canvas sampling, with foreground/background and command-backed commit behavior. VectorCraft's [picker][vc-picker] has a related field/slider model and feeds its own paint and swatch types. These are good sources for a shared 2D color-field painter and hit testing. Color conversions, profile choice, original/new color, sampling and dialog acceptance should remain injected or app-owned. An sRGB preview chip is not a document color model.

egui's built-in color widgets are worth using for simple sRGB controls. They do not replace these complete document-aware dialogs. Before sharing a custom field, test hue retention at zero saturation, field/slider round trips, out-of-range input, alpha previews and the app's existing color-profile behavior.

## Menus and dialogs

[PhotoCraft's menu navigation][pc-menu-nav] has scrolling and keyboard behavior with a [dedicated test module][pc-menu-tests]; `widgets::menu_scroll` also serves context menus. LightCraft separates a [menu level][lc-menu-level] from its menu definitions. VectorCraft's [menus][vc-menus] add dynamic labels, command availability, swatches, fonts and native-menu integration. Those are different layers.

Share viewport-constrained popup geometry, scrolling, hover delays and focus/navigation state before sharing a menu tree. Inputs should be display labels, stable IDs, checked/enabled state and callbacks or returned selections. The app resolves shortcuts, localizes labels and runs commands. Native menu services belong to issue 2's platform workstream, so this audit does not propose a second OS integration abstraction.

Dialog shells need explicit modality, Escape/default-button policy, focus restoration and child-popup handling. CADCraft uses ordinary [egui windows][cad-dialogs] for some tools; PhotoCraft has a [dialog stack][pc-dialogs] and canvas interactions during color sampling. One unconditional modal wrapper would lose these differences. Preserve existing save/cancel behavior and test nested popups, narrow windows and focus recovery before adopting a shell.

## Rulers and docking

[PhotoCraft's rulers][pc-rulers] use pixels and document resolution, have unit-menu/guide-drag tests and normalize flipped ranges. [VectorCraft][vc-canvas] and [DesignCraft][dc-canvas] use document units and their own tick/label painting; DesignCraft also has page/spread origins and stacked vertical labels. [WordCraft][wc-canvas] adds paragraph indent and tab-stop interaction. [DeckCraft][deck-canvas] paints slide rulers. SoundCraft's [ruler area][sc-edit] is based on audio time formats. A generic list of ticks can serve these consumers; a shared unit enum or guide controller cannot yet.

Start with an axis range, screen transform, tick budget and caller-produced labels. Tick generation must terminate for tiny zoom, non-finite values and enormous ranges. Guides, indent markers, unit menus and time conversion stay in adapters. Reuse the existing ruler tests as characterization and add bounded-work tests before extracting loops.

FilmCraft's [dock tree][fc-dock] has splits, fixed/relative sizes and tab groups. EffectCraft's [dock tree][ec-dock] adds stacks, floating layouts and additional panel/viewer identities. PhotoCraft's [dock][pc-dock] allocates vertical groups with preferred/minimum/fill heights and has [interaction/persistence tests][pc-dock-tests]. VectorCraft and DesignCraft also have icon rails and flyouts. These are related capabilities, not a single interchangeable serialized model.

The pane extraction compared [egui_dock 0.21.1](https://docs.rs/egui_dock/0.21.1/egui_dock/) and [egui_tiles 0.17.1](https://docs.rs/egui_tiles/0.17.1/egui_tiles/) at their egui 0.36 APIs. `egui_dock` offers binary fraction splits and floating egui windows within one OS window. `egui_tiles` offers tab, linear and grid containers with relative shares. Neither directly represents EffectCraft's accordion stack plus FilmCraft/EffectCraft's fixed-first/fixed-second sizing. Migrating either would require adapters for saved layouts, panel IDs and commands, as well as bounds on untrusted layout traversal. This source comparison does not establish runtime acceptance of either library.

The implemented boundary therefore stays small: `craft-ui::tabs` fits labels and registers accessible tab interactions; `craft-ui::layout` computes one bounded split and reports resize movement. Apps keep dock trees, panel contents, stacks, floating state and serialization. PhotoCraft and VectorCraft retain their existing overflow/drag policies in adapters; VectorCraft gains fitted tabs and an overflow menu. FilmCraft and EffectCraft retain fixed and relative sizing, with focusable resize handles. DesignCraft and PhotoCraft reuse geometry for their existing split document views. No native multi-monitor or dock-tree migration is implied.

The shared geometry relaxes minima in tiny areas and rejects non-finite or inverted inputs. EffectCraft's former inverted-clamp case receives a synthetic regression test. Interaction tests cover pointer deltas, arrow keys, focus indication and AccessKit identity/value/actions; application tests retain responsibility for docking, pane focus, document cameras and saved layouts.

## ArtCraft and ArtCraftX

The React apps are worth including in the inventory, but they need a separate package decision. Their component directories contain shared ancestry and later divergence. For example, the [ArtCraft modal][ac-modal] contains delayed recovery of a stranded body pointer-events lock, while the [ArtCraftX modal][ax-modal] at this snapshot lacks that block. Both have similar portal, animation, dragging and Escape-stack machinery. This is a concrete comparison target; the source difference alone does not prove an ArtCraftX runtime bug.

Their [button implementations][ac-button] also have [different styling and icon wrappers][ax-button]. Extracting behavior beneath theme adapters is more plausible than copying one app's full presentation into the other. Preserve keyboard/focus and portal behavior, and test rapid open/close, stacked dialogs and nested menus in a real browser.

Their licensing also needs separate treatment. ArtCraft's [root license][ac-license] states usage restrictions; ArtCraftX's root tree has no corresponding license file at this revision. A `license` string in an individual frontend package is not enough evidence to relicense copied application code into this MIT/Apache workspace. The pilot copies no code or assets from either React app. Any later extraction needs a clear license/provenance decision from the owners.

## Adoption and acceptance

`craft-ui` exposes focused components; each app adopts them independently through its UI crate and pins an immutable shared revision. Icon catalogs serve PhotoCraft, VectorCraft and PdfCraft; icon buttons serve PhotoCraft and PdfCraft. Tabs serve PhotoCraft and VectorCraft. Split geometry serves DesignCraft, PhotoCraft, FilmCraft and EffectCraft, with resize handles in FilmCraft and EffectCraft.

Run the shared crate's fmt, strict Clippy, tests and wasm check, followed by each adopting app's own gates. Compare actual UI renders at normal and narrow sizes, verify pointer and keyboard interactions, and distinguish passing focused checks from full application acceptance. Local path overrides are development tools; dependency manifests use immutable Git pins.

A shared UI dependency stays above the engine boundary, compiles for wasm, preserves source notices and avoids panic paths. An app can roll back adoption independently by reverting its adapter and dependency pin. Dock trees, layout persistence, document identity, command catalogs and platform windows remain app-owned.
## Source revisions

Snapshot date: 2026-10-09. The links throughout this report resolve to these exact revisions, not moving branch heads. The audit covers the listed source areas. The component consumers are listed above; the remaining apps are reference candidates.

| Repository | Revision |
| --- | --- |
| `storytold/artcraft` | [`3e5793b6934b`](https://github.com/storytold/artcraft/tree/3e5793b6934b51536606720e5d56bcfd1fe7cc2d) |
| `storytold/artcraftx` | [`98c2a48940aa`](https://github.com/storytold/artcraftx/tree/98c2a48940aa0a54604637f665ee539c122fa707) |
| `storytold/cadcraft` | [`86f3199b400c`](https://github.com/storytold/cadcraft/tree/86f3199b400c271a1166103519b3785e4ac88e72) |
| `storytold/deckcraft` | [`4b09f35c1d6f`](https://github.com/storytold/deckcraft/tree/4b09f35c1d6fa50e2d94243a5d65d98fe49558e0) |
| `storytold/designcraft` | [`66c7ce7e96ec`](https://github.com/storytold/designcraft/tree/66c7ce7e96ec286a091878cc2541ee8369aa79d6) |
| `storytold/effectcraft` | [`2a460986ee7a`](https://github.com/storytold/effectcraft/tree/2a460986ee7a4b32656a3d434849693904b486bc) |
| `storytold/filmcraft` | [`7b6c13447228`](https://github.com/storytold/filmcraft/tree/7b6c134472287db4367fca80bcb498240ba36567) |
| `storytold/gridcraft` | [`51b531be3e8b`](https://github.com/storytold/gridcraft/tree/51b531be3e8b3f5a19d2b7714a4780a79569ca92) |
| `storytold/lightcraft` | [`88ea1a804b35`](https://github.com/storytold/lightcraft/tree/88ea1a804b35709e1fe436e3792adf0428b97daa) |
| `storytold/pdfcraft` | [`f626db25cca2`](https://github.com/storytold/pdfcraft/tree/f626db25cca2c89f8fa3f7e30d22bf59faea65ec) |
| `storytold/photocraft` | [`e7a83ca342cd`](https://github.com/storytold/photocraft/tree/e7a83ca342cdff568254f85f442b9cc8afb08066) |
| `storytold/soundcraft` | [`eac0edd896cd`](https://github.com/storytold/soundcraft/tree/eac0edd896cd6bdf27c0cdd0f9f9158c8956248a) |
| `storytold/vectorcraft` | [`825d4fdb8b52`](https://github.com/storytold/vectorcraft/tree/825d4fdb8b527c597855d273fb4bcc1971855b6a) |
| `storytold/wordcraft` | [`d7d9702bda1f`](https://github.com/storytold/wordcraft/tree/d7d9702bda1fe626952024dc37fd2624323dad0f) |

[pc-theme]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/theme.rs
[pc-widgets]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/widgets.rs
[pc-icons]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/icons.rs
[pc-dock]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/dock.rs
[pc-rulers]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/rulers.rs
[pc-picker]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/color_picker_ui.rs
[pc-menu-nav]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/menu_nav.rs
[pc-menu-tests]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/menu_nav_tests.rs
[pc-dock-tests]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/dock_tests.rs
[pc-dialogs]: https://github.com/storytold/photocraft/blob/e7a83ca342cdff568254f85f442b9cc8afb08066/crates/ui-egui/src/dialogs.rs
[vc-theme]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/theme.rs
[vc-widgets]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/widgets.rs
[vc-icons]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/icons.rs
[vc-dock]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/dock.rs
[vc-canvas]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/canvas.rs
[vc-picker]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/dialogs/color_picker.rs
[vc-menus]: https://github.com/storytold/vectorcraft/blob/825d4fdb8b527c597855d273fb4bcc1971855b6a/crates/ui-egui/src/menus.rs
[dc-theme]: https://github.com/storytold/designcraft/blob/66c7ce7e96ec286a091878cc2541ee8369aa79d6/crates/ui-egui/src/theme.rs
[dc-widgets]: https://github.com/storytold/designcraft/blob/66c7ce7e96ec286a091878cc2541ee8369aa79d6/crates/ui-egui/src/widgets.rs
[dc-icons]: https://github.com/storytold/designcraft/blob/66c7ce7e96ec286a091878cc2541ee8369aa79d6/crates/ui-egui/src/icons.rs
[dc-dock]: https://github.com/storytold/designcraft/blob/66c7ce7e96ec286a091878cc2541ee8369aa79d6/crates/ui-egui/src/dock.rs
[dc-canvas]: https://github.com/storytold/designcraft/blob/66c7ce7e96ec286a091878cc2541ee8369aa79d6/crates/ui-egui/src/canvas.rs
[pdf-theme]: https://github.com/storytold/pdfcraft/blob/f626db25cca2c89f8fa3f7e30d22bf59faea65ec/crates/ui-egui/src/theme.rs
[pdf-widgets]: https://github.com/storytold/pdfcraft/blob/f626db25cca2c89f8fa3f7e30d22bf59faea65ec/crates/ui-egui/src/widgets.rs
[pdf-icons]: https://github.com/storytold/pdfcraft/blob/f626db25cca2c89f8fa3f7e30d22bf59faea65ec/crates/ui-egui/src/icons.rs
[pdf-dialogs]: https://github.com/storytold/pdfcraft/blob/f626db25cca2c89f8fa3f7e30d22bf59faea65ec/crates/ui-egui/src/dialogs.rs
[lc-theme]: https://github.com/storytold/lightcraft/blob/88ea1a804b35709e1fe436e3792adf0428b97daa/crates/ui-egui/src/theme.rs
[lc-widgets]: https://github.com/storytold/lightcraft/blob/88ea1a804b35709e1fe436e3792adf0428b97daa/crates/ui-egui/src/widgets.rs
[lc-menu]: https://github.com/storytold/lightcraft/blob/88ea1a804b35709e1fe436e3792adf0428b97daa/crates/ui-egui/src/menubar.rs
[lc-menu-level]: https://github.com/storytold/lightcraft/blob/88ea1a804b35709e1fe436e3792adf0428b97daa/crates/ui-egui/src/menu_level.rs
[fc-theme]: https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/ui-egui/src/theme.rs
[fc-widgets]: https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/ui-egui/src/widgets.rs
[fc-dock]: https://github.com/storytold/filmcraft/blob/7b6c134472287db4367fca80bcb498240ba36567/crates/ui-egui/src/dock.rs
[ec-theme]: https://github.com/storytold/effectcraft/blob/2a460986ee7a4b32656a3d434849693904b486bc/crates/ui-egui/src/theme.rs
[ec-widgets]: https://github.com/storytold/effectcraft/blob/2a460986ee7a4b32656a3d434849693904b486bc/crates/ui-egui/src/widgets.rs
[ec-dock]: https://github.com/storytold/effectcraft/blob/2a460986ee7a4b32656a3d434849693904b486bc/crates/ui-egui/src/dock.rs
[wc-theme]: https://github.com/storytold/wordcraft/blob/d7d9702bda1fe626952024dc37fd2624323dad0f/crates/ui-egui/src/theme.rs
[wc-widgets]: https://github.com/storytold/wordcraft/blob/d7d9702bda1fe626952024dc37fd2624323dad0f/crates/ui-egui/src/widgets.rs
[wc-canvas]: https://github.com/storytold/wordcraft/blob/d7d9702bda1fe626952024dc37fd2624323dad0f/crates/ui-egui/src/canvas.rs
[gc-theme]: https://github.com/storytold/gridcraft/blob/51b531be3e8b3f5a19d2b7714a4780a79569ca92/crates/ui-egui/src/theme.rs
[gc-widgets]: https://github.com/storytold/gridcraft/blob/51b531be3e8b3f5a19d2b7714a4780a79569ca92/crates/ui-egui/src/widgets.rs
[gc-dialogs]: https://github.com/storytold/gridcraft/blob/51b531be3e8b3f5a19d2b7714a4780a79569ca92/crates/ui-egui/src/dialogs.rs
[deck-theme]: https://github.com/storytold/deckcraft/blob/4b09f35c1d6fa50e2d94243a5d65d98fe49558e0/crates/ui-egui/src/theme.rs
[deck-widgets]: https://github.com/storytold/deckcraft/blob/4b09f35c1d6fa50e2d94243a5d65d98fe49558e0/crates/ui-egui/src/widgets.rs
[deck-canvas]: https://github.com/storytold/deckcraft/blob/4b09f35c1d6fa50e2d94243a5d65d98fe49558e0/crates/ui-egui/src/canvas.rs
[cad-theme]: https://github.com/storytold/cadcraft/blob/86f3199b400c271a1166103519b3785e4ac88e72/crates/ui-egui/src/theme.rs
[cad-chrome]: https://github.com/storytold/cadcraft/blob/86f3199b400c271a1166103519b3785e4ac88e72/crates/ui-egui/src/chrome.rs
[cad-palettes]: https://github.com/storytold/cadcraft/blob/86f3199b400c271a1166103519b3785e4ac88e72/crates/ui-egui/src/palettes.rs
[cad-dialogs]: https://github.com/storytold/cadcraft/blob/86f3199b400c271a1166103519b3785e4ac88e72/crates/ui-egui/src/dialogs.rs
[sc-theme]: https://github.com/storytold/soundcraft/blob/eac0edd896cd6bdf27c0cdd0f9f9158c8956248a/crates/ui-egui/src/theme.rs
[sc-widgets]: https://github.com/storytold/soundcraft/blob/eac0edd896cd6bdf27c0cdd0f9f9158c8956248a/crates/ui-egui/src/widgets.rs
[sc-toolbar]: https://github.com/storytold/soundcraft/blob/eac0edd896cd6bdf27c0cdd0f9f9158c8956248a/crates/ui-egui/src/toolbar.rs
[sc-edit]: https://github.com/storytold/soundcraft/blob/eac0edd896cd6bdf27c0cdd0f9f9158c8956248a/crates/ui-egui/src/edit_window.rs
[ac-package]: https://github.com/storytold/artcraft/blob/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/frontend/package.json
[ac-components]: https://github.com/storytold/artcraft/tree/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/frontend/libs/components
[ac-theme]: https://github.com/storytold/artcraft/blob/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/frontend/apps/artcraft/tailwind.config.ts
[ac-modal]: https://github.com/storytold/artcraft/blob/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/frontend/libs/components/modal/src/lib/modal.tsx
[ac-button]: https://github.com/storytold/artcraft/blob/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/frontend/libs/components/button/src/lib/button.tsx
[ac-license]: https://github.com/storytold/artcraft/blob/3e5793b6934b51536606720e5d56bcfd1fe7cc2d/LICENSE.md
[ax-package]: https://github.com/storytold/artcraftx/blob/98c2a48940aa0a54604637f665ee539c122fa707/frontend/package.json
[ax-components]: https://github.com/storytold/artcraftx/tree/98c2a48940aa0a54604637f665ee539c122fa707/frontend/libs/components
[ax-theme]: https://github.com/storytold/artcraftx/blob/98c2a48940aa0a54604637f665ee539c122fa707/frontend/apps/artcraft/tailwind.config.ts
[ax-modal]: https://github.com/storytold/artcraftx/blob/98c2a48940aa0a54604637f665ee539c122fa707/frontend/libs/components/modal/src/lib/modal.tsx
[ax-button]: https://github.com/storytold/artcraftx/blob/98c2a48940aa0a54604637f665ee539c122fa707/frontend/libs/components/button/src/lib/button.tsx
