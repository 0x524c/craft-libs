use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let extra = "A | Regular | a.ttf | Latn | OFL-1.1 | OFL.txt | 00 | src | future field\n";
    assert_eq!(parse_manifest(extra).unwrap().len(), 1);
}

#[test]
fn unset_dir_writes_an_empty_table() {
    let r = run(Embed::new(), None, None);
    assert_eq!(r.result.unwrap(), Vec::<String>::new());
    assert_eq!(r.table.trim(), "&[]");
    assert!(r.cargo.contains(&"cargo::rerun-if-env-changed=CRAFT_FONTS_DIR".to_owned()));
    assert!(r.cargo.contains(&"cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED".to_owned()));
    assert!(!r.cargo.iter().any(|l| l.starts_with("cargo::warning")));
    // `0` and empty are not "required".
    assert!(run(Embed::new(), None, Some("0")).result.is_ok());
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
