# craft-i18n

Translations for the Craft apps: lookup with fallback, plurals, language names and the system
locale, over the catalogs of [craft-i18n](https://github.com/benandez/craft-i18n) (the data
repository, same name).

The catalogs are a fully optional build input. An app's `build.rs` embeds them from
`CRAFT_I18N_DIR` as a static `CRAFT_I18N: &[(locale, shared catalog, app catalog)]`, empty when the
variable is unset; the recipe is in craft-i18n's
[`docs/integration.md`](https://github.com/benandez/craft-i18n/blob/main/docs/integration.md).
This crate never reads files.

## Wiring an app

```rust
use std::cell::Cell;
use std::sync::LazyLock;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs")); // pub static CRAFT_I18N: &craft_i18n::Embedded

pub static I18N: LazyLock<craft_i18n::I18n> = LazyLock::new(|| {
    let offered: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect(); // the app's own list
    let (i18n, problems) = craft_i18n::I18n::new(CRAFT_I18N, &offered); // parses nothing yet
    problems.iter().for_each(|p| log::warn!("translation catalog: {p}"));
    i18n
});

thread_local! { static CURRENT: Cell<craft_i18n::Lang> = const { Cell::new(craft_i18n::Lang::EN) }; }

macro_rules! tl { ($s:expr) => { $crate::I18N.t($crate::CURRENT.get(), $s) }; }
```

The app keeps its `tl!` macro, its language setting (`"auto"` → `I18N.system_lang()`, otherwise
`I18N.from_code`) and its picker (`I18N.languages()`, `I18N.name(lang)`).

## Behaviour

- **Offered locales:** `I18n::new` takes the codes the app offers (its own language list), so an
  app never offers a language its interface isn't ready for (Arabic without right-to-left layout,
  say). `languages()`, `from_code`, `match_tag` and `system_lang` only return offered locales; an
  embedded locale that isn't offered still serves as a fallback (`pt` for an offered `pt-br`).
- **Fallback:** locale by locale along the chain, `pt-br` → `pt` → the English source text. In
  each locale, app rows override shared ones, and `tc`/`tid` try their context or `@id` row before
  the plain row, so `pt-br`'s plain row beats `pt`'s `menu` row. Only a region falls back (`pt-br`
  → `pt`, `zh-hant-hk` → `zh-hant`); `sr-latn` never falls back to Cyrillic `sr`. A missing
  translation never errors.
- **Lookups:** `t` (plain), `tc` (with a context such as `menu` or `@msg`, else plain), `tid` (an
  `@id` command id, else the English label), `trn` (a count, `@plural` rows, `{n}` filled), `get`
  (one context along the locale chain, so `pt-br` can return a `pt` row, but never the plain row
  or English: a key cap's name under `key`, or "is there a translation?"), and `fmt` for `{name}`
  placeholders, filled in one pass.
- **Isolation:** when a message or a value inserted into it has a right-to-left letter (bidi
  class R or AL), or a value has a bidi control (LRE, RLE, PDF, LRO, RLO, LRI, RLI, FSI, PDI,
  LRM, RLM, ALM), `fmt` wraps each inserted value in a first-strong isolate (FSI…PDI), and `trn`
  wraps `{n}` in a left-to-right one (LRI…PDI), so a Latin file name in an Arabic sentence, a
  Hebrew name in an English one, or an ASCII file name with a right-to-left override can't
  reorder the words around it. Embeddings, overrides and isolates a value opens are closed inside
  its isolate, and a stray PDF or PDI in it is dropped. Wrap numbers, paths and codes in `ltr`
  before passing them; a value that already is one isolate keeps it. Other text comes out
  without marks, as if `fmt` didn't isolate at all. egui draws the marks as nothing. For text
  that leaves the interface (file names, logs, terminals, text parsed later), use `fmt_plain` or
  `strip_isolates`.
- **Bad rows** (malformed, an unknown `@` context, a wrong plural form count, a row defined twice,
  of which the first is kept) and bad locales (`en`, `pseudo`, a code embedded twice in any case)
  are skipped; craft-i18n's CI rejects them before they ship. `I18n::new` parses nothing: it
  returns the bad locales, and each locale's rows are parsed (and its bad rows logged with
  `log::warn!`) on the first lookup that reaches it, including as a fallback. `I18n::check_all()`
  parses every locale and returns all problems (`<locale>/<common|app>: line N: …`), for a test.
- **Plurals:** forms are listed in CLDR order (zero, one, two, few, many, other) for the categories
  whole numbers 0 to 1000 reach, from `intl_pluralrules`; a count in any other category takes the
  last form. CLDR's `pt` is Brazilian, so `pt` uses `pt-PT`'s rules (0 is plural) and `pt-br`
  `pt`'s. `intl_pluralrules` is pinned to the version craft-i18n's checker uses, and
  `plural_form_counts` holds the same counts.
- **Names:** a language's name comes from the app catalog's `@id language.name` row, else the
  shared catalog's, else its code; English is "English".
- **Scripts and direction:** for any language tag, `scripts` lists the ISO 15924 scripts its
  interface text is written in (`ja` → Hani, Hira, Kana; `sr-latn` → Latn; `pa-PK` → Arab),
  `direction` is right to left when the main script is (Arabic, Hebrew, Syriac, Thaana), and `han`
  gives the regional forms of its Han characters for picking a CJK face: `zh-CN`/`zh-SG`/`zh-Hans` →
  SC, `zh-TW`/`zh-US`/`zh-Hant` → TC, `zh-HK`/`zh-MO` → HK, `ja` → JP, `ko` → KR. `I18n::rtl` is
  `direction` of the language's code; putting text into display order is the app's job.
- **System locale:** `system_lang` matches the system's list once and caches it: on desktop Unix
  `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG`, read raw; elsewhere `sys-locale`'s (macOS and
  Windows preferred languages, the browser's on the web). `fr_CA.UTF-8` → `fr`, `zh-TW` → `zh-hant`,
  `pt-PT` → `pt-br` when that is the only Portuguese, `C`/`POSIX` → English. A script is never
  crossed: `sr_RS@latin` and `sr-Latn-RS` pick `sr-latn`, never `sr`, while `sr-Cyrl-RS` picks `sr`.
  A region that implies a script counts as that script: `pa-PK` picks `pa-arab`, not Gurmukhi `pa`.
- **Pseudo locale** (`pseudo`, debug builds only, never in `languages()`): every lookup, `get`
  included, comes back as `⟦…⟧`, line by line, so text drawn without the marks never went through
  translation. `looked_up()` lists the source texts looked up so far, for an audit against a
  complete catalog.
