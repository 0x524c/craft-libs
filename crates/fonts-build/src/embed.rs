//! The build-script side: read `CRAFT_FONTS_DIR`, select faces, write the generated table.

use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::io;
use std::path::{Path, PathBuf};

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
        let (table, faces) = match selected {
            Ok(faces) => (table(&faces), faces.into_iter().map(|(entry, _)| entry).collect()),
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
    fn select(&self, dir: &Path, cargo: &mut Vec<String>) -> Result<Vec<(ManifestEntry, PathBuf)>, Error> {
        let manifest = dir.join("fonts/manifest.txt");
        cargo.push(format!("cargo::rerun-if-changed={}", manifest.display()));
        let text = std::fs::read_to_string(&manifest).map_err(|source| Error::Read { path: manifest.clone(), source })?;
        let entries = parse_manifest(&text).map_err(|error| Error::Parse { manifest: manifest.clone(), error })?;
        let mut faces = Vec::new();
        for entry in entries {
            let wanted = self.scripts.as_ref().is_none_or(|tags| tags.iter().any(|t| entry.has_script(t)));
            if !wanted || self.filter.as_ref().is_some_and(|keep| !keep(&entry)) {
                continue;
            }
            let file = dir.join(&entry.file);
            // `include_bytes!` resolves relative paths from the including source file.
            let path = file.canonicalize().map_err(|source| Error::Read { path: file, source })?;
            cargo.push(format!("cargo::rerun-if-changed={}", path.display()));
            faces.push((entry, path));
        }
        Ok(faces)
    }

    fn check_required(&self, dir: &Path, faces: Vec<(ManifestEntry, PathBuf)>) -> Result<Vec<(ManifestEntry, PathBuf)>, Error> {
        let manifest = dir.join("fonts/manifest.txt");
        if faces.is_empty() {
            return Err(Error::NoFaces { manifest });
        }
        let missing: Vec<String> = self
            .scripts
            .iter()
            .flatten()
            .filter(|tag| !faces.iter().any(|(entry, _)| entry.scripts.iter().any(|s| script_matches(s, tag))))
            .cloned()
            .collect();
        if missing.is_empty() { Ok(faces) } else { Err(Error::MissingScripts { manifest, scripts: missing }) }
    }
}

/// The environment a build script sees, separated out for tests.
pub(crate) struct Env {
    pub dir: Option<OsString>,
    pub required: Option<OsString>,
    pub out_dir: Option<OsString>,
}

fn table(faces: &[(ManifestEntry, PathBuf)]) -> String {
    let mut src = String::from("// Generated by craft-fonts-build. Do not edit.\n&[\n");
    for (entry, path) in faces {
        let scripts: Vec<String> = entry.scripts.iter().map(|s| format!("{s:?}")).collect();
        let _ = writeln!(
            src,
            "    EmbeddedFace {{ family: {:?}, style: {:?}, scripts: &[{}], index: {}, bytes: include_bytes!({:?}) }},",
            entry.family,
            entry.style,
            scripts.join(", "),
            entry.index,
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
