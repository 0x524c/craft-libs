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
