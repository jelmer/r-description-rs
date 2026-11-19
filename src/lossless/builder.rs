//! Builder for new DESCRIPTION files.

use super::{is_valid_field_name, InvalidFieldName, RDescription, Relations};
use crate::remotes::{format_remotes, Remote, RemoteParseError};
use crate::{RCode, Version};
use deb822_lossless::{IndentPattern, Paragraph};

/// Error returned by [`RDescriptionBuilder::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// A field name passed to [`RDescriptionBuilder::field`] is invalid.
    InvalidFieldName(InvalidFieldName),

    /// A value can not be represented in a DESCRIPTION file, e.g. because it contains an
    /// empty line.
    InvalidValue {
        /// The field the value was given for.
        field: String,
        /// Why the value is invalid.
        message: String,
    },

    /// A remote can not be written to the `Remotes` field.
    Remotes(RemoteParseError),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidFieldName(e) => e.fmt(f),
            Self::InvalidValue { field, message } => {
                write!(f, "invalid value for {field}: {message}")
            }
            Self::Remotes(e) => write!(f, "invalid Remotes: {e}"),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidFieldName(e) => Some(e),
            Self::InvalidValue { .. } => None,
            Self::Remotes(e) => Some(e),
        }
    }
}

/// Builder for a new DESCRIPTION file.
///
/// Fields are written in the order they are added; adding a field a second time replaces
/// its value. Continuation lines are indented by four spaces. Errors are reported by
/// [`RDescriptionBuilder::build`].
///
/// ```
/// use r_description::lossless::RDescription;
///
/// let desc = RDescription::builder()
///     .package("foo")
///     .version(&"1.0.0".parse().unwrap())
///     .title("Does Foo")
///     .imports("cli (>= 3.0), glue".parse().unwrap())
///     .field("Config/testthat/edition", "3")
///     .build()
///     .unwrap();
/// assert_eq!(
///     desc.to_string(),
///     "Package: foo\nVersion: 1.0.0\nTitle: Does Foo\nImports: cli (>= 3.0), glue\nConfig/testthat/edition: 3\n"
/// );
/// ```
#[derive(Debug, Default)]
pub struct RDescriptionBuilder {
    fields: Vec<(String, String)>,
    error: Option<BuildError>,
}

impl RDescriptionBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(mut self, field: &str, value: String) -> Self {
        match self.fields.iter_mut().find(|(name, _)| name == field) {
            Some((_, existing)) => *existing = value,
            None => self.fields.push((field.to_string(), value)),
        }
        self
    }

    /// Add an arbitrary field.
    pub fn field(mut self, name: &str, value: &str) -> Self {
        if !is_valid_field_name(name) {
            self.error
                .get_or_insert(BuildError::InvalidFieldName(InvalidFieldName(
                    name.to_string(),
                )));
            return self;
        }
        self.push(name, value.to_string())
    }

    /// Set the package name.
    pub fn package(self, package: &str) -> Self {
        self.push("Package", package.to_string())
    }

    /// Set the version.
    pub fn version(self, version: &Version) -> Self {
        self.push("Version", version.to_string())
    }

    /// Set the title.
    pub fn title(self, title: &str) -> Self {
        self.push("Title", title.to_string())
    }

    /// Set the description.
    pub fn description(self, description: &str) -> Self {
        self.push("Description", description.to_string())
    }

    /// Set the license.
    pub fn license(self, license: &str) -> Self {
        self.push("License", license.to_string())
    }

    /// Set the `Authors@R` field.
    pub fn authors(self, authors: &RCode) -> Self {
        self.push("Authors@R", authors.to_string())
    }

    /// Set the free-form author list.
    pub fn author(self, author: &str) -> Self {
        self.push("Author", author.to_string())
    }

    /// Set the maintainer.
    pub fn maintainer(self, maintainer: &str) -> Self {
        self.push("Maintainer", maintainer.to_string())
    }

    /// Set the packages this package depends on.
    pub fn depends(self, depends: Relations) -> Self {
        self.push("Depends", depends.to_string())
    }

    /// Set the packages this package imports.
    pub fn imports(self, imports: Relations) -> Self {
        self.push("Imports", imports.to_string())
    }

    /// Set the packages this package suggests.
    pub fn suggests(self, suggests: Relations) -> Self {
        self.push("Suggests", suggests.to_string())
    }

    /// Set the packages this package links to.
    pub fn linking_to(self, linking_to: Relations) -> Self {
        self.push("LinkingTo", linking_to.to_string())
    }

    /// Set the packages this package enhances.
    pub fn enhances(self, enhances: Relations) -> Self {
        self.push("Enhances", enhances.to_string())
    }

    /// Set the URLs for the package.
    pub fn urls(self, urls: &[url::Url]) -> Self {
        let urls: Vec<&str> = urls.iter().map(url::Url::as_str).collect();
        self.push("URL", urls.join(", "))
    }

    /// Set the URL for bug reports.
    pub fn bug_reports(self, bug_reports: &url::Url) -> Self {
        self.push("BugReports", bug_reports.to_string())
    }

    /// Set the non-CRAN sources for dependencies.
    pub fn remotes(mut self, remotes: &[Remote]) -> Self {
        match format_remotes(remotes) {
            Ok(value) => self.push("Remotes", value),
            Err(e) => {
                self.error.get_or_insert(BuildError::Remotes(e));
                self
            }
        }
    }

    /// Set whether the package contains code that needs compiling.
    pub fn needs_compilation(self, needs_compilation: bool) -> Self {
        let value = if needs_compilation { "yes" } else { "no" };
        self.push("NeedsCompilation", value.to_string())
    }

    // TODO: add os_type() and priority() taking the typed OsType and Priority values.

    /// Build the description, reporting the first error encountered.
    pub fn build(self) -> Result<RDescription, BuildError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut paragraph = Paragraph::new();
        for (field, value) in &self.fields {
            paragraph
                .try_set_with_forced_indent(field, value, &IndentPattern::Fixed(4), None)
                .map_err(|e| BuildError::InvalidValue {
                    field: field.clone(),
                    message: e.to_string(),
                })?;
        }
        Ok(RDescription(paragraph))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build() {
        let desc = RDescriptionBuilder::new()
            .package("foo")
            .title("Does Foo")
            .version(&"1.0-1".parse().unwrap())
            .description("Foo does\nmany things.")
            .license("MIT")
            .authors(&"person('A', 'B')".parse().unwrap())
            .urls(&[
                "https://example.com/".parse().unwrap(),
                "https://github.com/x/foo".parse().unwrap(),
            ])
            .bug_reports(&"https://github.com/x/foo/issues".parse().unwrap())
            .depends("R (>= 4.1)".parse().unwrap())
            .suggests("testthat".parse().unwrap())
            .remotes(&["x/bar".parse().unwrap()])
            .needs_compilation(false)
            .build()
            .unwrap();
        assert_eq!(
            desc.to_string(),
            "Package: foo\nTitle: Does Foo\nVersion: 1.0-1\nDescription: Foo does\n    many things.\nLicense: MIT\nAuthors@R: person('A', 'B')\nURL: https://example.com/, https://github.com/x/foo\nBugReports: https://github.com/x/foo/issues\nDepends: R (>= 4.1)\nSuggests: testthat\nRemotes: x/bar\nNeedsCompilation: no\n"
        );
        assert_eq!(desc.version().unwrap().unwrap().as_str(), "1.0-1");
        assert_eq!(
            desc.description(),
            Some("Foo does\nmany things.".to_string())
        );
    }

    #[test]
    fn test_build_empty() {
        let desc = RDescriptionBuilder::new().build().unwrap();
        assert_eq!(desc.to_string(), "");
    }

    #[test]
    fn test_replace_field() {
        let desc = RDescriptionBuilder::new()
            .package("foo")
            .title("Old")
            .package("bar")
            .build()
            .unwrap();
        assert_eq!(desc.to_string(), "Package: bar\nTitle: Old\n");
    }

    #[test]
    fn test_invalid_field_name() {
        let result = RDescriptionBuilder::new()
            .package("foo")
            .field("Bad:Name", "x")
            .field("Also Bad", "y")
            .build();
        assert_eq!(
            result.unwrap_err(),
            BuildError::InvalidFieldName(InvalidFieldName("Bad:Name".to_string()))
        );
    }

    #[test]
    fn test_invalid_value() {
        let result = RDescriptionBuilder::new()
            .package("foo")
            .description("First paragraph.\n\nSecond paragraph.")
            .build();
        assert_eq!(
            result.unwrap_err(),
            BuildError::InvalidValue {
                field: "Description".to_string(),
                message:
                    "Invalid value: empty continuation line (line with only whitespace) at line 2"
                        .to_string(),
            }
        );
    }
}
