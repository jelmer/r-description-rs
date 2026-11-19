//! Values of single-valued fields with a fixed set of options.

/// Operating system type a package is restricted to, from the `OS_type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsType {
    /// Unix-like systems.
    Unix,
    /// Microsoft Windows.
    Windows,
}

impl std::fmt::Display for OsType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unix => "unix",
            Self::Windows => "windows",
        })
    }
}

/// Error returned when an `OS_type` value is not recognised.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InvalidOsType(pub String);

impl std::fmt::Display for InvalidOsType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "expected unix or windows, got {:?}", self.0)
    }
}

impl std::error::Error for InvalidOsType {}

impl std::str::FromStr for OsType {
    type Err = InvalidOsType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unix" => Ok(Self::Unix),
            "windows" => Ok(Self::Windows),
            _ => Err(InvalidOsType(s.to_string())),
        }
    }
}

/// Priority of a package, from the `Priority` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// A base package, shipped with R.
    Base,
    /// A recommended package, shipped with binary distributions of R.
    Recommended,
    /// An optional package; only found in historical repository metadata.
    Optional,
    /// A contributed package; only found in historical repository metadata.
    Contrib,
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Self::Base => "base",
            Self::Recommended => "recommended",
            Self::Optional => "optional",
            Self::Contrib => "contrib",
        })
    }
}

/// Error returned when a `Priority` value is not recognised.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InvalidPriority(pub String);

impl std::fmt::Display for InvalidPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "expected base, recommended, optional or contrib, got {:?}",
            self.0
        )
    }
}

impl std::error::Error for InvalidPriority {}

impl std::str::FromStr for Priority {
    type Err = InvalidPriority;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "base" => Ok(Self::Base),
            "recommended" => Ok(Self::Recommended),
            "optional" => Ok(Self::Optional),
            "contrib" => Ok(Self::Contrib),
            _ => Err(InvalidPriority(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_type_round_trip() {
        for (text, value) in [("unix", OsType::Unix), ("windows", OsType::Windows)] {
            assert_eq!(text.parse::<OsType>(), Ok(value));
            assert_eq!(value.to_string(), text);
        }
        assert_eq!(
            "Unix".parse::<OsType>(),
            Err(InvalidOsType("Unix".to_string()))
        );
    }

    #[test]
    fn test_priority_round_trip() {
        for (text, value) in [
            ("base", Priority::Base),
            ("recommended", Priority::Recommended),
            ("optional", Priority::Optional),
            ("contrib", Priority::Contrib),
        ] {
            assert_eq!(text.parse::<Priority>(), Ok(value));
            assert_eq!(value.to_string(), text);
        }
        assert_eq!(
            "high".parse::<Priority>(),
            Err(InvalidPriority("high".to_string()))
        );
    }
}
