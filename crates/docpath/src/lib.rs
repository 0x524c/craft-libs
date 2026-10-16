#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

//! Decide whether a file path or file URL stored in a document may be opened, read or written.
//!
//! Paths stored in documents (linked images, templates, data sources, media) are only followed when
//! they refer to a local file, or to a file on the same network share the document itself was opened
//! from. Paths to other computers, device paths and non-file URLs are refused, so opening a document
//! never makes the app contact a server or device the user did not choose.
//!
//! The check is lexical: it does no I/O and gives the same answer on every platform. Windows path
//! forms (`C:\`, `\\server\share`, `\\?\`, `\\.\`, reserved names such as `NUL`) are recognised on
//! all platforms, and both `/` and `\` count as separators.
//!
//! - [`classify`] sorts a stored path into a [`PathKind`].
//! - [`is_local`] is `classify(raw) == PathKind::Local`.
//! - [`resolve`] turns a stored path into the path to open, relative to the document, or a [`Refused`]
//!   that says why it may not be opened.
//!
//! ```
//! use std::path::Path;
//! use craft_docpath::{resolve, Refused};
//!
//! let doc = Path::new(r"\\server\share\docs\report.doc");
//! assert!(resolve(r"img\logo.png", Some(doc)).is_ok());
//! assert!(resolve(r"\\server\share\img\logo.png", Some(doc)).is_ok());
//! assert_eq!(resolve(r"\\other\share\logo.png", Some(doc)), Err(Refused::Network));
//! ```

use std::fmt;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

/// What a stored path refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathKind {
    /// A file on this computer: absolute, drive-relative, root-relative or relative.
    Local,
    /// A file on another computer: `\\host\share`, `//host/share`, `\\?\UNC\…`, `file://host/…`.
    Network,
    /// A device or object-manager path: `\\.\…`, `\??\…`, `\\?\GLOBALROOT\…`, or a reserved name such as `NUL` or `COM1`.
    Device,
    /// A URL with a scheme other than `file:`, such as `https:` or `ms-word:`.
    Url,
    /// Not a usable path: empty, containing control characters, or climbing above its root with `..`.
    Invalid,
}

/// Why [`resolve`] refused a stored path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refused {
    /// The path is on another computer, or on a network share other than the document's.
    Network,
    /// The path names a device rather than a file.
    Device,
    /// The link is a URL, not a file path.
    Url,
    /// The path is not usable.
    Invalid,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Network => "the path is on another computer",
            Self::Device => "the path names a device, not a file",
            Self::Url => "the link is not a file path",
            Self::Invalid => "the path is not valid",
        })
    }
}

impl std::error::Error for Refused {}

impl From<Refused> for PathKind {
    fn from(r: Refused) -> Self {
        match r {
            Refused::Network => Self::Network,
            Refused::Device => Self::Device,
            Refused::Url => Self::Url,
            Refused::Invalid => Self::Invalid,
        }
    }
}

/// Classify a path or URL stored in a document, without a document to resolve it against.
///
/// Relative paths are [`PathKind::Local`]; a rooted path whose `..` components climb above its
/// drive, share or `/` is [`PathKind::Invalid`].
pub fn classify(raw: &str) -> PathKind {
    match resolve(raw, None) {
        Ok(_) => PathKind::Local,
        Err(r) => r.into(),
    }
}

/// Whether a path or URL stored in a document refers to a local file (see [`classify`]).
pub fn is_local(raw: &str) -> bool {
    classify(raw) == PathKind::Local
}

/// Turn a path or URL stored in a document into the path to open, or refuse it.
///
/// `document` is the path of the document the link came from, if it has one. The steps:
///
/// 1. A `file:` URL is percent-decoded and its scheme removed. Only an empty host or `localhost`
///    is local; `file://host/…` is refused as [`Refused::Network`].
/// 2. A relative path is joined to the document's folder, a root-relative path (`\x`) to the
///    document's drive or share, and a drive-relative path (`C:x`) to the document's folder when the
///    document is on that drive. Without a document they are returned unresolved.
/// 3. `.` and `..` are folded lexically; climbing above a drive, share or `/` is [`Refused::Invalid`].
/// 4. A network path is allowed only when it is on the document's own share (same `\\server\share`,
///    compared ignoring ASCII case). Other network paths, device paths and reserved device names
///    are refused.
///
/// Windows-form results (drive, share, `\\?\`) use `\` as the separator on every platform; others
/// use the platform separator.
pub fn resolve(raw: &str, document: Option<&Path>) -> Result<PathBuf, Refused> {
    let plain = strip_url(raw)?;
    let link = parse(&plain, true)?;

    let doc_str = document.and_then(Path::to_str);
    if let (Some(doc), None, Root::Relative) = (document, doc_str, &link.root) {
        return join_native(doc, &link.comps);
    }
    let doc = doc_str.map(|d| parse(d, false));
    let doc_share = match &doc {
        Some(Ok(Parsed { root: Root::Share { host, share, .. }, .. })) => Some((host.clone(), share.clone())),
        _ => None,
    };

    let (root, comps) = match (link.root, doc) {
        (Root::Relative, Some(doc)) => {
            let doc = doc?;
            (doc.root, folder(doc.comps).chain(link.comps).collect())
        }
        (Root::Rooted, Some(Ok(doc))) if doc.root.is_absolute() => (doc.root, link.comps),
        (Root::DriveRelative(l), Some(Ok(doc))) if doc.root.drive().is_some_and(|d| d.eq_ignore_ascii_case(&l)) => {
            (doc.root, folder(doc.comps).chain(link.comps).collect())
        }
        (root, _) => (root, link.comps),
    };
    let comps = normalize(&root, comps)?;

    if let Root::Share { host, share, .. } = &root {
        let same = doc_share.is_some_and(|(h, s)| h.eq_ignore_ascii_case(host) && s.eq_ignore_ascii_case(share));
        if !same {
            return Err(Refused::Network);
        }
    }
    Ok(render(&root, &comps))
}

#[derive(Debug)]
enum Root {
    /// `a\b`
    Relative,
    /// `\a` or `/a`
    Rooted,
    /// `C:a`
    DriveRelative(char),
    /// `C:\a`
    Drive(char),
    /// `\\?\C:\a`
    VerbatimDrive(char),
    /// `\\host\share\a`, or `\\?\UNC\host\share\a` when `verbatim`
    Share { host: String, share: String, verbatim: bool },
}

impl Root {
    fn is_absolute(&self) -> bool {
        !matches!(self, Self::Relative | Self::DriveRelative(_))
    }

    fn drive(&self) -> Option<char> {
        match self {
            Self::Drive(l) | Self::VerbatimDrive(l) => Some(*l),
            _ => None,
        }
    }
}

struct Parsed {
    root: Root,
    comps: Vec<String>,
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// The document's folder: its components without the file name.
fn folder(mut comps: Vec<String>) -> impl Iterator<Item = String> {
    comps.pop();
    comps.into_iter()
}

/// Reject control characters and non-file URLs; reduce a `file:` URL to its path.
fn strip_url(raw: &str) -> Result<String, Refused> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return Err(Refused::Invalid);
    }
    let path = match raw.get(..5) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file:") => file_url_path(raw.get(5..).unwrap_or_default())?,
        _ => raw.to_string(),
    };
    if has_scheme(&path) {
        // `NUL:` and `COM1:` name devices, not URL schemes.
        let device = path.split_once(':').is_some_and(|(scheme, _)| is_dos_device(scheme));
        return Err(if device { Refused::Device } else { Refused::Url });
    }
    Ok(path)
}

/// `scheme:` with a scheme of two or more characters, so a drive letter (`C:`) is not one.
fn has_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else { return false };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The path of a `file:` URL (the part after `file:`).
fn file_url_path(rest: &str) -> Result<String, Refused> {
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let decoded = percent_decode(rest)?;
    if decoded.is_empty() || decoded.chars().any(char::is_control) {
        return Err(Refused::Invalid);
    }
    let mut chars = decoded.chars();
    let path = match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if is_sep(a) && is_sep(b) => {
            let after = chars.as_str();
            let (host, path) = match after.find(is_sep) {
                Some(i) => after.split_at(i),
                None => (after, ""),
            };
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
                return Err(Refused::Network);
            }
            path
        }
        _ => decoded.as_str(),
    };
    // `/C:/x` and `/C|/x` name the drive path `C:/x`.
    let mut chars = path.chars();
    if let (Some(s), Some(l), Some(':' | '|')) = (chars.next(), chars.next(), chars.next())
        && is_sep(s)
        && l.is_ascii_alphabetic()
        && chars.clone().next().is_none_or(is_sep)
    {
        return Ok(format!("{l}:{}", chars.as_str()));
    }
    Ok(path.to_string())
}

fn percent_decode(s: &str) -> Result<String, Refused> {
    let hex = |c: Option<u8>| c.and_then(|c| (c as char).to_digit(16)).and_then(|d| u8::try_from(d).ok());
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        if c == b'%'
            && let (Some(h), Some(l)) = (hex(bytes.get(i + 1).copied()), hex(bytes.get(i + 2).copied()))
        {
            out.push((h << 4) | l);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Refused::Invalid)
}

/// Split a plain path into its root and components. `check` applies the component rules
/// (reserved device names, dot-and-space names); the document's own path is not checked.
fn parse(s: &str, check: bool) -> Result<Parsed, Refused> {
    let mut chars = s.chars();
    let first = chars.next();
    let second = chars.next();
    let rest = chars.as_str();
    match (first, second) {
        (Some(a), Some(b)) if is_sep(a) && is_sep(b) => parse_double(s, check),
        (Some(a), _) if is_sep(a) => {
            let after = s.get(1..).unwrap_or_default();
            if let Some(r) = after.strip_prefix("??")
                && r.chars().next().is_none_or(is_sep)
            {
                return Err(Refused::Device);
            }
            Ok(Parsed { root: Root::Rooted, comps: comps(after, check)? })
        }
        (Some(l), Some(':')) if l.is_ascii_alphabetic() => {
            let mut r = rest.chars();
            match r.next() {
                Some(c) if is_sep(c) => Ok(Parsed { root: Root::Drive(l), comps: comps(r.as_str(), check)? }),
                _ => Ok(Parsed { root: Root::DriveRelative(l), comps: comps(rest, check)? }),
            }
        }
        _ => Ok(Parsed { root: Root::Relative, comps: comps(s, check)? }),
    }
}

/// A path starting with two separators: a share, or a `\\?\`, `\\.\` or `//?/` prefix.
fn parse_double(s: &str, check: bool) -> Result<Parsed, Refused> {
    let t = s.get(2..).unwrap_or_default();
    let (first, after) = t.split_once(is_sep).unwrap_or((t, ""));
    if first != "?" && first != "." {
        return parse_share(t, false, check);
    }
    if let Some(v) = s.strip_prefix(r"\\?\") {
        return parse_verbatim(v, check);
    }
    // `\\.\`, and `\\?\` spelled with any `/`, which Windows normalises into the device namespace.
    let (next, rest) = after.split_once(is_sep).unwrap_or((after, ""));
    if next.eq_ignore_ascii_case("UNC") {
        return parse_share(rest, false, check);
    }
    Err(Refused::Device)
}

/// `\\?\` paths: only `\`, and no normalisation, so `.`, `..` and `/` are not usable in them.
fn parse_verbatim(v: &str, check: bool) -> Result<Parsed, Refused> {
    let (first, rest) = match v.split_once('\\') {
        Some((f, r)) => (f, Some(r)),
        None => (v, None),
    };
    let mut drive = first.chars();
    match (drive.next(), drive.next(), drive.next(), rest) {
        // `\\?\C:` without a separator is the volume, not its root folder.
        (Some(l), Some(':'), None, Some(rest)) if l.is_ascii_alphabetic() => {
            Ok(Parsed { root: Root::VerbatimDrive(l), comps: verbatim_comps(rest, check)? })
        }
        _ if first.eq_ignore_ascii_case("UNC") && rest.is_some() => parse_share(rest.unwrap_or_default(), true, check),
        _ => Err(Refused::Device),
    }
}

/// `host\share\rest`, the part after the leading separators (or `\\?\UNC\`).
fn parse_share(t: &str, verbatim: bool, check: bool) -> Result<Parsed, Refused> {
    let split = |s: &str| -> (String, String) {
        let pair = if verbatim { s.split_once('\\') } else { s.split_once(is_sep) };
        let (a, b) = pair.unwrap_or((s, ""));
        (a.to_string(), b.to_string())
    };
    let (host, rest) = split(t);
    let (share, rest) = split(&rest);
    if host.is_empty() || share.is_empty() {
        return Err(Refused::Network);
    }
    let comps = if verbatim { verbatim_comps(&rest, check)? } else { comps(&rest, check)? };
    Ok(Parsed { root: Root::Share { host, share, verbatim }, comps })
}

fn comps(s: &str, check: bool) -> Result<Vec<String>, Refused> {
    s.split(is_sep)
        .filter(|c| !c.is_empty())
        .map(|c| {
            if check {
                if is_dos_device(c) {
                    return Err(Refused::Device);
                }
                // Windows trims trailing dots and spaces, so `.. ` or `...` could act as `..`.
                if c != "." && c != ".." && c.chars().all(|ch| ch == '.' || ch == ' ') {
                    return Err(Refused::Invalid);
                }
            }
            Ok(c.to_string())
        })
        .collect()
}

fn verbatim_comps(s: &str, check: bool) -> Result<Vec<String>, Refused> {
    let parts: Vec<&str> = s.split('\\').collect();
    let last = parts.len().saturating_sub(1);
    let mut out = Vec::with_capacity(parts.len());
    for (i, c) in parts.into_iter().enumerate() {
        if c.is_empty() && i == last {
            continue;
        }
        if c.is_empty() || c == "." || c == ".." || c.contains('/') {
            return Err(Refused::Invalid);
        }
        if check && is_dos_device(c) {
            return Err(Refused::Device);
        }
        out.push(c.to_string());
    }
    Ok(out)
}

/// A reserved DOS device name, with or without an extension, stream or trailing dots and spaces.
fn is_dos_device(comp: &str) -> bool {
    let stem = comp.split(['.', ':']).next().unwrap_or_default().trim_end_matches(' ');
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].iter().any(|n| stem.eq_ignore_ascii_case(n)) {
        return true;
    }
    let Some(head) = stem.get(..3) else { return false };
    let mut tail = stem.get(3..).unwrap_or_default().chars();
    (head.eq_ignore_ascii_case("COM") || head.eq_ignore_ascii_case("LPT"))
        && matches!((tail.next(), tail.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
}

/// Fold `.` and `..`. A rooted path may not climb above its root; a relative one keeps leading `..`.
fn normalize(root: &Root, comps: Vec<String>) -> Result<Vec<String>, Refused> {
    let mut out: Vec<String> = Vec::with_capacity(comps.len());
    for c in comps {
        match c.as_str() {
            "." => {}
            ".." => match out.last() {
                Some(last) if last != ".." => {
                    out.pop();
                }
                _ if root.is_absolute() => return Err(Refused::Invalid),
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    Ok(out)
}

fn render(root: &Root, comps: &[String]) -> PathBuf {
    let (prefix, sep) = match root {
        Root::Relative if comps.is_empty() => (".".to_string(), MAIN_SEPARATOR),
        Root::Relative => (String::new(), MAIN_SEPARATOR),
        Root::Rooted => (MAIN_SEPARATOR.to_string(), MAIN_SEPARATOR),
        Root::DriveRelative(l) => (format!("{l}:"), '\\'),
        Root::Drive(l) => (format!("{l}:\\"), '\\'),
        Root::VerbatimDrive(l) => (format!(r"\\?\{l}:\"), '\\'),
        Root::Share { host, share, verbatim: false } => (format!(r"\\{host}\{share}\"), '\\'),
        Root::Share { host, share, verbatim: true } => (format!(r"\\?\UNC\{host}\{share}\"), '\\'),
    };
    PathBuf::from(prefix + &comps.join(&sep.to_string()))
}

/// A relative link next to a document whose path is not UTF-8 (possible on Unix only).
fn join_native(doc: &Path, comps: &[String]) -> Result<PathBuf, Refused> {
    let mut p = doc.parent().map(Path::to_path_buf).unwrap_or_default();
    for c in comps {
        match c.as_str() {
            "." => {}
            ".." => {
                if !p.pop() {
                    return Err(Refused::Invalid);
                }
            }
            _ => p.push(c),
        }
    }
    Ok(p)
}
