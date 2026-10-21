use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::cmap::Cmap;
use crate::embed::Env;
use crate::{Embed, EmbeddedFace, Error, parse_manifest};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/craft-fonts")
}

/// A fresh scratch directory under the system temp dir.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("craft-fonts-build-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    result: Result<Vec<String>, Error>,
    cargo: Vec<String>,
    table: String,
}

/// Run `embed` with `CRAFT_FONTS_DIR` and `CRAFT_FONTS_REQUIRED` as given; returns the embedded
/// face names, the cargo lines and the generated table.
fn run(embed: Embed, dir: Option<&Path>, required: Option<&str>) -> Run {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let scratch = Scratch::new(&format!("run-{}", RUNS.fetch_add(1, Ordering::Relaxed)));
    let env = Env { dir: dir.map(OsString::from), required: required.map(OsString::from), out_dir: Some(scratch.0.clone().into()) };
    let mut cargo = Vec::new();
    let result = embed.run(&env, Path::new("craft_fonts.rs"), &mut cargo);
    let table = std::fs::read_to_string(scratch.0.join("craft_fonts.rs")).unwrap();
    Run { result: result.map(|faces| faces.iter().map(|f| format!("{} {}", f.family, f.style)).collect()), cargo, table }
}

#[test]
fn parses_the_manifest_with_collection_indices() {
    let text = std::fs::read_to_string(fixture().join("fonts/manifest.txt")).unwrap();
    let entries = parse_manifest(&text).unwrap();
    assert_eq!(entries.len(), 6);
    assert_eq!((entries[0].family.as_str(), entries[0].style.as_str()), ("Sans JP", "Regular"));
    assert_eq!(entries[0].scripts, ["Jpan", "Latn"]);
    assert_eq!((entries[0].file.as_str(), entries[0].index), ("fonts/sans-jp/SansJP-Regular.ttf", 0));
    assert_eq!((entries[4].file.as_str(), entries[4].index), ("fonts/sans-cjk/SansCJK.ttc", 3));
    assert_eq!(entries[4].scripts, ["Hant-HK", "Latn"]);
    assert_eq!(entries[4].licence, "OFL-1.1");

    let short = "# comment\n\nA | Regular | a.ttf | Latn\n";
    let err = parse_manifest(short).unwrap_err();
    assert_eq!(err.line, 3);
    assert!(err.to_string().contains("expected 8 fields"), "{err}");
    let bad_index = "A | Regular | a.ttc#x | Latn | OFL-1.1 | OFL.txt | 00 | src\n";
    assert!(parse_manifest(bad_index).unwrap_err().to_string().contains("face index"));
    let ui = "A UI | Regular | a.ttf | Hant | OFL-1.1 | OFL.txt | 00 | src | ui | future field\nA | Regular | a.ttf | Hant | OFL-1.1 | OFL.txt | 00 | src |\n";
    let entries = parse_manifest(ui).unwrap();
    assert_eq!(entries[0].role.as_deref(), Some("ui"));
    assert!(entries[0].is_ui() && entries[0].is_cjk());
    assert_eq!(entries[1].role, None);
    assert!(!entries[1].is_ui());
    assert!(entries.iter().all(|e| e.role.is_none() || e.is_ui()));
}

#[test]
fn unset_dir_writes_an_empty_table() {
    let r = run(Embed::new(), None, None);
    assert_eq!(r.result.unwrap(), Vec::<String>::new());
    assert_eq!(r.table.trim(), "&[]");
    assert!(r.cargo.contains(&"cargo::rerun-if-env-changed=CRAFT_FONTS_DIR".to_owned()));
    assert!(r.cargo.contains(&"cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED".to_owned()));
    assert!(!r.cargo.iter().any(|l| l.starts_with("cargo::warning")));
    // Only `1` means required.
    for value in ["0", "true", "yes", " 1"] {
        assert!(run(Embed::new(), None, Some(value)).result.is_ok(), "{value:?}");
    }
    assert!(run(Embed::new(), Some(Path::new("")), Some("")).result.is_ok());
}

#[test]
fn required_fails_without_a_usable_checkout() {
    for dir in [None, Some(Path::new(""))] {
        let r = run(Embed::new(), dir, Some("1"));
        assert!(matches!(r.result, Err(Error::NotSet)), "{dir:?}");
        assert_eq!(r.table.trim(), "&[]");
    }
    let err = run(Embed::new(), None, Some("1")).result.unwrap_err().to_string();
    assert!(err.contains("CRAFT_FONTS_DIR") && err.contains("CRAFT_FONTS_REQUIRED"), "{err}");

    let missing = Path::new("/nonexistent/craft-fonts");
    assert!(matches!(run(Embed::new(), Some(missing), Some("1")).result, Err(Error::Read { .. })));
    let r = run(Embed::new(), Some(missing), None);
    assert!(r.result.unwrap().is_empty());
    assert!(r.cargo.iter().any(|l| l.starts_with("cargo::warning=building without craft-fonts")), "{:?}", r.cargo);

    let scratch = Scratch::new("missing-file");
    std::fs::create_dir_all(scratch.0.join("fonts")).unwrap();
    std::fs::write(scratch.0.join("fonts/manifest.txt"), "A | Regular | fonts/a.ttf | Latn | OFL-1.1 | OFL.txt | 00 | src\n").unwrap();
    assert!(matches!(run(Embed::new(), Some(&scratch.0), Some("1")).result, Err(Error::Read { .. })));
    std::fs::write(scratch.0.join("fonts/manifest.txt"), "# no faces\n").unwrap();
    assert!(matches!(run(Embed::new(), Some(&scratch.0), Some("1")).result, Err(Error::NoFaces { .. })));
}

#[test]
fn embeds_the_selected_scripts() {
    let r = run(Embed::new(), Some(&fixture()), Some("1"));
    assert_eq!(r.result.unwrap().len(), 6);

    let r = run(Embed::new().scripts(&["Jpan", "Hant"]), Some(&fixture()), Some("1"));
    assert_eq!(r.result.unwrap(), ["Sans JP Regular", "Sans JP Bold", "Sans CJK TC Regular", "Sans CJK HK Regular"]);
    let manifest = fixture().join("fonts/manifest.txt");
    assert!(r.cargo.contains(&format!("cargo::rerun-if-changed={}", manifest.display())));
    let jp = fixture().join("fonts/sans-jp/SansJP-Regular.ttf").canonicalize().unwrap();
    assert!(r.cargo.contains(&format!("cargo::rerun-if-changed={}", jp.display())));
    assert!(r.table.contains(&format!("include_bytes!({:?})", jp.display().to_string())), "{}", r.table);
    assert!(r.table.contains("family: \"Sans CJK HK\", style: \"Regular\", scripts: &[\"Hant-HK\", \"Latn\"], index: 3"), "{}", r.table);

    let r = run(Embed::new().scripts(&["Hant-HK"]).filter(|f| f.index != 0), Some(&fixture()), None);
    assert_eq!(r.result.unwrap(), ["Sans CJK HK Regular"]);

    let r = run(Embed::new().scripts(&["Arab", "Kore", "Geor"]), Some(&fixture()), Some("1"));
    let Err(Error::MissingScripts { scripts, .. }) = r.result else { panic!("{:?}", r.result) };
    assert_eq!(scripts, ["Kore", "Geor"]);
    // Not required: whatever matched is embedded.
    let r = run(Embed::new().scripts(&["Arab", "Kore"]), Some(&fixture()), None);
    assert_eq!(r.result.unwrap(), ["Sans Arabic Regular"]);
}

#[test]
fn embedded_face_helpers() {
    let face = |style: &'static str| EmbeddedFace { family: "Sans", style, scripts: &["Hant-HK", "Latn"], index: 0, bytes: b"" };
    assert_eq!(face("Regular").name(), "Sans Regular");
    assert!(face("Regular").has_script("Hant") && face("Regular").has_script("Hant-HK") && !face("Regular").has_script("Han"));
    let weights: Vec<u16> = ["Regular", "Bold", "SemiBold", "Extra Light", "Black Italic", "Italic"].map(|s| face(s).weight()).into();
    assert_eq!(weights, [400, 700, 600, 200, 900, 400]);
}

/// A font with only a `cmap` table: a Windows format 4 subtable with `bmp` segments `(first,
/// last, glyphs)`, mapped to glyphs 1.. when `glyphs` is `None` and through the glyph array
/// otherwise, and a format 12 subtable with `groups` `(first, last)` when there are any.
fn tiny_font(bmp: &[(u16, u16, Option<&[u16]>)], groups: &[(u32, u32)]) -> Vec<u8> {
    let be16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_be_bytes());
    let be32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_be_bytes());

    let mut segments = bmp.to_vec();
    segments.push((0xFFFF, 0xFFFF, None));
    let n = u16::try_from(segments.len()).unwrap();
    let (mut ends, mut starts, mut deltas, mut offsets, mut glyphs) = (vec![], vec![], vec![], vec![], vec![]);
    for (i, &(first, last, ids)) in segments.iter().enumerate() {
        be16(&mut ends, last);
        be16(&mut starts, first);
        match ids {
            None => {
                // The last segment maps 0xFFFF to glyph 0, as fonts do.
                be16(&mut deltas, 1u16.wrapping_sub(if first == 0xFFFF { 0 } else { first }));
                be16(&mut offsets, 0);
            }
            Some(ids) => {
                be16(&mut deltas, 0);
                be16(&mut offsets, 2 * (n - u16::try_from(i).unwrap()) + u16::try_from(glyphs.len()).unwrap());
                ids.iter().for_each(|&g| be16(&mut glyphs, g));
            }
        }
    }
    let mut format4 = vec![];
    for x in [4, 0, 0, 2 * n, 0, 0, 0] {
        be16(&mut format4, x);
    }
    format4.extend(ends.iter().chain(&[0, 0]).chain(&starts).chain(&deltas).chain(&offsets).chain(&glyphs));
    let len = u16::try_from(format4.len()).unwrap();
    format4[2..4].copy_from_slice(&len.to_be_bytes());

    let mut format12 = vec![];
    be16(&mut format12, 12);
    be16(&mut format12, 0);
    be32(&mut format12, 16 + 12 * u32::try_from(groups.len()).unwrap());
    be32(&mut format12, 0);
    be32(&mut format12, u32::try_from(groups.len()).unwrap());
    for &(first, last) in groups {
        [first, last, 1].into_iter().for_each(|x| be32(&mut format12, x));
    }

    let subtables: Vec<(u16, Vec<u8>)> = [(1, format4), (10, format12)].into_iter().filter(|(enc, _)| *enc == 1 || !groups.is_empty()).collect();
    let mut cmap = vec![];
    be16(&mut cmap, 0);
    be16(&mut cmap, u16::try_from(subtables.len()).unwrap());
    let mut at = 4 + 8 * u32::try_from(subtables.len()).unwrap();
    for (encoding, table) in &subtables {
        [3, *encoding].into_iter().for_each(|x| be16(&mut cmap, x));
        be32(&mut cmap, at);
        at += u32::try_from(table.len()).unwrap();
    }
    subtables.iter().for_each(|(_, table)| cmap.extend(table));

    let mut font = vec![];
    be32(&mut font, 0x0001_0000);
    for x in [1, 0, 0, 0] {
        be16(&mut font, x);
    }
    font.extend_from_slice(b"cmap");
    for x in [0, 28, u32::try_from(cmap.len()).unwrap()] {
        be32(&mut font, x);
    }
    font.extend(cmap);
    font
}

/// A `.ttc` of fonts made by [`tiny_font`].
fn tiny_collection(fonts: &[Vec<u8>]) -> Vec<u8> {
    let header = 12 + 4 * fonts.len();
    let mut ttc = b"ttcf\0\x01\0\0".to_vec();
    ttc.extend_from_slice(&u32::try_from(fonts.len()).unwrap().to_be_bytes());
    let mut body = vec![];
    for font in fonts {
        let start = u32::try_from(header + body.len()).unwrap();
        ttc.extend_from_slice(&start.to_be_bytes());
        let mut font = font.clone();
        // The table offsets of a collection's faces count from the start of the file.
        let offset = u32::from_be_bytes(font[20..24].try_into().unwrap()) + start;
        font[20..24].copy_from_slice(&offset.to_be_bytes());
        body.extend(font);
    }
    ttc.extend(body);
    ttc
}

#[test]
fn cmap_reads_formats_4_and_12_and_collections() {
    let font = tiny_font(&[(0x41, 0x5A, None), (0x4E00, 0x4E02, Some(&[5, 0, 7]))], &[(0x2_0000, 0x2_0001)]);
    let cmap = Cmap::read(&font, 0).unwrap();
    let covered: Vec<char> = "AZa一丁丂七\u{FFFF}\u{20000}\u{20001}\u{20002}".chars().filter(|&c| cmap.covers(c)).collect();
    assert_eq!(covered, ['A', 'Z', '一', '丂', '\u{20000}', '\u{20001}']);

    let bmp_in_format12 = tiny_font(&[], &[(0x4E00, 0x4E0F)]);
    assert!(Cmap::read(&bmp_in_format12, 0).unwrap().covers('丁'));

    let ttc = tiny_collection(&[tiny_font(&[(0x41, 0x41, None)], &[]), tiny_font(&[(0x4E00, 0x4E00, None)], &[])]);
    let second = Cmap::read(&ttc, 1).unwrap();
    assert!(second.covers('一') && !second.covers('A'));
    assert!(Cmap::read(&ttc, 0).unwrap().covers('A'));
    assert!(Cmap::read(&ttc, 2).is_none());
    assert!(Cmap::read(b"fixture: Sans JP Regular", 0).is_none());
    assert!(Cmap::read(&font[..40], 0).is_none());
}

/// A craft-fonts checkout in `scratch` with these `(family, scripts, role, font)` faces, all
/// Regular.
fn checkout(scratch: &Scratch, faces: &[(&str, &str, &str, Vec<u8>)]) -> PathBuf {
    let dir = scratch.0.join("craft-fonts");
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    let mut manifest = String::new();
    for (i, (family, scripts, role, font)) in faces.iter().enumerate() {
        std::fs::write(dir.join(format!("fonts/{i}.ttf")), font).unwrap();
        manifest += &format!("{family} | Regular | fonts/{i}.ttf | {scripts} | OFL-1.1 | fonts/OFL.txt | 00 | src | {role}\n");
    }
    std::fs::write(dir.join("fonts/manifest.txt"), manifest).unwrap();
    dir
}

#[test]
fn for_ui_embeds_cjk_faces_only_as_ui_subsets() {
    let scratch = Scratch::new("for-ui");
    let latin = || tiny_font(&[(0x20, 0x7E, None)], &[]);
    let dir = checkout(
        &scratch,
        &[
            ("Noto Sans TC", "Hant,Latn", "", latin()),
            ("Noto Sans TC UI", "Hant,Latn", "ui", latin()),
            ("Noto Sans HK", "Hant-HK,Latn", "", latin()),
            ("Noto Sans HK UI", "Hant-HK,Latn", "ui", latin()),
            ("Noto Sans JP", "Jpan,Latn", "", latin()),
            ("Noto Sans Arabic", "Arab", "", latin()),
        ],
    );
    let r = run(Embed::new().for_ui(), Some(&dir), Some("1"));
    assert_eq!(r.result.unwrap(), ["Noto Sans TC UI Regular", "Noto Sans HK UI Regular", "Noto Sans Arabic Regular"]);
    let r = run(Embed::new().for_ui().scripts(&["Hant"]), Some(&dir), Some("1"));
    assert_eq!(r.result.unwrap(), ["Noto Sans TC UI Regular", "Noto Sans HK UI Regular"]);
    let r = run(Embed::new().scripts(&["Hant-HK"]).for_ui().filter(|f| f.style == "Regular"), Some(&dir), Some("1"));
    assert_eq!(r.result.unwrap(), ["Noto Sans HK UI Regular"]);
    // Japanese has only a full face here, which `for_ui` leaves out.
    let r = run(Embed::new().for_ui().scripts(&["Hant", "Jpan"]), Some(&dir), Some("1"));
    let Err(Error::MissingScripts { scripts, .. }) = r.result else { panic!("{:?}", r.result) };
    assert_eq!(scripts, ["Jpan"]);
    assert_eq!(run(Embed::new().scripts(&["Jpan"]), Some(&dir), Some("1")).result.unwrap(), ["Noto Sans JP Regular"]);
}

#[test]
fn check_text_warns_about_characters_no_face_covers() {
    let scratch = Scratch::new("check-text");
    let dir = checkout(
        &scratch,
        &[
            ("Noto Sans TC", "Hant,Latn", "", tiny_font(&[(0x20, 0x7E, None), (0x3000, 0x9FFF, None)], &[])),
            ("Noto Sans TC UI", "Hant,Latn", "ui", tiny_font(&[(0x20, 0x7E, None), (0x4E00, 0x4E03, None)], &[])),
            ("Noto Sans Arabic", "Arab", "", tiny_font(&[(0x600, 0x6FF, None)], &[])),
        ],
    );
    let catalogs = scratch.0.join("i18n");
    std::fs::create_dir_all(&catalogs).unwrap();
    std::fs::write(catalogs.join("zh-TW.ftl"), "\u{FEFF}open = 一丁万。\nclose = 丁万\u{FE0F}\n").unwrap();
    std::fs::write(catalogs.join("ar.ftl"), "open = فتح\nok = 一\n").unwrap();
    let long: String = (0x4E10..0x4E20).filter_map(char::from_u32).collect();
    std::fs::write(catalogs.join("long.ftl"), format!("x = {long}\n")).unwrap();
    let paths = ["zh-TW.ftl", "ar.ftl", "long.ftl", "missing.ftl"].map(|f| catalogs.join(f));
    let warnings = |r: &Run| r.cargo.iter().filter(|l| l.starts_with("cargo::warning")).cloned().collect::<Vec<_>>();

    let r = run(Embed::new().for_ui().check_text(paths.clone()), Some(&dir), None);
    assert_eq!(r.result.as_ref().unwrap().len(), 2);
    for path in &paths {
        assert!(r.cargo.contains(&format!("cargo::rerun-if-changed={}", path.display())), "{:?}", r.cargo);
    }
    let w = warnings(&r);
    assert_eq!(w.len(), 3, "{w:#?}");
    assert!(w[0].contains("zh-TW.ftl has 2 characters no selected face covers (万。); system fonts"), "{}", w[0]);
    assert!(w[0].contains("tools/ui-chars and tools/ui-subset"), "{}", w[0]);
    assert!(w[1].contains("long.ftl has 16 characters") && w[1].contains("(丐丑丒专且丕世丗 …)"), "{}", w[1]);
    assert!(w[2].contains("cannot check") && w[2].contains("missing.ftl"), "{}", w[2]);

    // The full face covers everything; without a checkout there is nothing to check.
    let r = run(Embed::new().check_text(paths[..3].to_vec()), Some(&dir), None);
    assert_eq!(warnings(&r), Vec::<String>::new());
    let r = run(Embed::new().for_ui().check_text(paths.clone()), None, None);
    assert_eq!(warnings(&r), Vec::<String>::new());
    assert!(r.cargo.contains(&format!("cargo::rerun-if-changed={}", paths[0].display())));

    // The fixture's "fonts" are text, so they cover nothing.
    let r = run(Embed::new().scripts(&["Arab"]).check_text([&paths[1]]), Some(&fixture()), None);
    let w = warnings(&r);
    assert!(w[0].contains("cannot read the character map of Sans Arabic Regular"), "{w:#?}");
    assert!(w[1].contains("ar.ftl has 1 character no selected face covers (一)"), "{w:#?}");
}
