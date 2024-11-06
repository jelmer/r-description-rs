#![deny(missing_docs)]
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

pub mod lossless;
pub mod lossy;

mod relations;
pub use relations::{VersionConstraint, VersionLookup};

pub use lossy::RDescription;

mod version;
pub use version::Version;

#[derive(Debug, PartialEq, Eq)]
/// A block of R code
///
/// This is a simple wrapper around a string that represents a block of R code, as used in e.g. the
/// Authors@R field.
pub struct RCode(String);

impl std::str::FromStr for RCode {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl std::fmt::Display for RCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
