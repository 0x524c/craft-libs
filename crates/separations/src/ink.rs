//! The ink model: which inks a job prints with, and colours as tints of those inks.

use std::borrow::Cow;

/// A process ink, in plate order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Process {
    Cyan,
    Magenta,
    Yellow,
    Black,
}

impl Process {
    /// The four process inks, in plate order.
    pub const ALL: [Process; 4] = [Process::Cyan, Process::Magenta, Process::Yellow, Process::Black];

    /// The colorant name a `Separation` or `DeviceN` colour space gives this ink.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cyan => "Cyan",
            Self::Magenta => "Magenta",
            Self::Yellow => "Yellow",
            Self::Black => "Black",
        }
    }

    /// The process ink a colorant name denotes (ISO 32000-2 §8.6.6.4).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The ink's position in a CMYK colour.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How a spot ink looks, for previews and for printing it as process colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alternate {
    /// CMYK components, `0.0..=1.0`.
    Cmyk([f32; 4]),
    /// CIE L\*a\*b\* relative to D50.
    Lab([f32; 3]),
}

/// A spot ink.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    /// The colorant name, as a `Separation` colour space would name it, without PDF name
    /// escapes (see [`normalize_name`]).
    pub name: String,
    /// The ink at 100%.
    pub alternate: Alternate,
    /// How opaque the ink is (`0.0..=1.0`, as InDesign's Ink Manager states it), when known.
    pub solidity: Option<f32>,
}

/// A process or spot ink.
#[derive(Clone, Debug, PartialEq)]
pub enum Ink {
    Process(Process),
    Spot(Spot),
}

impl Ink {
    /// The colorant name: `Cyan` … `Black`, or the spot ink's name.
    pub fn name(&self) -> &str {
        match self {
            Self::Process(p) => p.name(),
            Self::Spot(s) => &s.name,
        }
    }

    pub fn is_spot(&self) -> bool {
        matches!(self, Self::Spot(_))
    }
}

/// An ink's index in its [`InkSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InkId(u16);

impl InkId {
    /// The ink's position in [`InkSet::iter`] order, which is also its plane in
    /// [`Plates::planes`](crate::Plates::planes).
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The id of process ink `p`, the same in every [`InkSet`].
    pub fn process(p: Process) -> Self {
        Self(p as u16)
    }
}

/// The inks of one document or job: the four process inks, always first and in plate order,
/// then spot inks in the order added. Ids stay valid as inks are added.
#[derive(Clone, Debug, PartialEq)]
pub struct InkSet {
    inks: Vec<Ink>,
}

impl Default for InkSet {
    fn default() -> Self {
        Self::new()
    }
}

impl InkSet {
    /// The four process inks.
    pub fn new() -> Self {
        Self { inks: Process::ALL.into_iter().map(Ink::Process).collect() }
    }

    /// Adds `ink` and returns its id. An ink whose name is already in the set (compared after
    /// [`normalize_name`]) is not added again, and its existing id is returned; a spot named
    /// `Cyan` … `Black` is the process ink. A spot's name is stored normalised. `None` for the
    /// names `All` and `None`, which are not inks (§8.6.6.4), and when the set is full
    /// (65 536 inks).
    pub fn add(&mut self, ink: Ink) -> Option<InkId> {
        let spot = match ink {
            Ink::Process(p) => return Some(InkId::process(p)),
            Ink::Spot(spot) => spot,
        };
        let name = normalize_name(&spot.name).into_owned();
        if name == "All" || name == "None" {
            return None;
        }
        if let Some(id) = self.lookup(&name) {
            return Some(id);
        }
        let id = InkId(u16::try_from(self.inks.len()).ok()?);
        self.inks.push(Ink::Spot(Spot { name, ..spot }));
        Some(id)
    }

    /// The ink named `name`, compared after [`normalize_name`]: `"/Gold#20Foil"` finds
    /// `Gold Foil`.
    pub fn lookup(&self, name: &str) -> Option<InkId> {
        let name = normalize_name(name);
        if let Some(p) = Process::from_name(&name) {
            return Some(InkId::process(p));
        }
        let at = self.inks.iter().skip(4).position(|ink| ink.name() == name)?;
        u16::try_from(at + 4).ok().map(InkId)
    }

    pub fn get(&self, id: InkId) -> Option<&Ink> {
        self.inks.get(id.index())
    }

    /// Every ink with its id: the process inks, then the spot inks.
    pub fn iter(&self) -> impl Iterator<Item = (InkId, &Ink)> {
        // At most 65 536 inks (`add`), so every index fits.
        self.inks.iter().enumerate().map(|(i, ink)| (InkId(i as u16), ink))
    }

    /// The spot inks with their ids.
    pub fn spots(&self) -> impl Iterator<Item = (InkId, &Spot)> {
        self.iter().filter_map(|(id, ink)| match ink {
            Ink::Spot(s) => Some((id, s)),
            Ink::Process(_) => None,
        })
    }

    /// The number of inks, process inks included.
    pub fn len(&self) -> usize {
        self.inks.len()
    }

    /// Always `false`: a set holds the process inks.
    pub fn is_empty(&self) -> bool {
        self.inks.is_empty()
    }
}

/// A colour as ink tints.
#[derive(Clone, Debug, PartialEq)]
pub enum InkColor {
    /// Tints `0.0..=1.0` of the listed inks; other inks are unspecified (they keep what is
    /// under the paint when it overprints, and are knocked out otherwise). One entry is a
    /// `Separation` colour, several a `DeviceN` one.
    Tints(Vec<(InkId, f32)>),
    /// The same tint on every plate: the registration colour (the `All` colorant).
    All(f32),
    /// No ink on any plate (the `None` colorant).
    None,
}

/// Overprint parameters of a paint (ISO 32000-2 §8.6.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overprint {
    /// Fills and other non-stroking paints overprint (`op`).
    pub fill: bool,
    /// Strokes overprint (`OP`).
    pub stroke: bool,
    /// Nonzero overprint mode (`OPM 1`): a zero DeviceCMYK component leaves its plate as it is.
    pub nonzero: bool,
}

/// A colorant name as a PDF name object carries it: without a leading `/` and with `#xx`
/// escapes decoded (ISO 32000-2 §7.3.5), read as UTF-8 (invalid sequences replaced).
pub fn normalize_name(name: &str) -> Cow<'_, str> {
    let name = name.strip_prefix('/').unwrap_or(name);
    if !name.contains('#') {
        return Cow::Borrowed(name);
    }
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'#', Some(decoded)) => {
                out.push(decoded);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(name: &str) -> Ink {
        Ink::Spot(Spot { name: name.into(), alternate: Alternate::Lab([80.0, 5.0, 60.0]), solidity: None })
    }

    #[test]
    fn spots_follow_the_process_inks_and_names_are_compared_as_pdf_names() {
        let mut set = InkSet::new();
        let gold = set.add(spot("/Gold#20Foil")).unwrap();
        assert_eq!(gold.index(), 4);
        assert_eq!(set.get(gold).unwrap().name(), "Gold Foil");
        assert_eq!(set.add(spot("Gold Foil")), Some(gold), "added once");
        assert_eq!(set.lookup("Gold#20Foil"), Some(gold));
        assert_eq!(set.add(spot("Black")), Some(InkId::process(Process::Black)));
        assert_eq!(set.lookup("/Cyan"), Some(InkId::process(Process::Cyan)));
        assert_eq!(set.add(spot("All")), None);
        assert_eq!(set.add(spot("None")), None);
        assert_eq!(set.lookup("gold foil"), None, "names are case-sensitive");
        let names: Vec<&str> = set.iter().map(|(_, ink)| ink.name()).collect();
        assert_eq!(names, ["Cyan", "Magenta", "Yellow", "Black", "Gold Foil"]);
        assert_eq!(set.spots().count(), 1);
    }

    #[test]
    fn malformed_escapes_stay_as_written() {
        assert_eq!(normalize_name("A#2"), "A#2");
        assert_eq!(normalize_name("A#zz#41"), "A#zzA");
        assert_eq!(normalize_name("#E2#82#AC"), "€");
    }
}
