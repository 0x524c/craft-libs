#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Print separations for the Craft apps: an ink model shared by the writers, plates rendered
//! from a PDF, and process colour conversion through craft-cms. See the README.

pub mod cms;
pub mod ink;
pub mod plates;

pub use cms::{CmsConversion, ProfileError};
pub use ink::{Alternate, Ink, InkColor, InkId, InkSet, Overprint, Process, Spot, normalize_name};
pub use plates::{Conversion, Error, Fallback, MAX_PLATE_PIXELS, MAX_SPOT_INKS, Options, Plates, plates};
