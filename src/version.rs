//! R Version strings
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq, std::hash::Hash, Clone)]
/// Represents an R version string like "1.2.3" or "2.5-1".
///
/// R version strings consist of non-negative integers separated by `.` or `-`.
/// Both separators are equivalent: `2.5-1` and `2.5.1` represent the same version.
/// There is no concept of pre-release versions in R's versioning scheme.
pub struct Version {
    /// Version components like [1, 2, 3]
    pub components: Vec<u32>,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            &self
                .components
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join("."),
        )
    }
}

impl Version {
    /// Create a new version
    pub fn new(major: u32, minor: u32, patch: Option<u32>) -> Self {
        Self {
            components: if let Some(patch) = patch {
                vec![major, minor, patch]
            } else {
                vec![major, minor]
            },
        }
    }
}

/// Error parsing an R version string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionParseError {
    /// The version has fewer than two components, e.g. `1`.
    TooFewComponents(String),

    /// A component is not a non-negative integer that fits in a `u32`.
    InvalidComponent(String),
}

impl std::fmt::Display for VersionParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::TooFewComponents(version) => {
                write!(f, "version {version:?} has fewer than two components")
            }
            Self::InvalidComponent(component) => {
                write!(f, "invalid version component {component:?}")
            }
        }
    }
}

impl std::error::Error for VersionParseError {}

/// Split an R version string into its numeric components.
///
/// Both '.' and '-' are valid separators in R version strings and are equivalent,
/// e.g. "2.5-1" == "2.5.1".
pub(crate) fn parse_components(s: &str) -> Result<Vec<u32>, VersionParseError> {
    let components = s
        .split(['.', '-'])
        .map(|part| {
            part.parse()
                .map_err(|_| VersionParseError::InvalidComponent(part.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if components.len() < 2 {
        return Err(VersionParseError::TooFewComponents(s.to_string()));
    }

    Ok(components)
}

impl std::str::FromStr for Version {
    type Err = VersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            components: parse_components(s)?,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.components.iter().zip(other.components.iter()) {
            match a.cmp(b) {
                Ordering::Equal => continue,
                ordering => return ordering,
            }
        }
        self.components.len().cmp(&other.components.len())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::{Version, VersionParseError};
    use std::str::FromStr;

    #[test]
    fn test_version_from_str() {
        let version = Version::from_str("1.2.3").unwrap();
        assert_eq!(version, Version::new(1, 2, Some(3)));

        // '-' and '.' are equivalent separators in R
        let version = Version::from_str("2.5-1").unwrap();
        assert_eq!(version.components, vec![2, 5, 1]);

        // Development versions use a 4th numeric component
        let version = Version::from_str("1.2.3.9000").unwrap();
        assert_eq!(version.components, vec![1, 2, 3, 9000]);
    }

    #[test]
    fn test_version_cmp() {
        use std::cmp::Ordering;

        let v1 = Version::from_str("1.2.3").unwrap();
        let v2 = Version::from_str("1.2.3").unwrap();
        assert_eq!(v1.cmp(&v2), Ordering::Equal);

        let v1 = Version::from_str("1.2.3").unwrap();
        let v2 = Version::from_str("1.2.4").unwrap();
        assert_eq!(v1.cmp(&v2), Ordering::Less);

        // '-' and '.' are equivalent: "2.5-1" == "2.5.1"
        let v1 = Version::from_str("2.5-1").unwrap();
        let v2 = Version::from_str("2.5.1").unwrap();
        assert_eq!(v1.cmp(&v2), Ordering::Equal);

        // Versions can have more than three components
        let v1 = Version::from_str("1.2.3.9000").unwrap();
        let v2 = Version::from_str("1.2.3").unwrap();
        assert_eq!(v1.cmp(&v2), Ordering::Greater);

        let v1 = Version::from_str("1.2.3.9000").unwrap();
        let v2 = Version::from_str("1.2.4").unwrap();
        assert_eq!(v1.cmp(&v2), Ordering::Less);
    }

    #[test]
    fn test_version_display() {
        // Display normalizes to '.' separator
        let version = Version::from_str("1.2.3").unwrap();
        assert_eq!(version.to_string(), "1.2.3");

        let version = Version::from_str("2.5-1").unwrap();
        assert_eq!(version.to_string(), "2.5.1");
    }

    #[test]
    fn test_version_invalid() {
        assert!(Version::from_str("a").is_err());
        assert!(Version::from_str("1.a.3").is_err());
        // Single component is not a valid R package version
        assert!(Version::from_str("1").is_err());
    }

    #[test]
    fn test_parse_errors() {
        assert_eq!(
            Version::from_str("1.a.3"),
            Err(VersionParseError::InvalidComponent("a".to_string()))
        );
        assert_eq!(
            Version::from_str("1..3"),
            Err(VersionParseError::InvalidComponent("".to_string()))
        );
        assert_eq!(
            Version::from_str("1"),
            Err(VersionParseError::TooFewComponents("1".to_string()))
        );
    }
}
