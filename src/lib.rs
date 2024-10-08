//! A library for parsing and manipulating R DESCRIPTION files.
//!
//! See https://r-pkgs.org/description.html for more information.
//!
//! See the ``lossless`` module for a lossless parser that is
//! forgiving in the face of errors and preserves formatting while editing
//! at the expense of a more complex API.
//!
//! Besides the fields in the DESCRIPTION file, the library also can
//! parse and evaluate R version strings according to the rules in
//! https://cran.r-project.org/doc/manuals/r-release/R-exts.html#The-DESCRIPTION-file
//!
//! # Example
//!
//! ```rust
//! use r_description::RDescription;
//! use std::str::FromStr;
//!
//! let desc = RDescription::from_str(
//!    r#"Package: foo
//! Title: A Foo Package
//! Version: 0.1.0
//! Authors@R: person("First", "Last", email = "email@example.com", role = c("aut", "cre"))
//! Description: A longer description of the package.
//! License: MIT + file LICENSE
//! Depends: R (>= 3.3.0)
//! URL: https://example.com
//! "#).unwrap();
//! assert_eq!(desc.name, "foo");
//! assert_eq!(desc.title, "A Foo Package");
//! assert_eq!(desc.version, "0.1.0".parse().unwrap());
//! let depends = desc.depends.as_ref().unwrap();
//! assert_eq!(depends.len(), 1);
//! assert_eq!(depends[0].name, "R");
//! ```

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
