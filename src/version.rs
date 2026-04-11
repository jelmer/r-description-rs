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

impl std::str::FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Both '.' and '-' are valid separators in R version strings and are equivalent.
        // e.g. "2.5-1" == "2.5.1"
        let components = s
            .split(|c| c == '.' || c == '-')
            .map(|part| {
                part.parse()
                    .map_err(|_| format!("Invalid version component: {s}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if components.len() < 2 {
            return Err(format!("Invalid version string: {s}"));
        }

        Ok(Self { components })
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
    use super::Version;
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
}
