//! The build-script side: read `CRAFT_FONTS_DIR`, select faces, write the generated table.

#[cfg(feature = "subset")]
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::io;
use std::path::{Path, PathBuf};

use crate::cmap::Cmap;
use crate::manifest::{ManifestEntry, ParseError, parse_manifest, script_matches};

const DIR_VAR: &str = "CRAFT_FONTS_DIR";
const REQUIRED_VAR: &str = "CRAFT_FONTS_REQUIRED";
const EMPTY_TABLE: &str = "&[]\n";

/// Embed every face in `CRAFT_FONTS_DIR`'s manifest into `out_file` (relative paths are under
/// `OUT_DIR`). Shorthand for `Embed::new().write(out_file)`.
pub fn embed(out_file: impl AsRef<Path>) -> Result<Vec<ManifestEntry>, Error> {
    Embed::new().write(out_file)
}

/// Whether the build script is building for wasm32 (web builds usually embed fewer faces).
pub fn target_is_wasm() -> bool {
    std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32")
}

type Filter = Box<dyn Fn(&ManifestEntry) -> bool>;

/// Which faces to embed, and from where. [`Embed::write`] does the work.
#[derive(Default)]
pub struct Embed {
    scripts: Option<Vec<String>>,
    filter: Option<Filter>,
    dir: Option<PathBuf>,
    required: Option<bool>,
    ui: bool,
    catalogs: Option<Vec<PathBuf>>,
    texts: Vec<PathBuf>,
}

/// A selected face and the file the table embeds it from.
struct Face {
    entry: ManifestEntry,
    path: PathBuf,
    index: u32,
    /// An interface face to cut to the catalogs' characters before it is embedded.
    #[cfg_attr(not(feature = "subset"), expect(dead_code))]
    cut: bool,
}

impl Embed {
    /// Every face, from `CRAFT_FONTS_DIR`, required when `CRAFT_FONTS_REQUIRED=1`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Embed only faces with at least one of these script tags (see
    /// [`ManifestEntry::has_script`]). With `CRAFT_FONTS_REQUIRED=1`, each tag must match a face.
    pub fn scripts(mut self, tags: &[&str]) -> Self {
        self.scripts = Some(tags.iter().map(|t| (*t).to_owned()).collect());
        self
    }

    /// Embed only faces for which `keep` returns true (applied after [`Embed::scripts`]).
    pub fn filter(mut self, keep: impl Fn(&ManifestEntry) -> bool + 'static) -> Self {
        self.filter = Some(Box::new(keep));
        self
    }

    /// Embed faces for an app's interface: CJK faces only as the interface faces (`role` `ui` in
    /// the manifest), cut to the characters of the catalogs given to `Embed::subset_to`. Other
    /// CJK faces are dropped and the rest are kept. Without catalogs the interface faces are left
    /// out too, with a `cargo::warning`, so a full CJK face is never embedded for the interface.
    /// Combines with [`Embed::scripts`] and [`Embed::filter`]; with `CRAFT_FONTS_REQUIRED=1`, an
    /// interface face satisfies its script tags.
    pub fn for_ui(mut self) -> Self {
        self.ui = true;
        self
    }

    /// Cut the interface faces to the characters these translation catalogs use, and embed the
    /// cut faces, written to `OUT_DIR`. Implies [`Embed::for_ui`].
    ///
    /// The cut keeps the catalogs' characters from U+2E80 up (CJK and later blocks, without emoji,
    /// variation selectors and tags), plus CJK punctuation (U+3000–303F), kana (U+3040–30FF,
    /// U+31F0–31FF) and full-width forms (U+FF00–FFEF), with every layout feature. The same inputs
    /// give the same bytes. A face with CFF outlines is an [`Error::NotTrueType`].
    ///
    /// The catalogs are also checked as by [`Embed::check_text`]. Relative paths are under the
    /// package directory; an unreadable catalog is a `cargo::warning`.
    #[cfg(feature = "subset")]
    pub fn subset_to<P: Into<PathBuf>>(mut self, catalogs: impl IntoIterator<Item = P>) -> Self {
        self.ui = true;
        self.catalogs.get_or_insert_default().extend(catalogs.into_iter().map(Into::into));
        self
    }

    /// Check that the embedded faces cover these text files, such as translation catalogs: one
    /// `cargo::warning` per file that has characters from U+2E80 up (CJK and later blocks, without
    /// emoji and variation selectors) that no embedded face maps. Those characters fall back to
    /// system fonts at run time. Relative paths are under the package directory. The check runs
    /// only when a checkout was read.
    pub fn check_text<P: Into<PathBuf>>(mut self, paths: impl IntoIterator<Item = P>) -> Self {
        self.texts.extend(paths.into_iter().map(Into::into));
        self
    }

    /// Read this checkout instead of `CRAFT_FONTS_DIR`.
    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Override `CRAFT_FONTS_REQUIRED` (required only when it is exactly `1`).
    pub fn required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }

    /// Write the face table to `out_file` and print the `cargo::` lines; returns the faces
    /// embedded.
    ///
    /// Without a checkout the table is empty. A checkout that can't be read is a
    /// `cargo::warning` and an empty table, or an [`Error`] when required. The table is written
    /// in every case, so on an error the build fails only on the message the caller prints.
    pub fn write(&self, out_file: impl AsRef<Path>) -> Result<Vec<ManifestEntry>, Error> {
        let env = Env { dir: std::env::var_os(DIR_VAR), required: std::env::var_os(REQUIRED_VAR), out_dir: std::env::var_os("OUT_DIR") };
        let mut cargo = Vec::new();
        let result = self.run(&env, out_file.as_ref(), &mut cargo);
        for line in cargo {
            println!("{line}");
        }
        result
    }

    pub(crate) fn run(&self, env: &Env, out_file: &Path, cargo: &mut Vec<String>) -> Result<Vec<ManifestEntry>, Error> {
        cargo.push(format!("cargo::rerun-if-env-changed={DIR_VAR}"));
        cargo.push(format!("cargo::rerun-if-env-changed={REQUIRED_VAR}"));
        for text in self.checked_files() {
            cargo.push(format!("cargo::rerun-if-changed={}", text.display()));
        }
        let out = match (out_file.is_absolute(), &env.out_dir) {
            (true, _) => out_file.to_path_buf(),
            (false, Some(out_dir)) => Path::new(out_dir).join(out_file),
            (false, None) => return Err(Error::NoOutDir),
        };
        let required = self.required.unwrap_or_else(|| env.required.as_ref().is_some_and(|v| v == "1"));
        let dir = self.dir.clone().or_else(|| env.dir.clone().filter(|d| !d.is_empty()).map(PathBuf::from));

        let selected = match &dir {
            None => Err(Error::NotSet),
            Some(dir) => self.select(dir, cargo).and_then(|faces| if required { self.check_required(dir, faces) } else { Ok(faces) }),
        };
        let mut selected = selected;
        if let Ok(faces) = &mut selected {
            // A face that can't be cut is a broken manifest line: an error even when not required.
            if let Err(e) = self.cut(faces, out.parent().unwrap_or(Path::new(""))) {
                write(&out, EMPTY_TABLE)?;
                return Err(e);
            }
            self.check_coverage(faces, cargo);
        }
        let (table, faces) = match selected {
            Ok(faces) => (table(&faces), faces.into_iter().map(|face| face.entry).collect()),
            Err(e) if required => {
                write(&out, EMPTY_TABLE)?;
                return Err(e);
            }
            Err(Error::NotSet) => (EMPTY_TABLE.to_owned(), Vec::new()),
            Err(e) => {
                cargo.push(format!("cargo::warning=building without craft-fonts: {e}"));
                (EMPTY_TABLE.to_owned(), Vec::new())
            }
        };
        write(&out, &table)?;
        Ok(faces)
    }

    /// The wanted manifest entries with their canonical file paths.
    fn select(&self, dir: &Path, cargo: &mut Vec<String>) -> Result<Vec<Face>, Error> {
        let manifest = dir.join("fonts/manifest.txt");
        cargo.push(format!("cargo::rerun-if-changed={}", manifest.display()));
        let text = std::fs::read_to_string(&manifest).map_err(|source| Error::Read { path: manifest.clone(), source })?;
        let entries = parse_manifest(&text).map_err(|error| Error::Parse { manifest: manifest.clone(), error })?;
        let mut faces = Vec::new();
        let mut uncut = Vec::new();
        for entry in entries {
            let wanted = self.scripts.as_ref().is_none_or(|tags| tags.iter().any(|t| entry.has_script(t)));
            let ui_dropped = self.ui && entry.is_cjk() && !entry.is_ui();
            if !wanted || ui_dropped || self.filter.as_ref().is_some_and(|keep| !keep(&entry)) {
                continue;
            }
            let cut = self.ui && entry.is_ui();
            if cut && self.catalogs.is_none() {
                uncut.push(format!("{} {}", entry.family, entry.style));
                continue;
            }
            let file = dir.join(&entry.file);
            // `include_bytes!` resolves relative paths from the including source file.
            let path = file.canonicalize().map_err(|source| Error::Read { path: file, source })?;
            cargo.push(format!("cargo::rerun-if-changed={}", path.display()));
            let index = entry.index;
            faces.push(Face { entry, path, index, cut });
        }
        if !uncut.is_empty() {
            cargo.push(format!(
                "cargo::warning=craft-fonts: not embedding the interface faces {}: pass the translation catalogs to \
                 Embed::subset_to (feature `subset`) to embed them cut to the catalogs' characters",
                uncut.join(", ")
            ));
        }
        Ok(faces)
    }

    /// Replace each interface face with its cut, written to `out_dir`.
    #[cfg(feature = "subset")]
    fn cut(&self, faces: &mut [Face], out_dir: &Path) -> Result<(), Error> {
        let Some(catalogs) = &self.catalogs else { return Ok(()) };
        if !faces.iter().any(|face| face.cut) {
            return Ok(());
        }
        let mut chars = BTreeSet::new();
        // An unreadable catalog adds nothing here; `check_coverage` reports it.
        for text in catalogs.iter().filter_map(|path| std::fs::read_to_string(path).ok()) {
            chars.extend(text.chars().filter(|&c| needs_glyph(c)));
        }
        for (start, end) in UI_RANGES {
            chars.extend(start..=end);
        }
        let dir = out_dir.join("craft-fonts-ui");
        std::fs::create_dir_all(&dir).map_err(|source| Error::Write { path: dir.clone(), source })?;
        for face in faces.iter_mut().filter(|face| face.cut) {
            let source = std::fs::read(&face.path).map_err(|source| Error::Read { path: face.path.clone(), source })?;
            let bytes = crate::subset::subset(&source, face.index, &chars).map_err(|failure| match failure {
                crate::subset::Failure::NotTrueType => Error::NotTrueType { path: face.path.clone() },
                crate::subset::Failure::Other(reason) => Error::Subset { path: face.path.clone(), reason },
            })?;
            let name: String =
                format!("{}-{}", face.entry.family, face.entry.style).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            let out = dir.join(format!("{name}.ttf"));
            // Rewriting identical bytes would make Cargo rebuild the crate that embeds them.
            if std::fs::read(&out).ok().as_deref() != Some(&bytes[..]) {
                std::fs::write(&out, &bytes).map_err(|source| Error::Write { path: out.clone(), source })?;
            }
            face.path = out;
            face.index = 0;
        }
        Ok(())
    }

    /// Without the subsetter there are no catalogs, so [`Embed::select`] left out every face to cut.
    #[cfg(not(feature = "subset"))]
    fn cut(&self, _faces: &mut [Face], _out_dir: &Path) -> Result<(), Error> {
        Ok(())
    }

    /// The catalogs, then the other [`Embed::check_text`] files.
    fn checked_files(&self) -> Vec<&PathBuf> {
        let mut files: Vec<&PathBuf> = self.catalogs.iter().flatten().collect();
        for text in &self.texts {
            if !files.contains(&text) {
                files.push(text);
            }
        }
        files
    }

    /// Warn about each catalog's and [`Embed::check_text`] file's characters that no embedded face
    /// covers.
    fn check_coverage(&self, faces: &[Face], cargo: &mut Vec<String>) {
        let files = self.checked_files();
        if files.is_empty() {
            return;
        }
        let fonts: Vec<(&Face, Option<Vec<u8>>)> = faces.iter().map(|face| (face, std::fs::read(&face.path).ok())).collect();
        let mut cmaps = Vec::new();
        for (Face { entry, index, .. }, bytes) in &fonts {
            match bytes.as_deref().and_then(|bytes| Cmap::read(bytes, *index)) {
                Some(cmap) => cmaps.push(cmap),
                None => cargo
                    .push(format!("cargo::warning=craft-fonts: cannot read the character map of {} {} ({})", entry.family, entry.style, entry.file)),
            }
        }
        for path in files {
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(e) => {
                    cargo.push(format!("cargo::warning=craft-fonts: cannot check {}: {e}", path.display()));
                    continue;
                }
            };
            let mut seen = HashSet::new();
            let missing: Vec<char> = text.chars().filter(|&c| needs_glyph(c) && seen.insert(c) && !cmaps.iter().any(|cmap| cmap.covers(c))).collect();
            if missing.is_empty() {
                continue;
            }
            const SHOWN: usize = 8;
            let shown: String = missing.iter().take(SHOWN).collect();
            let more = if missing.len() > SHOWN { " …" } else { "" };
            cargo.push(format!(
                "cargo::warning=craft-fonts: {} has {} character{} no embedded face covers ({shown}{more}); system fonts will draw them.",
                path.display(),
                missing.len(),
                if missing.len() == 1 { "" } else { "s" },
            ));
        }
    }

    fn check_required(&self, dir: &Path, faces: Vec<Face>) -> Result<Vec<Face>, Error> {
        let manifest = dir.join("fonts/manifest.txt");
        if faces.is_empty() {
            return Err(Error::NoFaces { manifest });
        }
        let missing: Vec<String> = self
            .scripts
            .iter()
            .flatten()
            .filter(|tag| !faces.iter().any(|face| face.entry.scripts.iter().any(|s| script_matches(s, tag))))
            .cloned()
            .collect();
        if missing.is_empty() { Ok(faces) } else { Err(Error::MissingScripts { manifest, scripts: missing }) }
    }
}

/// Whether the interface faces need `c`: CJK and later blocks, except emoji and other pictographs
/// (the emoji faces draw them), variation selectors, tags and a byte order mark.
fn needs_glyph(c: char) -> bool {
    c >= '\u{2E80}' && !matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{1F000}'..='\u{1FFFF}' | '\u{E0000}'..)
}

/// Kept in every cut, whatever the catalogs use: CJK punctuation, kana and full-width forms.
#[cfg(feature = "subset")]
const UI_RANGES: [(char, char); 4] = [('\u{3000}', '\u{303F}'), ('\u{3040}', '\u{30FF}'), ('\u{31F0}', '\u{31FF}'), ('\u{FF00}', '\u{FFEF}')];

/// The environment a build script sees, separated out for tests.
pub(crate) struct Env {
    pub dir: Option<OsString>,
    pub required: Option<OsString>,
    pub out_dir: Option<OsString>,
}

fn table(faces: &[Face]) -> String {
    let mut src = String::from("// Generated by craft-fonts-build. Do not edit.\n&[\n");
    for Face { entry, path, index, .. } in faces {
        let scripts: Vec<String> = entry.scripts.iter().map(|s| format!("{s:?}")).collect();
        let _ = writeln!(
            src,
            "    EmbeddedFace {{ family: {:?}, style: {:?}, scripts: &[{}], index: {}, bytes: include_bytes!({:?}) }},",
            entry.family,
            entry.style,
            scripts.join(", "),
            index,
            path.display().to_string(),
        );
    }
    src.push_str("]\n");
    src
}

fn write(path: &Path, contents: &str) -> Result<(), Error> {
    std::fs::write(path, contents).map_err(|source| Error::Write { path: path.to_path_buf(), source })
}

/// Why the faces could not be embedded. Print it as `cargo::error={error}`.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `CRAFT_FONTS_REQUIRED=1` but `CRAFT_FONTS_DIR` is unset or empty.
    NotSet,
    /// The manifest or a font file it lists can't be read.
    Read { path: PathBuf, source: io::Error },
    /// The manifest has a malformed line.
    Parse { manifest: PathBuf, error: ParseError },
    /// Required, but no face in the manifest was selected.
    NoFaces { manifest: PathBuf },
    /// Required, but no face in the manifest has these requested script tags.
    MissingScripts { manifest: PathBuf, scripts: Vec<String> },
    /// The generated table can't be written.
    Write { path: PathBuf, source: io::Error },
    /// `out_file` is relative and `OUT_DIR` is unset (not running as a build script).
    NoOutDir,
    /// An interface face has CFF outlines, which can't be cut correctly; it must be TrueType.
    NotTrueType { path: PathBuf },
    /// An interface face can't be cut.
    Subset { path: PathBuf, reason: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSet => {
                write!(f, "{REQUIRED_VAR}=1 but {DIR_VAR} is not: set {DIR_VAR} to the absolute path of a storytold/craft-fonts checkout")
            }
            Self::Read { path, source } => {
                write!(f, "cannot read {} ({DIR_VAR} must be the absolute path of a storytold/craft-fonts checkout): {source}", path.display())
            }
            Self::Parse { manifest, error } => write!(f, "{}: {error}", manifest.display()),
            Self::NoFaces { manifest } => write!(f, "{REQUIRED_VAR}=1 but no face in {} was selected", manifest.display()),
            Self::MissingScripts { manifest, scripts } => {
                write!(f, "{REQUIRED_VAR}=1 but {} has no face for {}", manifest.display(), scripts.join(", "))
            }
            Self::Write { path, source } => write!(f, "cannot write {}: {source}", path.display()),
            Self::NoOutDir => write!(f, "OUT_DIR is not set: call craft-fonts-build from a build script, or pass an absolute out_file"),
            Self::NotTrueType { path } => write!(
                f,
                "{} has CFF outlines, which craft-fonts-build can't subset correctly: an interface face (role ui) must be a TrueType (glyf) font",
                path.display()
            ),
            Self::Subset { path, reason } => write!(f, "cannot subset {}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { error, .. } => Some(error),
            _ => None,
        }
    }
}
