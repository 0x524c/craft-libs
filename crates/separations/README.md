# craft-separations

Print separations for the Craft apps:

- `ink`: the ink model the writers and previews share. `InkSet` holds the four process inks
  (always first, in plate order) and the spot inks of a job; `InkColor` is a colour as ink
  tints (`Separation`, `DeviceN`, registration or none); `Overprint` holds `op`, `OP` and
  `OPM`. Spot names are compared as PDF names: `/Gold#20Foil` and `Gold Foil` are one ink.
- `plates`: one 8-bit plane per ink (255 = 100% ink) rendered from a PDF page.
- `cms`: process colour conversion into a CMYK printing condition through craft-cms.

## Plates from an exported PDF

An app exports its PDF as usual, then separates a page of it:

```rust
use craft_separations::{plates, InkId, Options, Process};

let p = plates(&pdf, 0, 150.0, &Options::default())?;
for (id, ink) in p.inks.iter() {
    let plane = p.plane(id); // p.width × p.height tints, row-major
    println!("{}: {} pixels inked", ink.name(), plane.map_or(0, |t| t.iter().filter(|v| **v > 0).count()));
}
let black = p.tint(InkId::process(Process::Black), 10, 10);
let gold = p.inks.lookup("Gold").and_then(|id| p.tint(id, 10, 10));
```

The plates are the PDF as printed, so a preview built on them shows what the writer produced,
not what the document model intended.

How a plate is made: the page renders once per ink through craft-pdf's `raster` with
hayro-interpret's plate mode (`InterpreterSettings::plate`). Every paint draws its tint of
that ink as gray film. A paint that overprints (`op`/`OP`, and `OPM 1` for zero DeviceCMYK
components) without naming the ink leaves the film as it is; with `Options::overprint` off,
every paint knocks out. Clipping, anti-aliasing, soft masks, transparency and knockout groups
stay the renderer's own. `Separation` and `DeviceN` colorants paint their own plates (`All`
every plate, `None` none), DeviceCMYK its components, DeviceGray black only.

Inks are found by a low-resolution pass that records each spot ink a paint uses, in the order
first painted. Spot inks used only inside soft masks are left out. Each spot gets a Lab
alternate measured from its sRGB appearance at 100%, so out-of-sRGB spots are clipped.

### Process colour

RGB, CIE-based and ICCBased colours become CMYK through, in order:

1. the CMYK profile of the page's or the document's output intent (a `GTS_PDFX` intent first),
   when craft-cms can convert into it;
2. `Options::fallback`: craft-cms's Coated FOGRA39 profile (the default), a profile the caller
   gives, or the ISO 32000-2 §10.4.2.4 device formulas.

`Plates::conversion` says which was used. Conversions are relative colorimetric without black
point compensation. DeviceCMYK, and ICCBased CMYK in the printing condition's own profile,
separate as they are.

### Caps

| Constant | Value | Why |
|---|---|---|
| `MAX_SPOT_INKS` | 32 spot inks per page | A `DeviceN` space has at most 32 colorants, and each plate costs a render and a byte per pixel. Further spots are listed in `Plates::unseparated`. |
| `MAX_PLATE_PIXELS` | 4 MP per plate (2000 × 2000) | A page with many inks stays within a few hundred megabytes. The resolution is lowered to fit; `Plates::dpi` reports the one used. |

The renderer's side limit (`craft_pdf::raster::MAX_SIDE`) also lowers the resolution.

### Approximations

- Non-separable blend modes and groups that blend in RGB are approximated per ink.
- The graphics state's rendering intent is not read; transfer functions and halftones are not
  applied.

## Credits

- Plate mode, ink discovery and the output-intent handling come from
  [storytold/pdfcraft#2770](https://github.com/storytold/pdfcraft/pull/2770) by Guillem Hermida
  (guillehr2), with craft-cms in place of moxcms.
- Knockout groups in the renderer come from
  [storytold/pdfcraft#2974](https://github.com/storytold/pdfcraft/pull/2974) by mtlcl410.

Both MIT OR Apache-2.0, as this crate.
