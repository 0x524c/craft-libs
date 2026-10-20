# Shared Hayro reader snapshot

The `hayro`, `hayro-interpret`, `hayro-syntax` and `hayro-jbig2` directories are transferred
from [PdfCraft revision ec0a965074f376a5bf610435948e4e5184bf3299](https://github.com/storytold/pdfcraft/tree/ec0a965074f376a5bf610435948e4e5184bf3299/vendor)
(`main`, 2026-10-19). The upstream [patch ledger](https://github.com/storytold/pdfcraft/blob/ec0a965074f376a5bf610435948e4e5184bf3299/vendor/README.md)
describes the inherited patches. Source manifests record every transferred Git blob and
SHA-256 alongside the local hash. The source manifests themselves are not self-hashed.

The first transfer was from PdfCraft revision 7437bde378f43856eada40af25b983812e615e8a.
The snapshot was then re-synced to ec0a965 by a three-way merge (base 7437bde, PdfCraft's
current tree, this tree), so every local patch described below is carried unchanged. The
re-sync brings these PdfCraft changes:

| PdfCraft PR | Crate | Change |
|---|---|---|
| #1691 | hayro-interpret | Patch 28: images with few possible pixel values convert each value once (spot-colour images) |
| #1903, #3316 | hayro-syntax | CCITT images that declare `/Rows 0`, or fewer `/Rows` than `/Height`, decode to their height |
| #1954 | hayro-syntax | ASCII85 decoding stops at the stream's decode limit (here the per-document limit) |
| #1982 | hayro-syntax | `Page::base_dimensions` keeps a one-point floor after `/UserUnit` |
| #2027 | hayro-interpret | Reads a transparency group's `/I` and `/K`; a group that is neither isolated nor knockout composites directly onto its backdrop |
| #2132 | hayro | A soft mask fades a gradient fill |
| #1557 | hayro-interpret, hayro | Patch 29: devices are told where annotation drawing begins and ends |
| 13e3154 | hayro-interpret | Annex D.2 alias codes in standard, TrueType, Type 1 and Type 3 fonts read as the glyphs they draw |
| #2436 | hayro-interpret | Patches 31–33: faster sampled functions, image decodes reused across zoom levels, patch meshes interpolated once per grid point |
| #2812 | hayro-interpret | Patch 34: a per-thread tint-transform counter for deterministic tests |
| #2035 | hayro-syntax | Flate back-references are copied in runs |
| #3356 | hayro | A tiling pattern drawn once shows its cell at full scale |
| #3378 | hayro-syntax | Patch 20: spaces or tabs between `stream` and its line break are accepted |
| #3363 | hayro-syntax | A JP2 image in a colour space the decoder refuses is drawn by its component count |
| #3412 | hayro-interpret | Type 3 glyph bounding boxes are transformed to device space before shading bounds are intersected |

PdfCraft's vendored `vello_common` patch (clip-intersection performance) is not part of this
reader snapshot.

Changes ported from PdfCraft pull requests that were open when they were ported, under the
same MIT OR Apache-2.0 licence. Code comments keep the pull request's patch numbers, except
where noted below.

| PdfCraft PR | Author | Source commit | Files | Change |
|---|---|---|---|---|
| [#2770](https://github.com/storytold/pdfcraft/pull/2770) | Guillem Hermida (guillehr2) | [6fcf2a4](https://github.com/guillehr2/pdfcraft/commit/6fcf2a4aca29e943e28cb69017ff8f87d00656c6) | hayro-interpret `color.rs`, `interpret/mod.rs`, `interpret/path.rs`, `interpret/state.rs`, `pattern.rs`, `shading.rs`, `soft_mask.rs`, `x_object.rs` | Patches 35 and 36, plate mode: `InterpreterSettings::plate` draws one ink's separation as a gray film (`color::Plate`, `ColorSpace::ink_use`/`ink_tint`); the graphics state reads `OP`, `op` and `OPM` and an overprinting paint that doesn't specify the plate's ink leaves the plate as it is (ISO 32000-2 §8.6.7); `InterpreterSettings::ink_sink` reports spot inks; `color::CmykSimulation` converts process colour to an output intent's CMYK. Only the vendored interpreter part is ported; PdfCraft's `separations.rs` and Output Preview UI are not |
| [#2974](https://github.com/storytold/pdfcraft/pull/2974) | mtlcl410 | [bae6981](https://github.com/mtlcl410/pdfcraft/commit/bae69819594a6385a027fec1f67e6b3c36c259b4) | hayro-interpret `device.rs`, `x_object.rs`; hayro `renderer.rs` | Knockout groups (`/K true`) knock out: while one is innermost, fills, strokes, glyph outlines and shadings are drawn with `Compose::Copy` (weighted by coverage); an unmasked image's rectangle (`Device::knockout_image`) and a nested group's shape (an erase pass between `Device::begin_knockout_erase` and `end_knockout_erase`) are cleared before their layer composites; a transparency group drawn in a knockout group always gets a layer of its own. Masked images and stencils composite as before |

Code comments label #2974's changes `Knockout patch (pdfcraft #2974)` instead of a number: the
pull request numbers them hayro-interpret patch 39 and hayro patch 19, and PdfCraft may
renumber them when it merges. `crates/pdf/tests/knockout.rs` adapts its test; a plate-mode
test in `crates/pdf/tests/plates.rs` checks that a knockout group separates with only the top
object where two objects overlap. The erase pass resolves the group's resources with
`Resources::from_scope`, as the group's own draw does in this tree.

The output-intent conversion of patch 36 goes through a `color::CmykConversion` trait, so
that callers can convert with their own colour management. `CmykSimulation::new` keeps
#2770's moxcms conversion as the default, and `CmykSimulation::with_conversion` takes a
caller's. Lab and ICCBased colours reach the conversion as CIE XYZ relative to the D50 PCS
white (`color::PCS_WHITE`); the moxcms conversion takes them on through #2770's ProPhoto hub.
`crates/pdf/tests/plates.rs` adapts #2770's plate tests to the raster path, with its
synthetic ICC profiles in `crates/pdf/tests/support/synthetic_icc.rs`.

The shared snapshot adds immutable per-document ordinary decode limits, threaded through
initial xrefs, object streams, fallback and repair. Existing constructors retain their
previous default. Known-size image budgets and streaming content budgets remain distinct.
ContentReader now reports omitted or failed whole streams and incomplete Flate blocks as
known incomplete content. The interpreter also reports failed or budget-reaching nested
forms, patterns and Type 3 procedures. A stream exposes its immutable document limit so
callers can use the same conservative completion check. This diagnostic does not cover
structural repair or every omitted object and is not a rendering-completeness guarantee.
The syntax manifest pins JBIG2 exactly to 0.3.0 so a newer registry release cannot bypass
the transferred decoder patches.
Synthetic unit tests cover these boundaries; two version tests and the introductory doctest
use synthetic PDFs so they do not rely on the absent upstream corpus. No corpus is copied.

Compatibility patches follow the resource scopes and text-showing order in
[Hayro 0.8.0, revision ea9c81dc70cde40c64205986fdcfcd481fe934d8](https://github.com/LaurenzV/hayro/tree/ea9c81dc70cde40c64205986fdcfcd481fe934d8),
under the same MIT OR Apache-2.0 licence. Explicit page and nested-object resource
dictionaries replace the enclosing dictionary as a whole. Missing Form resources inherit
the caller; tiling patterns use only their own resources. Direct arrays and dictionaries
are limited to 64 levels in raw and decoded content. Page-tree traversal is iterative,
retains cycle detection and inheritance, and fails closed when its 100,000-entry work
budget or a fallible frame/page allocation fails. Text-showing operations fill the
whole run before stroking it, including strings separated by adjustments in `TJ`.
The implementation replays bounded source operands without allocating a glyph buffer.
The existing Type 3 single-paint behavior and incomplete-content diagnostics remain.
Raw array iteration now ends safely if validation and object parsing disagree;
standalone sign tokens retain the reader's zero tolerance.
Byte transfer-function samples use rounded 8-bit values; failed or non-finite evaluations
leave the sample unchanged. Historical source hashes remain in each `SOURCE.json`,
alongside local hashes and the compatibility reference.

This remains the patched 0.7 backend. It retains the existing Bradford Lab conversion,
floating component precision and codec versions, rather than claiming full 0.8
equivalence. The independent color oracles, backend tests and facade contracts cover
these retained policies separately from the compatibility changes.

All other source and asset files remain unchanged. Upstream licence texts, Foxit/PDFium
standard-14 fonts and the CMYK ICC profile are retained. Attribution is in the root NOTICE
and ATTRIBUTION.md. The removed LAB.icc profile remains absent. Lopdf is not part of this
snapshot: applications that need its editing or inspection compatibility keep owning it.

Root Cargo patch entries are required in consuming workspaces. The writer-only feature
remains independent; vendoring reader sources does not make them a writer dependency.

The transferred snapshot includes the latest Lab Bradford adaptation, Type 3 rendering-mode
handling and filtered shrinking of oversized soft masks. `tests/upstream_raster.rs` adapts
the corresponding portable PdfCraft rendering contracts for native and WASM.
The snapshot also preserves PdfCraft #1477 (WinAnsi Unicode fallback for TrueType
fonts without Encoding) and #1499 (byte-aligned packed TIFF predictor rows).
Synthetic tests cover 1/2/4/8-bit RGB rows and rendering, plus embedded TrueType
text with absent or partial ToUnicode maps and explicit encoding precedence.
