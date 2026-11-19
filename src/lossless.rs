//! A library for parsing and manipulating R DESCRIPTION files.
//!
//! This module allows losslessly parsing R DESCRIPTION files into a structured representation.
//! This allows modification of individual fields while preserving the
//! original formatting of the file.
//!
//! This parser also allows for syntax errors in the input, and will attempt to parse as much as
//! possible. Parsing with `FromStr` fails on syntax errors, while
//! [`RDescription::from_str_relaxed`] returns whatever could be recovered together with the
//! errors and their byte ranges in the input:
//!
//! ```rust
//! use r_description::lossless::RDescription;
//!
//! let text = "Package foo\nVersion: 1.0\n";
//! let (desc, errors) = RDescription::from_str_relaxed(text);
//!
//! assert_eq!(desc.version().unwrap().unwrap().as_str(), "1.0");
//! assert_eq!(errors[0].message, "missing colon ':' after field name");
//! assert_eq!(&text[errors[0].range], "foo");
//!
//! // The original text is kept as-is.
//! assert_eq!(desc.to_string(), text);
//! ```
//!
//! [`RDescription::field_range`] and [`RDescription::value_range`] return the position of a
//! field in the file, which can be used to point at invalid values.
//!
//! See https://r-pkgs.org/description.html for more information.

use crate::remotes::{format_remotes, parse_remotes, Remote, RemoteParseError};
use crate::RCode;
use deb822_lossless::{Deb822, Paragraph};
pub use deb822_lossless::{PositionedParseError, TextRange};
pub use relations::{Relation, Relations, Version};
use rowan::ast::AstNode;

mod builder;
pub use builder::{BuildError, RDescriptionBuilder};

/// R DESCRIPTION file
///
/// The fields live in the first paragraph of the parsed file; anything around it, such as
/// comments, is kept so that the file round-trips unchanged.
#[derive(Debug)]
pub struct RDescription(Paragraph);

impl std::fmt::Display for RDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let root = self
            .0
            .syntax()
            .ancestors()
            .last()
            .expect("ancestors() always includes the node itself");
        write!(f, "{}", root.text())
    }
}

impl Default for RDescription {
    fn default() -> Self {
        Self(Paragraph::new())
    }
}

impl From<RDescription> for Paragraph {
    fn from(description: RDescription) -> Self {
        description.0
    }
}

impl From<Paragraph> for RDescription {
    fn from(paragraph: Paragraph) -> Self {
        Self(paragraph)
    }
}

#[derive(Debug)]
/// Error type for parsing DESCRIPTION files
pub enum Error {
    /// I/O error
    Io(std::io::Error),

    /// Parse error
    Parse(deb822_lossless::ParseError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse(e) => write!(f, "Parse error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<deb822_lossless::ParseError> for Error {
    fn from(e: deb822_lossless::ParseError) -> Self {
        Self::Parse(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Error returned when a field is present but its value is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    field: &'static str,
    kind: FieldErrorKind,
    range: TextRange,
}

/// What is wrong with the value of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldErrorKind {
    /// The value is not a valid list of relations.
    Relations(relations::ParseError),

    /// The value contains an invalid URL.
    Url {
        /// The invalid URL.
        url: String,
        /// Why it could not be parsed.
        error: url::ParseError,
    },

    /// The value is not a valid logical (yes, no, true or false).
    Logical(String),

    /// The value is not a valid list of remotes.
    Remotes(RemoteParseError),

    /// The value is not a valid version.
    Version(crate::VersionParseError),
}

impl FieldErrorKind {
    /// Range of the offending part of the value, relative to the value, if it is more
    /// specific than the whole value.
    fn range(&self) -> Option<TextRange> {
        match self {
            Self::Relations(e) => e.errors().first().map(|e| e.range),
            // TODO: report the position of the invalid remote
            Self::Url { .. } | Self::Logical(_) | Self::Remotes(_) | Self::Version(_) => None,
        }
    }
}

impl std::fmt::Display for FieldErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Relations(e) => e.fmt(f),
            Self::Url { url, error } => write!(f, "{url:?}: {error}"),
            Self::Logical(value) => {
                write!(f, "expected yes, no, true or false, got {value:?}")
            }
            Self::Remotes(e) => e.fmt(f),
            Self::Version(e) => e.fmt(f),
        }
    }
}

impl FieldError {
    /// Name of the field with the invalid value.
    pub fn field(&self) -> &str {
        self.field
    }

    /// What is wrong with the value.
    pub fn kind(&self) -> &FieldErrorKind {
        &self.kind
    }

    /// Byte range in the file of the invalid part of the value.
    ///
    /// For list fields such as `Imports` this is the offending entry; otherwise it is the
    /// whole value. Like [`RDescription::value_range`], this refers to the file as it was
    /// when the error was returned.
    pub fn range(&self) -> TextRange {
        self.range
    }
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "invalid {} field: {}", self.field, self.kind)
    }
}

impl std::error::Error for FieldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            FieldErrorKind::Relations(e) => Some(e),
            FieldErrorKind::Url { error, .. } => Some(error),
            FieldErrorKind::Logical(_) => None,
            FieldErrorKind::Remotes(e) => Some(e),
            FieldErrorKind::Version(e) => Some(e),
        }
    }
}

/// Error returned when a field name can not be used in a DESCRIPTION file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidFieldName(pub String);

impl std::fmt::Display for InvalidFieldName {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "invalid field name {:?}", self.0)
    }
}

impl std::error::Error for InvalidFieldName {}

/// Check whether `name` is a portable field name.
///
/// R itself accepts anything before the first colon, but Writing R Extensions requires
/// ASCII names, and some names, such as ones containing spaces, are not read back
/// correctly by the parser.
fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_graphic() && c != ':')
}

/// Map an offset in the value of a field, as returned by `Entry::value`, to an offset in
/// the file.
///
/// `lines` are the source ranges of the value lines; the value consists of these lines,
/// without indentation, joined by newlines.
fn value_offset_to_source(lines: &[TextRange], offset: rowan::TextSize) -> rowan::TextSize {
    let mut line_start = rowan::TextSize::from(0);
    for line in lines {
        if offset <= line_start + line.len() {
            return line.start() + (offset - line_start);
        }
        line_start += line.len() + rowan::TextSize::from(1);
    }
    lines.last().map_or(offset, |line| line.end())
}

/// Parse a logical field value such as `LazyData`.
fn parse_logical(value: &str) -> Result<bool, FieldErrorKind> {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" => Ok(true),
        "no" | "false" => Ok(false),
        _ => Err(FieldErrorKind::Logical(value.to_string())),
    }
}

/// Parse a list of URLs, ignoring empty entries.
fn parse_url_list(
    value: &str,
    separator: impl Fn(char) -> bool,
) -> Result<Vec<url::Url>, FieldErrorKind> {
    value
        .split(separator)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            url::Url::parse(s).map_err(|error| FieldErrorKind::Url {
                url: s.to_string(),
                error,
            })
        })
        .collect()
}

impl std::str::FromStr for RDescription {
    type Err = Error;

    /// Parse a DESCRIPTION file, failing on any syntax error.
    ///
    /// Use [`RDescription::from_str_relaxed`] to get the recovered file along with the
    /// positions of the errors.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (desc, errors) = Self::from_str_relaxed(s);
        if errors.is_empty() {
            Ok(desc)
        } else {
            Err(Error::Parse(deb822_lossless::ParseError(
                errors.into_iter().map(|e| e.message).collect(),
            )))
        }
    }
}

impl RDescription {
    /// Parse a DESCRIPTION file, recovering from syntax errors.
    ///
    /// This always returns a description holding as much as could be parsed, along with any
    /// errors that were encountered. Error ranges are byte offsets into `s`. The file is
    /// preserved as-is, so `to_string()` returns `s` until it is modified.
    pub fn from_str_relaxed(s: &str) -> (Self, Vec<PositionedParseError>) {
        let parsed = Deb822::parse(s);
        let mut errors = parsed.positioned_errors().to_vec();
        let mut deb822 = parsed.tree();
        let mut paragraphs = deb822.paragraphs();
        let paragraph = paragraphs.next();
        errors.extend(paragraphs.map(|p| PositionedParseError {
            message: "unexpected additional paragraph".to_string(),
            range: p.text_range(),
            code: Some("multiple-paragraphs".to_string()),
        }));
        let paragraph = paragraph.unwrap_or_else(|| deb822.add_paragraph());
        (Self(paragraph), errors)
    }

    /// Return the byte range of the entry for `field`, including its name.
    ///
    /// Ranges are relative to the current rendering of the file, so they need to be
    /// obtained again after the file is modified.
    pub fn field_range(&self, field: &str) -> Option<TextRange> {
        self.0.get_entry(field).map(|entry| entry.text_range())
    }

    /// Return the byte range of the value of `field`.
    ///
    /// Ranges are relative to the current rendering of the file, so they need to be
    /// obtained again after the file is modified.
    pub fn value_range(&self, field: &str) -> Option<TextRange> {
        self.0.get_entry(field)?.value_range()
    }

    /// Create a new empty R DESCRIPTION file
    pub fn new() -> Self {
        Self(Paragraph::new())
    }

    /// Start building a new DESCRIPTION file.
    pub fn builder() -> RDescriptionBuilder {
        RDescriptionBuilder::new()
    }

    /// Parse the value of `field` if it is present.
    fn parse_field<T>(
        &self,
        field: &'static str,
        parse: impl FnOnce(&str) -> Result<T, FieldErrorKind>,
    ) -> Result<Option<T>, FieldError> {
        let Some(entry) = self.0.get_entry(field) else {
            return Ok(None);
        };
        parse(&entry.value()).map(Some).map_err(|kind| {
            let lines = entry.value_line_ranges();
            let range = match (kind.range(), lines.first(), lines.last()) {
                (Some(range), _, _) => TextRange::new(
                    value_offset_to_source(&lines, range.start()),
                    value_offset_to_source(&lines, range.end()),
                ),
                (None, Some(first), Some(last)) => first.cover(*last),
                // The value is empty, so point at the whole entry.
                _ => entry.text_range(),
            };
            FieldError { field, kind, range }
        })
    }

    /// Return the package name
    pub fn package(&self) -> Option<String> {
        self.0.get("Package")
    }

    /// Set the package name
    pub fn set_package(&mut self, package: &str) {
        self.0.set("Package", package);
    }

    /// One line description of the package, and is often shown in a package listing
    ///
    /// It should be plain text (no markup), capitalised like a title, and NOT end in a period.
    /// Keep it short: listings will often truncate the title to 65 characters.
    pub fn title(&self) -> Option<String> {
        self.0.get("Title")
    }

    /// Return the maintainer of the package
    pub fn maintainer(&self) -> Option<String> {
        self.0.get("Maintainer")
    }

    /// Set the maintainer of the package
    pub fn set_maintainer(&mut self, maintainer: &str) {
        self.0.set("Maintainer", maintainer);
    }

    /// Return the authors of the package
    pub fn authors(&self) -> Option<RCode> {
        self.0.get("Authors@R").map(|s| s.parse().unwrap())
    }

    /// Set the authors of the package
    pub fn set_authors(&mut self, authors: &RCode) {
        self.0.set("Authors@R", &authors.to_string());
    }

    /// Set the title of the package
    pub fn set_title(&mut self, title: &str) {
        self.0.set("Title", title);
    }

    /// Return the description of the package
    pub fn description(&self) -> Option<String> {
        self.0.get("Description")
    }

    /// Set the description of the package
    pub fn set_description(&mut self, description: &str) {
        self.0.set("Description", description);
    }

    /// Return the version of the package
    pub fn version(&self) -> Result<Option<Version>, FieldError> {
        self.parse_field("Version", |s| s.parse().map_err(FieldErrorKind::Version))
    }

    /// Set the version of the package
    pub fn set_version(&mut self, version: &Version) {
        self.0.set("Version", version.as_str());
    }

    /// Return the encoding of the description file
    pub fn encoding(&self) -> Option<String> {
        self.0.get("Encoding")
    }

    /// Set the encoding of the description file
    pub fn set_encoding(&mut self, encoding: &str) {
        self.0.set("Encoding", encoding);
    }

    /// Return the license of the package
    pub fn license(&self) -> Option<String> {
        self.0.get("License")
    }

    /// Set the license of the package
    pub fn set_license(&mut self, license: &str) {
        self.0.set("License", license);
    }

    /// Return the roxygen note
    pub fn roxygen_note(&self) -> Option<String> {
        self.0.get("RoxygenNote")
    }

    /// Set the roxygen note
    pub fn set_roxygen_note(&mut self, roxygen_note: &str) {
        self.0.set("RoxygenNote", roxygen_note);
    }

    /// Return the roxygen version
    pub fn roxygen(&self) -> Option<String> {
        self.0.get("Roxygen")
    }

    /// Set the roxygen version
    pub fn set_roxygen(&mut self, roxygen: &str) {
        self.0.set("Roxygen", roxygen);
    }

    /// Return the URLs for the package, e.g. its homepage and source repository
    pub fn urls(&self) -> Result<Option<Vec<url::Url>>, FieldError> {
        self.parse_field("URL", |s| {
            parse_url_list(s, |c| c == ',' || c.is_whitespace())
        })
    }

    /// Set the URLs for the package
    ///
    /// An empty list removes the field.
    pub fn set_urls(&mut self, urls: &[url::Url]) {
        self.set_url_list("URL", urls);
    }

    /// Return the bug reports URL
    pub fn bug_reports(&self) -> Result<Option<url::Url>, FieldError> {
        self.parse_field("BugReports", |s| {
            url::Url::parse(s).map_err(|error| FieldErrorKind::Url {
                url: s.to_string(),
                error,
            })
        })
    }

    /// Set the bug reports URL
    pub fn set_bug_reports(&mut self, bug_reports: &url::Url) {
        self.0.set("BugReports", bug_reports.as_str());
    }

    /// Return the imports field
    pub fn imports(&self) -> Result<Option<Relations>, FieldError> {
        self.parse_field("Imports", |s| s.parse().map_err(FieldErrorKind::Relations))
    }

    /// Set the imports field
    pub fn set_imports(&mut self, imports: Relations) {
        self.0.set("Imports", &imports.to_string());
    }

    /// Return the suggests field
    pub fn suggests(&self) -> Result<Option<Relations>, FieldError> {
        self.parse_field("Suggests", |s| s.parse().map_err(FieldErrorKind::Relations))
    }

    /// Set the suggests field
    pub fn set_suggests(&mut self, suggests: Relations) {
        self.0.set("Suggests", &suggests.to_string());
    }

    /// Return the depends field
    pub fn depends(&self) -> Result<Option<Relations>, FieldError> {
        self.parse_field("Depends", |s| s.parse().map_err(FieldErrorKind::Relations))
    }

    /// Set the depends field
    pub fn set_depends(&mut self, depends: Relations) {
        self.0.set("Depends", &depends.to_string());
    }

    /// Return the linking-to field
    pub fn linking_to(&self) -> Result<Option<Relations>, FieldError> {
        self.parse_field("LinkingTo", |s| {
            s.parse().map_err(FieldErrorKind::Relations)
        })
    }

    /// Set the linking-to field
    pub fn set_linking_to(&mut self, linking_to: Relations) {
        self.0.set("LinkingTo", &linking_to.to_string());
    }

    /// Return the enhances field
    pub fn enhances(&self) -> Result<Option<Relations>, FieldError> {
        self.parse_field("Enhances", |s| s.parse().map_err(FieldErrorKind::Relations))
    }

    /// Set the enhances field
    pub fn set_enhances(&mut self, enhances: Relations) {
        self.0.set("Enhances", &enhances.to_string());
    }

    /// Return the lazy data field
    pub fn lazy_data(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("LazyData", parse_logical)
    }

    /// Set the lazy data field
    pub fn set_lazy_data(&mut self, lazy_data: bool) {
        self.0
            .set("LazyData", if lazy_data { "true" } else { "false" });
    }

    /// Return the collate field
    pub fn collate(&self) -> Option<String> {
        self.0.get("Collate")
    }

    /// Set the collate field
    pub fn set_collate(&mut self, collate: &str) {
        self.0.set("Collate", collate);
    }

    /// Return the vignette builder field
    pub fn vignette_builder(&self) -> Option<Vec<String>> {
        self.0
            .get("VignetteBuilder")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
    }

    /// Set the vignette builder field
    pub fn set_vignette_builder(&mut self, vignette_builder: &[&str]) {
        self.0.set("VignetteBuilder", &vignette_builder.join(", "));
    }

    /// Return the system requirements field
    pub fn system_requirements(&self) -> Option<Vec<String>> {
        self.0
            .get("SystemRequirements")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
    }

    /// Set the system requirements field
    pub fn set_system_requirements(&mut self, system_requirements: &[&str]) {
        self.0
            .set("SystemRequirements", &system_requirements.join(", "));
    }

    /// Return the date field
    pub fn date(&self) -> Option<String> {
        self.0.get("Date")
    }

    /// Set the date field
    pub fn set_date(&mut self, date: &str) {
        self.0.set("Date", date);
    }

    /// The R Repository to use for this package.
    ///
    /// E.g. "CRAN" or "Bioconductor"
    pub fn repository(&self) -> Option<String> {
        self.0.get("Repository")
    }

    /// Set the R Repository to use for this package.
    pub fn set_repository(&mut self, repository: &str) {
        self.0.set("Repository", repository);
    }

    /// Additional repositories where dependency packages may be found.
    pub fn additional_repositories(&self) -> Result<Option<Vec<url::Url>>, FieldError> {
        self.parse_field("Additional_repositories", |s| {
            parse_url_list(s, |c| c == ',')
        })
    }

    /// Set the additional repositories field.
    ///
    /// An empty list removes the field.
    pub fn set_additional_repositories(&mut self, repositories: &[url::Url]) {
        self.set_url_list("Additional_repositories", repositories);
    }

    fn set_url_list(&mut self, field: &str, urls: &[url::Url]) {
        if urls.is_empty() {
            self.0.remove(field);
        } else {
            let value = urls.iter().map(url::Url::as_str).collect::<Vec<_>>();
            self.0.set(field, &value.join(", "));
        }
    }

    /// Return the value of an arbitrary field.
    ///
    /// Field names are compared case-insensitively.
    pub fn get(&self, field: &str) -> Option<String> {
        self.0.get(field)
    }

    /// Set the value of an arbitrary field, replacing any existing value.
    ///
    /// The field name has to start with an ASCII letter or digit, followed by printable
    /// ASCII characters other than `:`.
    pub fn set(&mut self, field: &str, value: &str) -> Result<(), InvalidFieldName> {
        if !is_valid_field_name(field) {
            return Err(InvalidFieldName(field.to_string()));
        }
        self.0.set(field, value);
        Ok(())
    }

    /// Remove a field, if present.
    pub fn remove(&mut self, field: &str) {
        self.0.remove(field);
    }

    /// Return the free-form author list
    pub fn author(&self) -> Option<String> {
        self.0.get("Author")
    }

    /// Set the authors
    pub fn set_author(&mut self, author: &str) {
        self.0.set("Author", author);
    }

    /// Return the copyright statement
    pub fn copyright(&self) -> Option<String> {
        self.0.get("Copyright")
    }

    /// Set the copyright
    pub fn set_copyright(&mut self, copyright: &str) {
        self.0.set("Copyright", copyright);
    }

    /// Return the priority, e.g. "base" or "recommended"
    pub fn priority(&self) -> Option<String> {
        self.0.get("Priority")
    }

    /// Set the priority
    pub fn set_priority(&mut self, priority: &str) {
        self.0.set("Priority", priority);
    }

    /// Return the operating system type the package is restricted to, e.g. "unix"
    pub fn os_type(&self) -> Option<String> {
        self.0.get("OS_type")
    }

    /// Set the OS type
    pub fn set_os_type(&mut self, os_type: &str) {
        self.0.set("OS_type", os_type);
    }

    /// Return the package type, e.g. "Package" or "Translation"
    pub fn package_type(&self) -> Option<String> {
        self.0.get("Type")
    }

    /// Set the package type
    pub fn set_package_type(&mut self, package_type: &str) {
        self.0.set("Type", package_type);
    }

    /// Return the contact address for questions, if different from the maintainer
    pub fn contact(&self) -> Option<String> {
        self.0.get("Contact")
    }

    /// Set the contact
    pub fn set_contact(&mut self, contact: &str) {
        self.0.set("Contact", contact);
    }

    /// Return the mailing list for the package
    pub fn mailing_list(&self) -> Option<String> {
        self.0.get("MailingList")
    }

    /// Set the mailing list
    pub fn set_mailing_list(&mut self, mailing_list: &str) {
        self.0.set("MailingList", mailing_list);
    }

    /// Return the additional note
    pub fn note(&self) -> Option<String> {
        self.0.get("Note")
    }

    /// Set the note
    pub fn set_note(&mut self, note: &str) {
        self.0.set("Note", note);
    }

    /// Return the language(s) of the documentation, if not English
    pub fn language(&self) -> Option<String> {
        self.0.get("Language")
    }

    /// Set the language
    pub fn set_language(&mut self, language: &str) {
        self.0.set("Language", language);
    }

    /// Return whether the package uses lazy loading
    pub fn lazy_load(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("LazyLoad", parse_logical)
    }

    /// Set whether the package uses lazy loading
    pub fn set_lazy_load(&mut self, lazy_load: bool) {
        self.0
            .set("LazyLoad", if lazy_load { "true" } else { "false" });
    }

    /// Return whether R code keeps its source references
    pub fn keep_source(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("KeepSource", parse_logical)
    }

    /// Set whether R code keeps its source references
    pub fn set_keep_source(&mut self, keep_source: bool) {
        self.0
            .set("KeepSource", if keep_source { "true" } else { "false" });
    }

    /// Return whether the R code is byte-compiled on installation
    pub fn byte_compile(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("ByteCompile", parse_logical)
    }

    /// Set whether the R code is byte-compiled on installation
    pub fn set_byte_compile(&mut self, byte_compile: bool) {
        self.0
            .set("ByteCompile", if byte_compile { "true" } else { "false" });
    }

    /// Return whether native code is compiled with link-time optimization
    pub fn use_lto(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("UseLTO", parse_logical)
    }

    /// Set whether native code is compiled with link-time optimization
    pub fn set_use_lto(&mut self, use_lto: bool) {
        self.0.set("UseLTO", if use_lto { "true" } else { "false" });
    }

    /// Return whether the package is installed in a staging directory first
    pub fn staged_install(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("StagedInstall", parse_logical)
    }

    /// Set whether the package is installed in a staging directory first
    pub fn set_staged_install(&mut self, staged_install: bool) {
        self.0.set(
            "StagedInstall",
            if staged_install { "true" } else { "false" },
        );
    }

    /// Return legacy setting for zipping data, ignored by current versions of R
    pub fn zip_data(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("ZipData", parse_logical)
    }

    /// Set legacy setting for zipping data, ignored by current versions of R
    pub fn set_zip_data(&mut self, zip_data: bool) {
        self.0
            .set("ZipData", if zip_data { "true" } else { "false" });
    }

    /// Return whether to build for both architectures on Windows
    pub fn biarch(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("Biarch", parse_logical)
    }

    /// Set whether to build for both architectures on Windows
    pub fn set_biarch(&mut self, biarch: bool) {
        self.0.set("Biarch", if biarch { "true" } else { "false" });
    }

    /// Return whether vignettes are built by R CMD build
    pub fn build_vignettes(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("BuildVignettes", parse_logical)
    }

    /// Set whether vignettes are built by R CMD build
    pub fn set_build_vignettes(&mut self, build_vignettes: bool) {
        self.0.set(
            "BuildVignettes",
            if build_vignettes { "true" } else { "false" },
        );
    }

    /// Return whether the package contains code that needs compiling
    pub fn needs_compilation(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("NeedsCompilation", parse_logical)
    }

    /// Set whether the package contains code that needs compiling
    pub fn set_needs_compilation(&mut self, needs_compilation: bool) {
        self.0.set(
            "NeedsCompilation",
            if needs_compilation { "yes" } else { "no" },
        );
    }

    /// Return whether the license is a free or open source license
    pub fn license_is_foss(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("License_is_FOSS", parse_logical)
    }

    /// Set whether the license is a free or open source license
    pub fn set_license_is_foss(&mut self, license_is_foss: bool) {
        self.0.set(
            "License_is_FOSS",
            if license_is_foss { "yes" } else { "no" },
        );
    }

    /// Return whether the license restricts use
    pub fn license_restricts_use(&self) -> Result<Option<bool>, FieldError> {
        self.parse_field("License_restricts_use", parse_logical)
    }

    /// Set whether the license restricts use
    pub fn set_license_restricts_use(&mut self, license_restricts_use: bool) {
        self.0.set(
            "License_restricts_use",
            if license_restricts_use { "yes" } else { "no" },
        );
    }

    /// Return the Remotes field, listing non-CRAN sources for dependencies.
    pub fn remotes(&self) -> Result<Option<Vec<Remote>>, FieldError> {
        self.parse_field("Remotes", |s| {
            parse_remotes(s).map_err(FieldErrorKind::Remotes)
        })
    }

    /// Set the Remotes field. An empty slice removes the field.
    ///
    /// Fails without modifying the field if a remote can not be represented.
    pub fn set_remotes(&mut self, remotes: &[Remote]) -> Result<(), RemoteParseError> {
        if remotes.is_empty() {
            self.0.remove("Remotes");
        } else {
            self.0.set("Remotes", &format_remotes(remotes)?);
        }
        Ok(())
    }
}

pub mod relations {
    //! Parser for relationship fields like `Depends`, `Recommends`, etc.
    //!
    //! # Example
    //! ```
    //! use r_description::lossless::{Relations, Relation};
    //! use r_description::VersionConstraint;
    //!
    //! let mut relations: Relations = r"cli (>= 0.19.0), R".parse().unwrap();
    //! assert_eq!(relations.to_string(), "cli (>= 0.19.0), R");
    //! assert!(relations.satisfied_by(|name: &str| -> Option<r_description::Version> {
    //!    match name {
    //!    "cli" => Some("0.19.0".parse().unwrap()),
    //!    "R" => Some("2.25.1".parse().unwrap()),
    //!    _ => None
    //!    }}));
    //! relations.remove_relation(1);
    //! assert_eq!(relations.to_string(), "cli (>= 0.19.0)");
    //! ```
    use crate::relations::SyntaxKind::{self, *};
    use crate::relations::{RelationParseError, VersionConstraint};
    pub use crate::version::Version;
    use rowan::{Direction, NodeOrToken, TextRange, TextSize};

    /// A relation parse error, with the byte range in the input where it occurred.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct PositionedRelationParseError {
        /// The error.
        pub error: RelationParseError,
        /// Where in the input the error occurred.
        pub range: TextRange,
    }

    impl std::fmt::Display for PositionedRelationParseError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            self.error.fmt(f)
        }
    }

    impl std::error::Error for PositionedRelationParseError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.error)
        }
    }

    /// Error type for parsing relations fields
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ParseError(Vec<PositionedRelationParseError>);

    impl ParseError {
        /// The errors that were encountered, in the order they occur in the input.
        pub fn errors(&self) -> &[PositionedRelationParseError] {
            &self.0
        }
    }

    impl std::fmt::Display for ParseError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            let messages: Vec<String> = self.0.iter().map(|e| e.to_string()).collect();
            f.write_str(&messages.join("; "))
        }
    }

    impl std::error::Error for ParseError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.0
                .first()
                .map(|e| e as &(dyn std::error::Error + 'static))
        }
    }

    /// Second, implementing the `Language` trait teaches rowan to convert between
    /// these two SyntaxKind types, allowing for a nicer SyntaxNode API where
    /// "kinds" are values from our `enum SyntaxKind`, instead of plain u16 values.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Lang {}
    impl rowan::Language for Lang {
        type Kind = SyntaxKind;
        fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
            unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
        }
        fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
            kind.into()
        }
    }

    /// GreenNode is an immutable tree, which is cheap to change,
    /// but doesn't contain offsets and parent pointers.
    use rowan::{GreenNode, GreenToken};

    /// You can construct GreenNodes by hand, but a builder
    /// is helpful for top-down parsers: it maintains a stack
    /// of currently in-progress nodes
    use rowan::GreenNodeBuilder;

    /// Emit one token per character of a version constraint into an in-progress
    /// CONSTRAINT node, matching what the lexer would have produced for the same
    /// operator.
    fn push_constraint_tokens(builder: &mut GreenNodeBuilder, vc: &VersionConstraint) {
        for c in vc.to_string().chars() {
            let kind = match c {
                '>' => R_ANGLE,
                '<' => L_ANGLE,
                '=' => EQUAL,
                '!' => NOT,
                _ => unreachable!("unexpected constraint character {c:?}"),
            };
            builder.token(kind.into(), c.to_string().as_str());
        }
    }

    /// The parse results are stored as a "green tree".
    /// We'll discuss working with the results later
    struct Parse {
        green_node: GreenNode,
        errors: Vec<PositionedRelationParseError>,
    }

    fn parse(text: &str) -> Parse {
        struct Parser {
            /// input tokens, including whitespace,
            /// in *reverse* order.
            tokens: Vec<(SyntaxKind, String)>,
            /// the in-progress tree.
            builder: GreenNodeBuilder<'static>,
            /// the list of syntax errors we've accumulated
            /// so far.
            errors: Vec<PositionedRelationParseError>,
            /// byte offset of the next token in the input.
            offset: TextSize,
        }

        impl Parser {
            /// Record an error for the current token, which is wrapped in an error node.
            fn unexpected(&mut self) {
                let error = match self.tokens.last() {
                    Some((_, text)) => RelationParseError::UnexpectedToken(text.clone()),
                    None => RelationParseError::MissingPackageName,
                };
                self.error(error);
            }

            /// Range of the next `n` tokens.
            fn next_range(&self, n: usize) -> TextRange {
                let len: TextSize = self
                    .tokens
                    .iter()
                    .rev()
                    .take(n)
                    .map(|(_, text)| TextSize::of(text.as_str()))
                    .sum();
                TextRange::at(self.offset, len)
            }

            fn push_error(&mut self, error: RelationParseError, range: TextRange) {
                self.errors
                    .push(PositionedRelationParseError { error, range });
            }

            fn error(&mut self, error: RelationParseError) {
                self.push_error(error, self.next_range(1));
                self.builder.start_node(SyntaxKind::ERROR.into());
                if self.current().is_some() {
                    self.bump();
                }
                self.builder.finish_node();
            }

            fn parse_relation(&mut self) {
                self.builder.start_node(SyntaxKind::RELATION.into());
                if self.current() == Some(IDENT) {
                    self.bump();
                } else {
                    self.error(RelationParseError::MissingPackageName);
                }
                match self.peek_past_ws() {
                    Some(COMMA) => {}
                    None | Some(L_PARENS) => {
                        self.skip_ws();
                    }
                    _ => {
                        self.skip_ws();
                        self.unexpected();
                    }
                }

                if self.peek_past_ws() == Some(L_PARENS) {
                    self.skip_ws();
                    self.builder.start_node(VERSION.into());
                    self.bump();
                    self.skip_ws();

                    let operator: Vec<&str> = self
                        .tokens
                        .iter()
                        .rev()
                        .take_while(|(kind, _)| matches!(kind, L_ANGLE | R_ANGLE | EQUAL | NOT))
                        .map(|(_, text)| text.as_str())
                        .collect();
                    let operator_len = operator.len();
                    let operator = operator.concat();
                    if operator.is_empty() {
                        self.push_error(RelationParseError::MissingConstraint, self.next_range(0));
                    } else {
                        // Invalid operators are kept in an error node so that
                        // Relation::version() does not see them.
                        let kind = match operator.parse::<VersionConstraint>() {
                            Ok(_) => CONSTRAINT,
                            Err(e) => {
                                self.push_error(
                                    RelationParseError::InvalidConstraint(e),
                                    self.next_range(operator_len),
                                );
                                ERROR
                            }
                        };
                        self.builder.start_node(kind.into());
                        for _ in 0..operator_len {
                            self.bump();
                        }
                        self.builder.finish_node();
                    }

                    self.skip_ws();

                    match self.tokens.last() {
                        Some((IDENT, text)) => {
                            if let Err(e) = text.parse::<Version>() {
                                self.error(RelationParseError::InvalidVersion(e));
                            } else {
                                self.bump();
                            }
                        }
                        _ => self.error(RelationParseError::MissingVersion),
                    }

                    self.skip_ws();

                    if self.current() == Some(R_PARENS) {
                        self.bump();
                    } else {
                        match self.current() {
                            // Leave the comma so the next relation can still be parsed.
                            None | Some(COMMA) => self.push_error(
                                RelationParseError::MissingClosingParenthesis,
                                self.next_range(0),
                            ),
                            Some(_) => self.unexpected(),
                        }
                    }

                    self.builder.finish_node();
                }

                self.builder.finish_node();
            }

            fn parse(mut self) -> Parse {
                self.builder.start_node(SyntaxKind::ROOT.into());

                self.skip_ws();

                while self.current().is_some() {
                    match self.current() {
                        Some(IDENT) => self.parse_relation(),
                        Some(COMMA) => {
                            // Empty relation, but that's okay - probably?
                        }
                        _ => self.unexpected(),
                    }

                    self.skip_ws();
                    match self.current() {
                        Some(COMMA) => {
                            self.bump();
                        }
                        None => {
                            break;
                        }
                        _ => self.unexpected(),
                    }
                    self.skip_ws();
                }

                self.builder.finish_node();
                // Turn the builder into a GreenNode
                Parse {
                    green_node: self.builder.finish(),
                    errors: self.errors,
                }
            }
            /// Advance one token, adding it to the current branch of the tree builder.
            fn bump(&mut self) {
                let (kind, text) = self.tokens.pop().unwrap();
                self.offset += TextSize::of(text.as_str());
                self.builder.token(kind.into(), text.as_str());
            }
            /// Peek at the first unprocessed token
            fn current(&self) -> Option<SyntaxKind> {
                self.tokens.last().map(|(kind, _)| *kind)
            }
            fn skip_ws(&mut self) {
                while self.current() == Some(WHITESPACE) || self.current() == Some(NEWLINE) {
                    self.bump()
                }
            }

            fn peek_past_ws(&self) -> Option<SyntaxKind> {
                let mut i = self.tokens.len();
                while i > 0 {
                    i -= 1;
                    match self.tokens[i].0 {
                        WHITESPACE | NEWLINE => {}
                        _ => return Some(self.tokens[i].0),
                    }
                }
                None
            }
        }

        let mut tokens = crate::relations::lex(text);
        tokens.reverse();
        Parser {
            tokens,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            offset: TextSize::from(0),
        }
        .parse()
    }

    /// To work with the parse results we need a view into the
    /// green tree - the Syntax tree.
    /// It is also immutable, like a GreenNode,
    /// but it contains parent pointers, offsets, and
    /// has identity semantics.
    type SyntaxNode = rowan::SyntaxNode<Lang>;
    #[allow(unused)]
    type SyntaxToken = rowan::SyntaxToken<Lang>;
    #[allow(unused)]
    type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

    impl Parse {
        fn root_mut(&self) -> Relations {
            Relations::cast(SyntaxNode::new_root_mut(self.green_node.clone())).unwrap()
        }
    }

    macro_rules! ast_node {
        ($ast:ident, $kind:ident) => {
            /// A node in the syntax tree representing a $ast
            #[repr(transparent)]
            pub struct $ast(SyntaxNode);
            impl $ast {
                #[allow(unused)]
                fn cast(node: SyntaxNode) -> Option<Self> {
                    if node.kind() == $kind {
                        Some(Self(node))
                    } else {
                        None
                    }
                }
            }

            impl std::fmt::Display for $ast {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(&self.0.text().to_string())
                }
            }
        };
    }

    ast_node!(Relations, ROOT);
    ast_node!(Relation, RELATION);

    impl PartialEq for Relations {
        fn eq(&self, other: &Self) -> bool {
            self.relations().collect::<Vec<_>>() == other.relations().collect::<Vec<_>>()
        }
    }

    impl PartialEq for Relation {
        fn eq(&self, other: &Self) -> bool {
            self.name() == other.name() && self.version() == other.version()
        }
    }

    #[cfg(feature = "serde")]
    impl serde::Serialize for Relations {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let rep = self.to_string();
            serializer.serialize_str(&rep)
        }
    }

    #[cfg(feature = "serde")]
    impl<'de> serde::Deserialize<'de> for Relations {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let s = String::deserialize(deserializer)?;
            let relations = s.parse().map_err(serde::de::Error::custom)?;
            Ok(relations)
        }
    }

    impl std::fmt::Debug for Relations {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut s = f.debug_struct("Relations");

            for relation in self.relations() {
                s.field("relation", &relation);
            }

            s.finish()
        }
    }

    impl std::fmt::Debug for Relation {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut s = f.debug_struct("Relation");

            s.field("name", &self.name());

            if let Some((vc, version)) = self.version() {
                s.field("version", &vc);
                s.field("version", &version);
            }

            s.finish()
        }
    }

    #[cfg(feature = "serde")]
    impl serde::Serialize for Relation {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let rep = self.to_string();
            serializer.serialize_str(&rep)
        }
    }

    #[cfg(feature = "serde")]
    impl<'de> serde::Deserialize<'de> for Relation {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let s = String::deserialize(deserializer)?;
            let relation = s.parse().map_err(serde::de::Error::custom)?;
            Ok(relation)
        }
    }

    impl Default for Relations {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Relations {
        /// Create a new relations field
        pub fn new() -> Self {
            Self::from(vec![])
        }

        /// Wrap and sort this relations field
        #[must_use]
        pub fn wrap_and_sort(self) -> Self {
            let mut entries = self
                .relations()
                .map(|e| e.wrap_and_sort())
                .collect::<Vec<_>>();
            entries.sort();
            // TODO: preserve comments
            Self::from(entries)
        }

        /// Iterate over the entries in this relations field
        pub fn relations(&self) -> impl Iterator<Item = Relation> + '_ {
            self.0.children().filter_map(Relation::cast)
        }

        /// Iterate over the entries in this relations field
        pub fn iter(&self) -> impl Iterator<Item = Relation> + '_ {
            self.relations()
        }

        /// Remove the entry at the given index
        pub fn get_relation(&self, idx: usize) -> Option<Relation> {
            self.relations().nth(idx)
        }

        /// Remove the relation at the given index
        pub fn remove_relation(&mut self, idx: usize) -> Relation {
            let mut relation = self.get_relation(idx).unwrap();
            relation.remove();
            relation
        }

        /// Insert a new relation at the given index
        pub fn insert(&mut self, idx: usize, relation: Relation) {
            let is_empty = !self.0.children_with_tokens().any(|n| n.kind() == COMMA);
            let (position, new_children) = if let Some(current_relation) = self.relations().nth(idx)
            {
                let to_insert: Vec<NodeOrToken<GreenNode, GreenToken>> = if idx == 0 && is_empty {
                    vec![relation.0.green().into()]
                } else {
                    vec![
                        relation.0.green().into(),
                        NodeOrToken::Token(GreenToken::new(COMMA.into(), ",")),
                        NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), " ")),
                    ]
                };

                (current_relation.0.index(), to_insert)
            } else {
                let child_count = self.0.children_with_tokens().count();
                (
                    child_count,
                    if idx == 0 {
                        vec![relation.0.green().into()]
                    } else {
                        vec![
                            NodeOrToken::Token(GreenToken::new(COMMA.into(), ",")),
                            NodeOrToken::Token(GreenToken::new(WHITESPACE.into(), " ")),
                            relation.0.green().into(),
                        ]
                    },
                )
            };
            // We can safely replace the root here since Relations is a root node
            self.0 = SyntaxNode::new_root_mut(
                self.0.replace_with(
                    self.0
                        .green()
                        .splice_children(position..position, new_children),
                ),
            );
        }

        /// Replace the relation at the given index
        pub fn replace(&mut self, idx: usize, relation: Relation) {
            let current_relation = self.get_relation(idx).unwrap();
            self.0.splice_children(
                current_relation.0.index()..current_relation.0.index() + 1,
                vec![relation.0.into()],
            );
        }

        /// Push a new relation to the relations field
        pub fn push(&mut self, relation: Relation) {
            let pos = self.relations().count();
            self.insert(pos, relation);
        }

        /// Parse a relations field from a string, allowing syntax errors
        pub fn parse_relaxed(s: &str) -> (Relations, Vec<PositionedRelationParseError>) {
            let parse = parse(s);
            (parse.root_mut(), parse.errors)
        }

        /// Check if this relations field is satisfied by the given package versions.
        pub fn satisfied_by(
            &self,
            package_version: impl crate::relations::VersionLookup + Copy,
        ) -> bool {
            self.relations().all(|e| e.satisfied_by(package_version))
        }

        /// Check if this relations field is empty
        pub fn is_empty(&self) -> bool {
            self.relations().count() == 0
        }

        /// Get the number of entries in this relations field
        pub fn len(&self) -> usize {
            self.relations().count()
        }
    }

    impl From<Vec<Relation>> for Relations {
        fn from(entries: Vec<Relation>) -> Self {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(ROOT.into());
            for (i, relation) in entries.into_iter().enumerate() {
                if i > 0 {
                    builder.token(COMMA.into(), ",");
                    builder.token(WHITESPACE.into(), " ");
                }
                inject(&mut builder, relation.0);
            }
            builder.finish_node();
            Relations(SyntaxNode::new_root_mut(builder.finish()))
        }
    }

    impl From<Relation> for Relations {
        fn from(relation: Relation) -> Self {
            Self::from(vec![relation])
        }
    }

    impl From<Relation> for crate::lossy::Relation {
        fn from(relation: Relation) -> Self {
            let mut rel = crate::lossy::Relation::new();
            rel.name = relation.name();
            rel.version = relation.version();
            rel
        }
    }

    impl From<Relations> for crate::lossy::Relations {
        fn from(relations: Relations) -> Self {
            let mut rels = crate::lossy::Relations::new();
            for relation in relations.relations() {
                rels.0.push(relation.into());
            }
            rels
        }
    }

    impl From<crate::lossy::Relations> for Relations {
        fn from(relations: crate::lossy::Relations) -> Self {
            let mut entries = vec![];
            for relation in relations.iter() {
                entries.push(relation.clone().into());
            }
            Self::from(entries)
        }
    }

    impl From<crate::lossy::Relation> for Relation {
        fn from(relation: crate::lossy::Relation) -> Self {
            Relation::new(&relation.name, relation.version)
        }
    }

    fn inject(builder: &mut GreenNodeBuilder, node: SyntaxNode) {
        builder.start_node(node.kind().into());
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(child) => {
                    inject(builder, child);
                }
                rowan::NodeOrToken::Token(token) => {
                    builder.token(token.kind().into(), token.text());
                }
            }
        }
        builder.finish_node();
    }

    impl Relation {
        /// Create a new relation
        ///
        /// # Arguments
        /// * `name` - The name of the package
        /// * `version_constraint` - The version constraint and version to use
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::{Relation};
        /// use r_description::VersionConstraint;
        /// let relation = Relation::new("vign", Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
        /// assert_eq!(relation.to_string(), "vign (>= 2.0)");
        /// ```
        pub fn new(name: &str, version_constraint: Option<(VersionConstraint, Version)>) -> Self {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(SyntaxKind::RELATION.into());
            builder.token(IDENT.into(), name);
            if let Some((vc, version)) = version_constraint {
                builder.token(WHITESPACE.into(), " ");
                builder.start_node(SyntaxKind::VERSION.into());
                builder.token(L_PARENS.into(), "(");
                builder.start_node(SyntaxKind::CONSTRAINT.into());
                push_constraint_tokens(&mut builder, &vc);
                builder.finish_node();

                builder.token(WHITESPACE.into(), " ");

                builder.token(IDENT.into(), version.to_string().as_str());

                builder.token(R_PARENS.into(), ")");

                builder.finish_node();
            }

            builder.finish_node();
            Relation(SyntaxNode::new_root_mut(builder.finish()))
        }

        /// Wrap and sort this relation
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::Relation;
        /// let relation = "  vign  (  >= 2.0) ".parse::<Relation>().unwrap();
        /// assert_eq!(relation.wrap_and_sort().to_string(), "vign (>= 2.0)");
        /// ```
        #[must_use]
        pub fn wrap_and_sort(&self) -> Self {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(SyntaxKind::RELATION.into());
            builder.token(IDENT.into(), self.name().as_str());
            if let Some((vc, version)) = self.version() {
                builder.token(WHITESPACE.into(), " ");
                builder.start_node(SyntaxKind::VERSION.into());
                builder.token(L_PARENS.into(), "(");
                builder.start_node(SyntaxKind::CONSTRAINT.into());
                push_constraint_tokens(&mut builder, &vc);
                builder.finish_node();
                builder.token(WHITESPACE.into(), " ");
                builder.token(IDENT.into(), version.to_string().as_str());
                builder.token(R_PARENS.into(), ")");
                builder.finish_node();
            }
            builder.finish_node();
            Relation(SyntaxNode::new_root_mut(builder.finish()))
        }

        /// Create a new simple relation, without any version constraints.
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::Relation;
        /// let relation = Relation::simple("vign");
        /// assert_eq!(relation.to_string(), "vign");
        /// ```
        pub fn simple(name: &str) -> Self {
            Self::new(name, None)
        }

        /// Remove the version constraint from the relation.
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::{Relation};
        /// use r_description::VersionConstraint;
        /// let mut relation = Relation::new("vign", Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
        /// relation.drop_constraint();
        /// assert_eq!(relation.to_string(), "vign");
        /// ```
        pub fn drop_constraint(&mut self) -> bool {
            let version_token = self.0.children().find(|n| n.kind() == VERSION);
            if let Some(version_token) = version_token {
                // Remove any whitespace before the version token
                while let Some(prev) = version_token.prev_sibling_or_token() {
                    if prev.kind() == WHITESPACE || prev.kind() == NEWLINE {
                        prev.detach();
                    } else {
                        break;
                    }
                }
                version_token.detach();
                return true;
            }

            false
        }

        /// Return the name of the package in the relation.
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::Relation;
        /// let relation = Relation::simple("vign");
        /// assert_eq!(relation.name(), "vign");
        /// ```
        pub fn name(&self) -> String {
            self.0
                .children_with_tokens()
                .find_map(|it| match it {
                    SyntaxElement::Token(token) if token.kind() == IDENT => Some(token),
                    _ => None,
                })
                .unwrap()
                .text()
                .to_string()
        }

        /// Return the version constraint and the version it is constrained to.
        pub fn version(&self) -> Option<(VersionConstraint, Version)> {
            let vc = self.0.children().find(|n| n.kind() == VERSION);
            let vc = vc.as_ref()?;
            let constraint = vc.children().find(|n| n.kind() == CONSTRAINT);

            let version = vc.children_with_tokens().find_map(|it| match it {
                SyntaxElement::Token(token) if token.kind() == IDENT => Some(token),
                _ => None,
            });

            if let (Some(constraint), Some(version)) = (constraint, version) {
                let vc: VersionConstraint = constraint.to_string().parse().unwrap();
                Some((vc, (version.text().to_string()).parse().unwrap()))
            } else {
                None
            }
        }

        /// Set the version constraint for this relation
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::{Relation};
        /// use r_description::VersionConstraint;
        /// let mut relation = Relation::simple("vign");
        /// relation.set_version(Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())));
        /// assert_eq!(relation.to_string(), "vign (>= 2.0)");
        /// ```
        pub fn set_version(&mut self, version_constraint: Option<(VersionConstraint, Version)>) {
            let current_version = self.0.children().find(|n| n.kind() == VERSION);
            if let Some((vc, version)) = version_constraint {
                let mut builder = GreenNodeBuilder::new();
                builder.start_node(VERSION.into());
                builder.token(L_PARENS.into(), "(");
                builder.start_node(CONSTRAINT.into());
                push_constraint_tokens(&mut builder, &vc);
                builder.finish_node(); // CONSTRAINT
                builder.token(WHITESPACE.into(), " ");
                builder.token(IDENT.into(), version.to_string().as_str());
                builder.token(R_PARENS.into(), ")");
                builder.finish_node(); // VERSION

                if let Some(current_version) = current_version {
                    self.0.splice_children(
                        current_version.index()..current_version.index() + 1,
                        vec![SyntaxNode::new_root_mut(builder.finish()).into()],
                    );
                } else {
                    let name_node = self.0.children_with_tokens().find(|n| n.kind() == IDENT);
                    let idx = if let Some(name_node) = name_node {
                        name_node.index() + 1
                    } else {
                        0
                    };
                    let new_children = vec![
                        GreenToken::new(WHITESPACE.into(), " ").into(),
                        builder.finish().into(),
                    ];
                    let new_root = SyntaxNode::new_root_mut(
                        self.0.green().splice_children(idx..idx, new_children),
                    );
                    if let Some(parent) = self.0.parent() {
                        parent.splice_children(
                            self.0.index()..self.0.index() + 1,
                            vec![new_root.into()],
                        );
                        self.0 = parent
                            .children_with_tokens()
                            .nth(self.0.index())
                            .unwrap()
                            .clone()
                            .into_node()
                            .unwrap();
                    } else {
                        self.0 = new_root;
                    }
                }
            } else if let Some(current_version) = current_version {
                // Remove any whitespace before the version token
                while let Some(prev) = current_version.prev_sibling_or_token() {
                    if prev.kind() == WHITESPACE || prev.kind() == NEWLINE {
                        prev.detach();
                    } else {
                        break;
                    }
                }
                current_version.detach();
            }
        }

        /// Remove this relation
        ///
        /// # Example
        /// ```
        /// use r_description::lossless::{Relation, Relations};
        /// let mut relations: Relations = r"cli (>= 0.19.0), blah (< 1.26.0)".parse().unwrap();
        /// let mut relation = relations.get_relation(0).unwrap();
        /// assert_eq!(relation.to_string(), "cli (>= 0.19.0)");
        /// relation.remove();
        /// assert_eq!(relations.to_string(), "blah (< 1.26.0)");
        /// ```
        pub fn remove(&mut self) {
            let is_first = !self
                .0
                .siblings(Direction::Prev)
                .skip(1)
                .any(|n| n.kind() == RELATION);
            if !is_first {
                // Not the first item in the list. Remove whitespace backwards to the previous
                // pipe, the pipe and any whitespace until the previous relation
                while let Some(n) = self.0.prev_sibling_or_token() {
                    if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                        n.detach();
                    } else if n.kind() == COMMA {
                        n.detach();
                        break;
                    } else {
                        break;
                    }
                }
                while let Some(n) = self.0.prev_sibling_or_token() {
                    if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                        n.detach();
                    } else {
                        break;
                    }
                }
            } else {
                // First item in the list. Remove whitespace up to the pipe, the pipe and anything
                // before the next relation
                while let Some(n) = self.0.next_sibling_or_token() {
                    if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                        n.detach();
                    } else if n.kind() == COMMA {
                        n.detach();
                        break;
                    } else {
                        panic!("Unexpected node: {n:?}");
                    }
                }

                while let Some(n) = self.0.next_sibling_or_token() {
                    if n.kind() == WHITESPACE || n.kind() == NEWLINE {
                        n.detach();
                    } else {
                        break;
                    }
                }
            }
            self.0.detach();
        }

        /// Check if this relation is satisfied by the given package version.
        pub fn satisfied_by(
            &self,
            package_version: impl crate::relations::VersionLookup + Copy,
        ) -> bool {
            let name = self.name();
            let version = self.version();
            if let Some(version) = version {
                if let Some(package_version) = package_version.lookup_version(&name) {
                    let required = version.1;
                    let actual = package_version.into_owned();
                    match version.0 {
                        VersionConstraint::GreaterThanEqual => actual >= required,
                        VersionConstraint::LessThanEqual => actual <= required,
                        VersionConstraint::Equal => actual == required,
                        VersionConstraint::NotEqual => actual != required,
                        VersionConstraint::GreaterThan => actual > required,
                        VersionConstraint::LessThan => actual < required,
                    }
                } else {
                    false
                }
            } else {
                true
            }
        }
    }

    impl PartialOrd for Relation {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Eq for Relation {}

    impl Ord for Relation {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            // Compare by name first, then by version
            let name_cmp = self.name().cmp(&other.name());
            if name_cmp != std::cmp::Ordering::Equal {
                return name_cmp;
            }

            let self_version = self.version();
            let other_version = other.version();

            match (self_version, other_version) {
                (Some((self_vc, self_version)), Some((other_vc, other_version))) => {
                    let vc_cmp = self_vc.cmp(&other_vc);
                    if vc_cmp != std::cmp::Ordering::Equal {
                        return vc_cmp;
                    }

                    self_version.cmp(&other_version)
                }
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            }
        }
    }

    impl std::str::FromStr for Relations {
        type Err = ParseError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let parse = parse(s);
            if parse.errors.is_empty() {
                Ok(parse.root_mut())
            } else {
                Err(ParseError(parse.errors))
            }
        }
    }

    impl std::str::FromStr for Relation {
        type Err = ParseError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let rels = s.parse::<Relations>()?;
            let mut relations = rels.relations();

            let relation = if let Some(relation) = relations.next() {
                relation
            } else {
                return Err(ParseError(vec![PositionedRelationParseError {
                    error: RelationParseError::NoRelation,
                    range: TextRange::up_to(TextSize::of(s)),
                }]));
            };

            if relations.next().is_some() {
                return Err(ParseError(vec![PositionedRelationParseError {
                    error: RelationParseError::MultipleRelations,
                    range: TextRange::up_to(TextSize::of(s)),
                }]));
            }

            Ok(relation)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::version::VersionParseError;

        /// Pair each error with the text it points at.
        fn located<'a>(
            input: &'a str,
            errors: &[PositionedRelationParseError],
        ) -> Vec<(RelationParseError, &'a str)> {
            errors
                .iter()
                .map(|e| (e.error.clone(), &input[e.range]))
                .collect()
        }

        #[test]
        fn test_parse() {
            let input = "cli";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.relations().count(), 1);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(relation.to_string(), "cli");
            assert_eq!(relation.version(), None);

            let input = "cli (>= 0.20.21)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.relations().count(), 1);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(relation.to_string(), "cli (>= 0.20.21)");
            assert_eq!(
                relation.version(),
                Some((
                    VersionConstraint::GreaterThanEqual,
                    "0.20.21".parse().unwrap()
                ))
            );

            let input = "xml2 (> 1.0.0)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(
                relation.version(),
                Some((VersionConstraint::GreaterThan, "1.0.0".parse().unwrap()))
            );

            let input = "xml2 (< 2.0.0)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(
                relation.version(),
                Some((VersionConstraint::LessThan, "2.0.0".parse().unwrap()))
            );
        }

        #[test]
        fn test_r_equality_operators() {
            let input = "xml2 (== 1.0.0)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(
                relation.version(),
                Some((VersionConstraint::Equal, "1.0.0".parse().unwrap()))
            );

            let input = "xml2 (!= 1.0.0)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(
                relation.version(),
                Some((VersionConstraint::NotEqual, "1.0.0".parse().unwrap()))
            );
        }

        #[test]
        fn test_constructed_relations_use_r_operators() {
            let cases = [
                (VersionConstraint::LessThan, "xml2 (< 1.0)"),
                (VersionConstraint::LessThanEqual, "xml2 (<= 1.0)"),
                (VersionConstraint::Equal, "xml2 (== 1.0)"),
                (VersionConstraint::NotEqual, "xml2 (!= 1.0)"),
                (VersionConstraint::GreaterThan, "xml2 (> 1.0)"),
                (VersionConstraint::GreaterThanEqual, "xml2 (>= 1.0)"),
            ];
            for (vc, expected) in cases {
                let relation = Relation::new("xml2", Some((vc, "1.0".parse().unwrap())));
                assert_eq!(relation.to_string(), expected);
            }
        }

        #[test]
        fn test_multiple() {
            let input = "cli (>= 0.20.21), cli (< 0.21)";
            let parsed: Relations = input.parse().unwrap();
            assert_eq!(parsed.to_string(), input);
            assert_eq!(parsed.relations().count(), 2);
            let relation = parsed.relations().next().unwrap();
            assert_eq!(relation.to_string(), "cli (>= 0.20.21)");
            assert_eq!(
                relation.version(),
                Some((
                    VersionConstraint::GreaterThanEqual,
                    "0.20.21".parse().unwrap()
                ))
            );
            let relation = parsed.relations().nth(1).unwrap();
            assert_eq!(relation.to_string(), "cli (< 0.21)");
            assert_eq!(
                relation.version(),
                Some((VersionConstraint::LessThan, "0.21".parse().unwrap()))
            );
        }

        #[test]
        fn test_new() {
            let r = Relation::new(
                "cli",
                Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())),
            );

            assert_eq!(r.to_string(), "cli (>= 2.0)");
        }

        #[test]
        fn test_drop_constraint() {
            let mut r = Relation::new(
                "cli",
                Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap())),
            );

            r.drop_constraint();

            assert_eq!(r.to_string(), "cli");
        }

        #[test]
        fn test_simple() {
            let r = Relation::simple("cli");

            assert_eq!(r.to_string(), "cli");
        }

        #[test]
        fn test_remove_first_relation() {
            let mut rels: Relations = r#"cli (>= 0.20.21), cli (< 0.21)"#.parse().unwrap();
            let removed = rels.remove_relation(0);
            assert_eq!(removed.to_string(), "cli (>= 0.20.21)");
            assert_eq!(rels.to_string(), "cli (< 0.21)");
        }

        #[test]
        fn test_remove_last_relation() {
            let mut rels: Relations = r#"cli (>= 0.20.21), cli (< 0.21)"#.parse().unwrap();
            rels.remove_relation(1);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21)");
        }

        #[test]
        fn test_remove_middle() {
            let mut rels: Relations =
                r#"cli (>= 0.20.21), cli (< 0.21), cli (< 0.22)"#.parse().unwrap();
            rels.remove_relation(1);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21), cli (< 0.22)");
        }

        #[test]
        fn test_remove_added() {
            let mut rels: Relations = r#"cli (>= 0.20.21)"#.parse().unwrap();
            let relation = Relation::simple("cli");
            rels.push(relation);
            rels.remove_relation(1);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21)");
        }

        #[test]
        fn test_push() {
            let mut rels: Relations = r#"cli (>= 0.20.21)"#.parse().unwrap();
            let relation = Relation::simple("cli");
            rels.push(relation);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21), cli");
        }

        #[test]
        fn test_push_from_empty() {
            let mut rels: Relations = "".parse().unwrap();
            let relation = Relation::simple("cli");
            rels.push(relation);
            assert_eq!(rels.to_string(), "cli");
        }

        #[test]
        fn test_insert() {
            let mut rels: Relations = r#"cli (>= 0.20.21), cli (< 0.21)"#.parse().unwrap();
            let relation = Relation::simple("cli");
            rels.insert(1, relation);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21), cli, cli (< 0.21)");
        }

        #[test]
        fn test_insert_at_start() {
            let mut rels: Relations = r#"cli (>= 0.20.21), cli (< 0.21)"#.parse().unwrap();
            let relation = Relation::simple("cli");
            rels.insert(0, relation);
            assert_eq!(rels.to_string(), "cli, cli (>= 0.20.21), cli (< 0.21)");
        }

        #[test]
        fn test_insert_after_error() {
            let (mut rels, errors) = Relations::parse_relaxed("@foo@, debhelper (>= 1.0)");
            assert_eq!(
                errors
                    .iter()
                    .map(|e| (e.error.clone(), e.range))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        RelationParseError::UnexpectedToken("@".to_string()),
                        TextRange::new(0.into(), 1.into())
                    ),
                    (
                        RelationParseError::UnexpectedToken("foo".to_string()),
                        TextRange::new(1.into(), 4.into())
                    ),
                    (
                        RelationParseError::UnexpectedToken("@".to_string()),
                        TextRange::new(4.into(), 5.into())
                    ),
                ]
            );
            let relation = Relation::simple("bar");
            rels.push(relation);
            assert_eq!(rels.to_string(), "@foo@, debhelper (>= 1.0), bar");
        }

        #[test]
        fn test_insert_before_error() {
            let (mut rels, errors) = Relations::parse_relaxed("debhelper (>= 1.0), @foo@, bla");
            assert_eq!(
                errors
                    .iter()
                    .map(|e| (e.error.clone(), e.range))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        RelationParseError::UnexpectedToken("@".to_string()),
                        TextRange::new(20.into(), 21.into())
                    ),
                    (
                        RelationParseError::UnexpectedToken("foo".to_string()),
                        TextRange::new(21.into(), 24.into())
                    ),
                    (
                        RelationParseError::UnexpectedToken("@".to_string()),
                        TextRange::new(24.into(), 25.into())
                    ),
                ]
            );
            let relation = Relation::simple("bar");
            rels.insert(0, relation);
            assert_eq!(rels.to_string(), "bar, debhelper (>= 1.0), @foo@, bla");
        }

        #[test]
        fn test_invalid_constraint() {
            let invalid = RelationParseError::InvalidConstraint(
                crate::VersionConstraintParseError(">>=".to_string()),
            );
            let input = "foo (>>= 1.0)";
            assert_eq!(
                located(input, input.parse::<Relations>().unwrap_err().errors()),
                vec![(invalid.clone(), ">>=")]
            );
            let input = "foo (>>= 1.0), bar (>= 2.0)";
            let (rels, errors) = Relations::parse_relaxed(input);
            assert_eq!(located(input, &errors), vec![(invalid, ">>=")]);
            assert_eq!(rels.to_string(), "foo (>>= 1.0), bar (>= 2.0)");
            let versions: Vec<_> = rels.relations().map(|r| r.version()).collect();
            assert_eq!(
                versions,
                vec![
                    None,
                    Some((VersionConstraint::GreaterThanEqual, "2.0".parse().unwrap()))
                ]
            );
        }

        #[test]
        fn test_whitespace_before_closing_parenthesis() {
            let rels: Relations = "foo (>= 1.0 )".parse().unwrap();
            assert_eq!(
                rels.relations().next().unwrap().version(),
                Some((VersionConstraint::GreaterThanEqual, "1.0".parse().unwrap()))
            );
        }

        #[test]
        fn test_missing_closing_parenthesis() {
            let err = "foo (>= 1.0".parse::<Relations>().unwrap_err();
            assert_eq!(
                err.errors(),
                [PositionedRelationParseError {
                    error: RelationParseError::MissingClosingParenthesis,
                    range: TextRange::empty(11.into()),
                }]
            );
            let input = "foo (>= 1.0 bar)";
            assert_eq!(
                located(input, input.parse::<Relations>().unwrap_err().errors())[0],
                (
                    RelationParseError::UnexpectedToken("bar".to_string()),
                    "bar"
                )
            );
        }

        #[test]
        fn test_missing_constraint() {
            assert_eq!(
                "foo (1.0)".parse::<Relations>().unwrap_err().errors(),
                [PositionedRelationParseError {
                    error: RelationParseError::MissingConstraint,
                    range: TextRange::empty(5.into()),
                }]
            );
            let (rels, _) = Relations::parse_relaxed("foo (1.0)");
            assert_eq!(rels.relations().next().unwrap().version(), None);
        }

        #[test]
        fn test_invalid_version() {
            let invalid = RelationParseError::InvalidVersion(VersionParseError::InvalidComponent(
                "beta".to_string(),
            ));
            let input = "foo (>= 1.0-beta)";
            let err = input.parse::<Relations>().unwrap_err();
            assert_eq!(
                located(input, err.errors()),
                vec![(invalid.clone(), "1.0-beta")]
            );
            assert_eq!(err.to_string(), "invalid version component \"beta\"");
            let (rels, errors) = Relations::parse_relaxed(input);
            assert_eq!(located(input, &errors), vec![(invalid, "1.0-beta")]);
            assert_eq!(rels.to_string(), "foo (>= 1.0-beta)");
            assert_eq!(rels.relations().next().unwrap().version(), None);
        }

        #[test]
        fn test_replace() {
            let mut rels: Relations = r#"cli (>= 0.20.21), cli (< 0.21)"#.parse().unwrap();
            let relation = Relation::simple("cli");
            rels.replace(1, relation);
            assert_eq!(rels.to_string(), "cli (>= 0.20.21), cli");
        }

        #[test]
        fn test_parse_relation() {
            let parsed: Relation = "cli (>= 0.20.21)".parse().unwrap();
            assert_eq!(parsed.to_string(), "cli (>= 0.20.21)");
            assert_eq!(
                parsed.version(),
                Some((
                    VersionConstraint::GreaterThanEqual,
                    "0.20.21".parse().unwrap()
                ))
            );
            assert_eq!(
                located(
                    "foo, bar",
                    "foo, bar".parse::<Relation>().unwrap_err().errors()
                ),
                vec![(RelationParseError::MultipleRelations, "foo, bar")]
            );
            assert_eq!(
                located("", "".parse::<Relation>().unwrap_err().errors()),
                vec![(RelationParseError::NoRelation, "")]
            );
        }

        #[test]
        fn test_parse_relation_preserves_version_separators() {
            let parsed: Relation = "cli (>= 2.5-1)".parse().unwrap();
            let (_, version) = parsed.version().unwrap();

            assert_eq!(version.to_string(), "2.5-1");
            assert_eq!(parsed.wrap_and_sort().to_string(), "cli (>= 2.5-1)");
            assert_eq!(version, "2.5.1".parse::<Version>().unwrap());
        }

        #[test]
        fn test_relations_satisfied_by() {
            let rels: Relations = "cli (>= 0.20.21), cli (< 0.21)".parse().unwrap();
            let satisfied = |name: &str| -> Option<Version> {
                match name {
                    "cli" => Some("0.20.21".parse().unwrap()),
                    _ => None,
                }
            };
            assert!(rels.satisfied_by(satisfied));

            let satisfied = |name: &str| match name {
                "cli" => Some("0.21".parse().unwrap()),
                _ => None,
            };
            assert!(!rels.satisfied_by(satisfied));

            let satisfied = |name: &str| match name {
                "cli" => Some("0.20.20".parse().unwrap()),
                _ => None,
            };
            assert!(!rels.satisfied_by(satisfied));
        }

        #[test]
        fn test_wrap_and_sort_relation() {
            let relation: Relation = "   cli   (>=   11.0)".parse().unwrap();

            let wrapped = relation.wrap_and_sort();

            assert_eq!(wrapped.to_string(), "cli (>= 11.0)");
        }

        #[test]
        fn test_wrap_and_sort_relations() {
            let relations: Relations = "cli (>= 0.20.21)  , \n\n\n\ncli (< 0.21)".parse().unwrap();

            let wrapped = relations.wrap_and_sort();

            assert_eq!(wrapped.to_string(), "cli (< 0.21), cli (>= 0.20.21)");
        }

        #[cfg(feature = "serde")]
        #[test]
        fn test_serialize_relations() {
            let relations: Relations = "cli (>= 0.20.21), cli (< 0.21)".parse().unwrap();
            let serialized = serde_json::to_string(&relations).unwrap();
            assert_eq!(serialized, r#""cli (>= 0.20.21), cli (< 0.21)""#);
        }

        #[cfg(feature = "serde")]
        #[test]
        fn test_deserialize_relations() {
            let relations: Relations = "cli (>= 0.20.21), cli (< 0.21)".parse().unwrap();
            let serialized = serde_json::to_string(&relations).unwrap();
            let deserialized: Relations = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized.to_string(), relations.to_string());
        }

        #[cfg(feature = "serde")]
        #[test]
        fn test_serialize_relation() {
            let relation: Relation = "cli (>= 0.20.21)".parse().unwrap();
            let serialized = serde_json::to_string(&relation).unwrap();
            assert_eq!(serialized, r#""cli (>= 0.20.21)""#);
        }

        #[cfg(feature = "serde")]
        #[test]
        fn test_deserialize_relation() {
            let relation: Relation = "cli (>= 0.20.21)".parse().unwrap();
            let serialized = serde_json::to_string(&relation).unwrap();
            let deserialized: Relation = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized.to_string(), relation.to_string());
        }

        #[test]
        fn test_relation_set_version() {
            let mut rel: Relation = "vign".parse().unwrap();
            rel.set_version(None);
            assert_eq!("vign", rel.to_string());
            rel.set_version(Some((
                VersionConstraint::GreaterThanEqual,
                "2.0".parse().unwrap(),
            )));
            assert_eq!("vign (>= 2.0)", rel.to_string());
            rel.set_version(None);
            assert_eq!("vign", rel.to_string());
            rel.set_version(Some((
                VersionConstraint::GreaterThanEqual,
                "2.0".parse().unwrap(),
            )));
            rel.set_version(Some((
                VersionConstraint::GreaterThanEqual,
                "1.1".parse().unwrap(),
            )));
            assert_eq!("vign (>= 1.1)", rel.to_string());
        }

        #[test]
        fn test_wrap_and_sort_removes_empty_entries() {
            let relations: Relations = "foo, , bar, ".parse().unwrap();
            let wrapped = relations.wrap_and_sort();
            assert_eq!(wrapped.to_string(), "bar, foo");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RelationParseError;

    #[test]
    fn test_parse() {
        let s = r###"Package: mypackage
Title: What the Package Does (One Line, Title Case)
Version: 0.0.0.9000
Authors@R:
    person("First", "Last", , "first.last@example.com", role = c("aut", "cre"),
           comment = c(ORCID = "YOUR-ORCID-ID"))
Description: What the package does (one paragraph).
License: `use_mit_license()`, `use_gpl3_license()` or friends to pick a
    license
Encoding: UTF-8
Roxygen: list(markdown = TRUE)
RoxygenNote: 7.3.2
"###;
        let desc: RDescription = s.parse().unwrap();

        assert_eq!(desc.package(), Some("mypackage".to_string()));
        assert_eq!(
            desc.title(),
            Some("What the Package Does (One Line, Title Case)".to_string())
        );
        assert_eq!(desc.version().unwrap().unwrap().as_str(), "0.0.0.9000");
        assert_eq!(
            desc.authors(),
            Some(RCode(
                r#"person("First", "Last", , "first.last@example.com", role = c("aut", "cre"),
comment = c(ORCID = "YOUR-ORCID-ID"))"#
                    .to_string()
            ))
        );
        assert_eq!(
            desc.description(),
            Some("What the package does (one paragraph).".to_string())
        );
        assert_eq!(
            desc.license(),
            Some(
                "`use_mit_license()`, `use_gpl3_license()` or friends to pick a\nlicense"
                    .to_string()
            )
        );
        assert_eq!(desc.encoding(), Some("UTF-8".to_string()));
        assert_eq!(desc.roxygen(), Some("list(markdown = TRUE)".to_string()));
        assert_eq!(desc.roxygen_note(), Some("7.3.2".to_string()));

        assert_eq!(desc.to_string(), s);
    }

    #[test]
    fn test_setters_replace_existing_fields() {
        let s = r###"Package: mypackage
Title: What the Package Does (One Line, Title Case)
Version: 0.0.0.9000
Description: What the package does (one paragraph).
Encoding: UTF-8
"###;
        let mut desc: RDescription = s.parse().unwrap();

        desc.set_version(&"0.0.1".parse().unwrap());

        // the field is updated in place rather than appended a second time
        assert_eq!(desc.version().unwrap().unwrap().as_str(), "0.0.1");
        assert_eq!(desc.to_string().matches("Version:").count(), 1);

        assert_eq!(
            desc.to_string(),
            r###"Package: mypackage
Title: What the Package Does (One Line, Title Case)
Version: 0.0.1
Description: What the package does (one paragraph).
Encoding: UTF-8
"###
        );
    }

    #[test]
    fn test_setters_add_missing_fields() {
        let s = "Package: mypackage\n";
        let mut desc: RDescription = s.parse().unwrap();

        desc.set_version(&"0.0.1".parse().unwrap());

        assert_eq!(desc.version().unwrap().unwrap().as_str(), "0.0.1");
        assert_eq!(desc.to_string(), "Package: mypackage\nVersion: 0.0.1\n");
    }

    #[test]
    fn test_parse_dplyr() {
        let s = include_str!("../testdata/dplyr.desc");

        let desc: RDescription = s.parse().unwrap();
        assert_eq!("dplyr", desc.package().unwrap());
        assert_eq!(
            desc.urls(),
            Ok(Some(vec![
                "https://dplyr.tidyverse.org/".parse().unwrap(),
                "https://github.com/tidyverse/dplyr".parse().unwrap(),
            ]))
        );
    }

    #[test]
    fn test_dependency_style_fields_return_relations() {
        let s = r###"Package: mypackage
Title: What the Package Does
Version: 1.0.0
Description: What the package does.
License: MIT
Imports: cli (>= 2.5-1), glue
Suggests: testthat (>= 3.0.0)
Depends: R (>= 4.1.0)
LinkingTo: cpp11 (>= 0.4-1)
Enhances: shiny
"###;
        let desc: RDescription = s.parse().unwrap();

        let imports = desc.imports().unwrap().unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(
            imports
                .iter()
                .next()
                .unwrap()
                .version()
                .unwrap()
                .1
                .to_string(),
            "2.5-1"
        );
        assert_eq!(desc.suggests().unwrap().unwrap().len(), 1);
        assert_eq!(desc.depends().unwrap().unwrap().len(), 1);
        assert_eq!(
            desc.linking_to()
                .unwrap()
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .version()
                .unwrap()
                .1
                .to_string(),
            "0.4-1"
        );
        assert_eq!(
            desc.enhances()
                .unwrap()
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .name(),
            "shiny"
        );
    }

    #[test]
    fn test_preserves_comments() {
        let s = "# comment\nPackage: foo\n";
        let mut desc: RDescription = s.parse().unwrap();
        assert_eq!(desc.to_string(), s);
        desc.set_title("Foo");
        assert_eq!(desc.to_string(), "# comment\nPackage: foo\nTitle: Foo\n");
    }

    #[test]
    fn test_empty() {
        let mut desc: RDescription = "".parse().unwrap();
        assert_eq!(desc.package(), None);
        assert_eq!(desc.to_string(), "");
        desc.set_package("foo");
        assert_eq!(desc.to_string(), "Package: foo\n");
    }

    #[test]
    fn test_multiple_paragraphs() {
        let s = "Package: a\n\nPackage: b\n";
        assert!(matches!(s.parse::<RDescription>(), Err(Error::Parse(_))));

        let (desc, errors) = RDescription::from_str_relaxed(s);
        assert_eq!(desc.package(), Some("a".to_string()));
        assert_eq!(desc.to_string(), s);
        assert_eq!(
            errors,
            vec![PositionedParseError {
                message: "unexpected additional paragraph".to_string(),
                range: TextRange::new(12.into(), 23.into()),
                code: Some("multiple-paragraphs".to_string()),
            }]
        );
    }

    #[test]
    fn test_from_str_relaxed() {
        let s = "Package foo\nVersion: 1.0\nTitle: Foo\n";
        let err = s.parse::<RDescription>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "Parse error: missing colon ':' after field name\n"
        );

        let (desc, errors) = RDescription::from_str_relaxed(s);
        assert_eq!(desc.version().unwrap().unwrap().as_str(), "1.0");
        assert_eq!(desc.title(), Some("Foo".to_string()));
        assert_eq!(desc.to_string(), s);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "missing colon ':' after field name");
        assert_eq!(&s[errors[0].range], "foo");
    }

    #[test]
    fn test_ranges() {
        let s = "# comment\nPackage: foo\nImports:\n    cli,\n    glue\n";
        let desc: RDescription = s.parse().unwrap();
        assert_eq!(&s[desc.field_range("Package").unwrap()], "Package: foo\n");
        assert_eq!(&s[desc.value_range("Package").unwrap()], "foo");
        assert_eq!(&s[desc.value_range("Imports").unwrap()], "cli,\n    glue");
        assert_eq!(desc.field_range("Title"), None);
        assert_eq!(desc.value_range("Title"), None);
    }

    #[test]
    fn test_version() {
        let s = "Package: foo\nVersion: 2.5-1\n";
        let mut desc: RDescription = s.parse().unwrap();
        let version = desc.version().unwrap().unwrap();
        assert_eq!(version, "2.5.1".parse().unwrap());
        assert_eq!(version.as_str(), "2.5-1");
        assert_eq!(desc.to_string(), s);

        desc.set_version(&"2.6-0".parse().unwrap());
        assert_eq!(desc.to_string(), "Package: foo\nVersion: 2.6-0\n");

        let desc: RDescription = "Package: foo\n".parse().unwrap();
        assert_eq!(desc.version(), Ok(None));
    }

    #[test]
    fn test_version_invalid() {
        let desc: RDescription = "Package: foo\nVersion: 1.0-beta\n".parse().unwrap();
        let err = desc.version().unwrap_err();
        assert_eq!(err.field(), "Version");
        assert_eq!(
            err.kind(),
            &FieldErrorKind::Version(crate::VersionParseError::InvalidComponent(
                "beta".to_string()
            ))
        );
    }

    #[test]
    fn test_set_invalid_field_name() {
        let mut desc: RDescription = "Package: foo\n".parse().unwrap();
        for name in [
            "", "Bad:Name", "Bad Name", "-Foo", "_Foo", "Fo\u{e9}", "Foo\n",
        ] {
            assert_eq!(
                desc.set(name, "x"),
                Err(InvalidFieldName(name.to_string())),
                "{name:?}"
            );
        }
        assert_eq!(desc.to_string(), "Package: foo\n");

        for name in [
            "Authors@R",
            "Date/Publication",
            "Config/testthat/edition",
            "9x",
        ] {
            desc.set(name, "x").unwrap();
            assert_eq!(desc.get(name), Some("x".to_string()), "{name:?}");
        }
    }

    #[test]
    fn test_lazy_data() {
        for (value, expected) in [
            ("true", true),
            ("TRUE", true),
            ("yes", true),
            ("Yes", true),
            ("false", false),
            ("no", false),
            ("NO", false),
        ] {
            let desc: RDescription = format!("Package: foo\nLazyData: {value}\n")
                .parse()
                .unwrap();
            assert_eq!(desc.lazy_data(), Ok(Some(expected)), "{value}");
        }

        let desc: RDescription = "Package: foo\n".parse().unwrap();
        assert_eq!(desc.lazy_data(), Ok(None));
    }

    #[test]
    fn test_lazy_data_invalid() {
        let desc: RDescription = "Package: foo\nLazyData: maybe\n".parse().unwrap();
        let err = desc.lazy_data().unwrap_err();
        assert_eq!(err.field(), "LazyData");
        assert_eq!(
            err.to_string(),
            "invalid LazyData field: expected yes, no, true or false, got \"maybe\""
        );
    }

    #[test]
    fn test_bug_reports() {
        let desc: RDescription = "Package: foo\nBugReports: https://example.com/issues\n"
            .parse()
            .unwrap();
        assert_eq!(
            desc.bug_reports(),
            Ok(Some("https://example.com/issues".parse().unwrap()))
        );

        let desc: RDescription = "Package: foo\n".parse().unwrap();
        assert_eq!(desc.bug_reports(), Ok(None));
    }

    #[test]
    fn test_bug_reports_invalid() {
        let desc: RDescription = "Package: foo\nBugReports: not a url\n".parse().unwrap();
        let err = desc.bug_reports().unwrap_err();
        assert_eq!(err.field(), "BugReports");
        assert_eq!(
            err.kind(),
            &FieldErrorKind::Url {
                url: "not a url".to_string(),
                error: url::ParseError::RelativeUrlWithoutBase,
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid BugReports field: \"not a url\": relative URL without a base"
        );
    }

    #[test]
    fn test_relations_invalid() {
        let s = "Package: foo\nImports: foo (>= 1.0, bar\n";
        let desc: RDescription = s.parse().unwrap();
        let err = desc.imports().unwrap_err();
        assert_eq!(err.field(), "Imports");
        let FieldErrorKind::Relations(e) = err.kind() else {
            panic!("unexpected error kind: {:?}", err.kind());
        };
        assert_eq!(
            e.errors()
                .iter()
                .map(|e| e.error.clone())
                .collect::<Vec<_>>(),
            [RelationParseError::MissingClosingParenthesis]
        );
        assert_eq!(err.range(), TextRange::empty(33.into()));
        assert_eq!(
            &s[..usize::from(err.range().start())],
            "Package: foo\nImports: foo (>= 1.0"
        );
    }

    #[test]
    fn test_field_error_range_multiline() {
        let s = "Package: foo\nImports:\n    cli,\n    glue (>> 1.0),\n    rlang\n";
        let desc: RDescription = s.parse().unwrap();
        let err = desc.imports().unwrap_err();
        assert_eq!(&s[err.range()], ">>");
    }

    #[test]
    fn test_field_error_range_whole_value() {
        let s = "Package: foo\nLazyData: maybe\n";
        let desc: RDescription = s.parse().unwrap();
        assert_eq!(&s[desc.lazy_data().unwrap_err().range()], "maybe");

        let s = "Package: foo\nBugReports:\n    not a url\n";
        let desc: RDescription = s.parse().unwrap();
        assert_eq!(&s[desc.bug_reports().unwrap_err().range()], "not a url");
    }

    #[test]
    fn test_value_offset_to_source() {
        // Value lines "a," and "bc" at 11..13 and 18..20 give the value "a,\nbc".
        let lines = [
            TextRange::new(11.into(), 13.into()),
            TextRange::new(18.into(), 20.into()),
        ];
        assert_eq!(value_offset_to_source(&lines, 0.into()), 11.into());
        assert_eq!(value_offset_to_source(&lines, 2.into()), 13.into());
        assert_eq!(value_offset_to_source(&lines, 3.into()), 18.into());
        assert_eq!(value_offset_to_source(&lines, 5.into()), 20.into());
    }

    #[test]
    fn test_r_description_additional_repositories() {
        let desc: RDescription = r###"Package: mypackage
Title: What the Package Does
Version: 1.0.0
Description: What the package does.
License: MIT
Additional_repositories: https://example.com/src/contrib,
    https://example.org/src/contrib
"###
        .parse()
        .unwrap();

        assert_eq!(
            desc.additional_repositories(),
            Ok(Some(vec![
                "https://example.com/src/contrib".parse().unwrap(),
                "https://example.org/src/contrib".parse().unwrap(),
            ]))
        );
    }

    #[test]
    fn test_set_additional_repositories() {
        let mut desc: RDescription = r###"Package: mypackage
Title: What the Package Does
Version: 1.0.0
Description: What the package does.
License: MIT
"###
        .parse()
        .unwrap();

        desc.set_additional_repositories(&[
            "https://example.com/src/contrib".parse().unwrap(),
            "https://example.org/src/contrib".parse().unwrap(),
        ]);

        assert_eq!(
            desc.additional_repositories(),
            Ok(Some(vec![
                "https://example.com/src/contrib".parse().unwrap(),
                "https://example.org/src/contrib".parse().unwrap(),
            ]))
        );
        assert_eq!(
            desc.to_string(),
            "Package: mypackage\nTitle: What the Package Does\nVersion: 1.0.0\nDescription: What the package does.\nLicense: MIT\nAdditional_repositories: https://example.com/src/contrib, https://example.org/src/contrib\n"
        );
    }

    #[test]
    fn test_remotes() {
        let desc: RDescription = include_str!("../testdata/remotes.desc").parse().unwrap();
        let remotes = desc.remotes().unwrap().unwrap();
        assert_eq!(
            remotes.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            vec![
                "httr=r-lib/httr@v0.4",
                "klutometis/roxygen#142",
                "gitlab::jimhester/covr",
                "git::git@bitbucket.org:djnavarro/lsr.git",
                "git::https://github.com/igraph/rigraph.git@main",
                "bitbucket::sulab/mygene.r@default",
                "bioc::3.3/SummarizedExperiment#117513",
                "svn::https://svn.example.com/stringr/trunk",
                "url::https://github.com/tidyverse/stringi/archive/HEAD.zip",
                "local::/pkgs/testthat",
            ]
        );
    }

    #[test]
    fn test_set_additional_repositories_empty_removes() {
        let mut desc: RDescription = "Package: foo\nAdditional_repositories: https://example.com\n"
            .parse()
            .unwrap();
        desc.set_additional_repositories(&[]);
        assert_eq!(desc.additional_repositories(), Ok(None));
        assert_eq!(desc.to_string(), "Package: foo\n");
    }

    #[test]
    fn test_urls() {
        let desc: RDescription =
            "Package: foo\nURL: https://a.example.com,https://b.example.com\n    https://c.example.com,\n"
                .parse()
                .unwrap();
        assert_eq!(
            desc.urls(),
            Ok(Some(vec![
                "https://a.example.com".parse().unwrap(),
                "https://b.example.com".parse().unwrap(),
                "https://c.example.com".parse().unwrap(),
            ]))
        );
    }

    #[test]
    fn test_remotes_missing() {
        let desc: RDescription = "Package: mypackage\n".parse().unwrap();
        assert_eq!(desc.remotes(), Ok(None));
    }

    #[test]
    fn test_remotes_invalid() {
        let desc: RDescription = "Package: mypackage\nRemotes: r-lib/httr,,r-lib/covr\n"
            .parse()
            .unwrap();
        let err = desc.remotes().unwrap_err();
        assert_eq!(err.field(), "Remotes");
        assert_eq!(
            err.kind(),
            &FieldErrorKind::Remotes(crate::remotes::RemoteParseError::Empty)
        );
    }

    #[test]
    fn test_set_remotes() {
        let mut desc: RDescription = "Package: mypackage\nRemotes: old/repo\nLicense: MIT\n"
            .parse()
            .unwrap();
        let remotes: Vec<Remote> = vec![
            "r-lib/httr@v0.4".parse().unwrap(),
            "cran::dplyr".parse().unwrap(),
        ];
        desc.set_remotes(&remotes).unwrap();
        assert_eq!(
            desc.to_string(),
            "Package: mypackage\nRemotes: r-lib/httr@v0.4, cran::dplyr\nLicense: MIT\n"
        );
        assert_eq!(desc.remotes(), Ok(Some(remotes)));

        desc.set_remotes(&[]).unwrap();
        assert_eq!(desc.to_string(), "Package: mypackage\nLicense: MIT\n");
        assert_eq!(desc.remotes(), Ok(None));
    }

    #[test]
    fn test_set_remotes_unrepresentable() {
        let mut desc: RDescription = "Package: mypackage\nRemotes: old/repo\n".parse().unwrap();
        let remote = Remote {
            package: None,
            source: crate::remotes::RemoteSource::Local("a path".to_string()),
        };
        assert_eq!(
            desc.set_remotes(&[remote]),
            Err(crate::remotes::RemoteParseError::Whitespace)
        );
        assert_eq!(desc.to_string(), "Package: mypackage\nRemotes: old/repo\n");
    }

    #[test]
    fn test_urls_invalid() {
        let desc: RDescription = "Package: foo\nURL: https://example.com, not-a-url\n"
            .parse()
            .unwrap();
        let err = desc.urls().unwrap_err();
        assert_eq!(err.field(), "URL");
        assert_eq!(
            err.kind(),
            &FieldErrorKind::Url {
                url: "not-a-url".to_string(),
                error: url::ParseError::RelativeUrlWithoutBase,
            }
        );
    }

    #[test]
    fn test_set_urls() {
        let mut desc: RDescription = "Package: foo\n".parse().unwrap();
        desc.set_urls(&[
            "https://a.example.com/".parse().unwrap(),
            "https://b.example.com/".parse().unwrap(),
        ]);
        assert_eq!(
            desc.to_string(),
            "Package: foo\nURL: https://a.example.com/, https://b.example.com/\n"
        );
        desc.set_urls(&[]);
        assert_eq!(desc.to_string(), "Package: foo\n");
    }

    #[test]
    fn test_named_fields() {
        let s = r###"Package: foo
Author: A. Person
Copyright: The authors
Priority: recommended
OS_type: unix
Type: Package
Contact: help@example.com
MailingList: list@example.com
Note: A note
Language: nl
LazyLoad: yes
KeepSource: no
ByteCompile: true
UseLTO: false
StagedInstall: TRUE
ZipData: no
Biarch: yes
BuildVignettes: false
NeedsCompilation: yes
License_is_FOSS: yes
License_restricts_use: no
"###;
        let desc: RDescription = s.parse().unwrap();
        assert_eq!(desc.author(), Some("A. Person".to_string()));
        assert_eq!(desc.copyright(), Some("The authors".to_string()));
        assert_eq!(desc.priority(), Some("recommended".to_string()));
        assert_eq!(desc.os_type(), Some("unix".to_string()));
        assert_eq!(desc.package_type(), Some("Package".to_string()));
        assert_eq!(desc.contact(), Some("help@example.com".to_string()));
        assert_eq!(desc.mailing_list(), Some("list@example.com".to_string()));
        assert_eq!(desc.note(), Some("A note".to_string()));
        assert_eq!(desc.language(), Some("nl".to_string()));
        assert_eq!(desc.lazy_load(), Ok(Some(true)));
        assert_eq!(desc.keep_source(), Ok(Some(false)));
        assert_eq!(desc.byte_compile(), Ok(Some(true)));
        assert_eq!(desc.use_lto(), Ok(Some(false)));
        assert_eq!(desc.staged_install(), Ok(Some(true)));
        assert_eq!(desc.zip_data(), Ok(Some(false)));
        assert_eq!(desc.biarch(), Ok(Some(true)));
        assert_eq!(desc.build_vignettes(), Ok(Some(false)));
        assert_eq!(desc.needs_compilation(), Ok(Some(true)));
        assert_eq!(desc.license_is_foss(), Ok(Some(true)));
        assert_eq!(desc.license_restricts_use(), Ok(Some(false)));

        let desc: RDescription = "Package: foo\n".parse().unwrap();
        assert_eq!(desc.author(), None);
        assert_eq!(desc.needs_compilation(), Ok(None));
    }

    #[test]
    fn test_set_named_fields() {
        let mut desc: RDescription = "Package: foo\n".parse().unwrap();
        desc.set_package_type("Package");
        desc.set_byte_compile(true);
        desc.set_needs_compilation(false);
        assert_eq!(
            desc.to_string(),
            "Package: foo\nType: Package\nByteCompile: true\nNeedsCompilation: no\n"
        );
    }

    #[test]
    fn test_generic_field_access() {
        let mut desc: RDescription = "Package: foo\nX-Custom: bar\n".parse().unwrap();
        assert_eq!(desc.get("x-custom"), Some("bar".to_string()));
        desc.set("X-Custom", "baz").unwrap();
        assert_eq!(desc.get("X-Custom"), Some("baz".to_string()));
        desc.remove("X-Custom");
        assert_eq!(desc.get("X-Custom"), None);
        assert_eq!(desc.to_string(), "Package: foo\n");
    }
}
