//! A library for parsing and manipulating debian/copyright files that
//! use the DEP-5 format.
//!
//! # Examples
//!
//! ```rust
//!
//! use debian_copyright::Copyright;
//! use std::path::Path;
//!
//! let text = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
//! Upstream-Author: John Doe <john@example>
//! Upstream-Name: example
//! Source: https://example.com/example
//!
//! Files: *
//! License: GPL-3+
//! Copyright: 2019 John Doe
//!
//! Files: debian/*
//! License: GPL-3+
//! Copyright: 2019 Jane Packager
//!
//! License: GPL-3+
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//! "#;
//!
//! let c = text.parse::<Copyright>().unwrap();
//! let license = c.find_license_for_file(Path::new("debian/foo")).unwrap();
//! assert_eq!(license.name(), Some("GPL-3+"));
//! ```

use deb822_lossless::{FromDeb822, ToDeb822, FromDeb822Paragraph, ToDeb822Paragraph};
use crate::CURRENT_FORMAT;
use std::path::Path;
use crate::License;

fn deserialize_file_list(text: &str) -> Result<Vec<String>, String> {
    Ok(text.split('\n').map(|x| x.to_string()).collect())
}

fn serialize_file_list(files: &Vec<String>) -> String {
    files.join("\n")
}

#[derive(FromDeb822, ToDeb822, Clone, PartialEq, Eq, Debug)]
pub struct Header {
    #[deb822(field = "Format")]
    format: String,

    #[deb822(field = "Files-Excluded", deserialize_with = deserialize_file_list, serialize_with = serialize_file_list)]
    files_excluded: Option<Vec<String>>,

    #[deb822(field = "Source")]
    source: Option<String>,

    #[deb822(field = "Upstream-Contact")]
    upstream_contact: Option<String>,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            format: CURRENT_FORMAT.to_string(),
            files_excluded: None,
            source: None,
            upstream_contact: None
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Copyright {
    pub header: Header,
    pub files: Vec<FilesParagraph>,
    pub licenses: Vec<LicenseParagraph>,
}

impl std::str::FromStr for Copyright {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with("Format:") {
            return Err("Not machine readable".to_string());
        }

        let deb822: deb822_lossless::Deb822 = s.parse()
            .map_err(|e: deb822_lossless::ParseError| e.to_string())?;

        let mut paragraphs = deb822.paragraphs();

        let first_para = if let Some(para) = paragraphs.next() {
            para
        } else {
            return Err("No paragraphs".to_string());
        };

        let header: Header = Header::from_paragraph(&first_para)?;

        let mut files_paras = vec![];
        let mut license_paras = vec![];

        while let Some(para) = paragraphs.next() {
            if para.get("Files").is_some() {
                files_paras.push(FilesParagraph::from_paragraph(&para)?);
            } else if para.get("License").is_some() {
                license_paras.push(LicenseParagraph::from_paragraph(&para)?);
            } else {
                return Err("Paragraph is neither License nor Files".to_string());
            }
        }

        Ok(Copyright {
            header,
            files: files_paras,
            licenses: license_paras,
        })
    }
}

#[derive(FromDeb822, ToDeb822, Clone, PartialEq, Eq, Debug)]
pub struct LicenseParagraph {
    #[deb822(field="License")]
    license: License,
    #[deb822(field="Comment")]
    comment: Option<String>
}

fn deserialize_copyrights(text: &str) -> Result<Vec<String>, String> {
    Ok(text.split('\n').map(ToString::to_string).collect())
}

fn serialize_copyrights(copyrights: &Vec<String>) -> String {
    copyrights.join("\n")
}

impl std::fmt::Display for LicenseParagraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_paragraph().to_string())
    }
}

#[derive(FromDeb822, ToDeb822, Clone, PartialEq, Eq, Debug)]
pub struct FilesParagraph {
    #[deb822(field="Files", deserialize_with = deserialize_file_list, serialize_with = serialize_file_list)]
    files: Vec<String>,
    #[deb822(field="License")]
    license: License,
    #[deb822(field="Copyright", deserialize_with = deserialize_copyrights, serialize_with = serialize_copyrights)]
    copyright: Vec<String>,
    #[deb822(field="Comment")]
    comment: Option<String>
}

impl FilesParagraph {
    pub fn matches(&self, filename: &std::path::Path) -> bool {
        self.files
            .iter()
            .any(|f| crate::glob::glob_to_regex(f).is_match(filename.to_str().unwrap()))
    }
}

impl std::fmt::Display for FilesParagraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_paragraph().to_string())
    }
}

impl Copyright {
    pub fn new() -> Self {
        Self {
            header: Header::default(),
            licenses: Vec::new(),
            files: Vec::new()
        }
    }

    pub fn find_files(&self, path: &std::path::Path) -> Option<&FilesParagraph> {
        self.files.iter().filter(|f| f.matches(path)).last()
    }

    /// Returns the license for the given file.
    pub fn find_license_for_file(&self, filename: &Path) -> Option<&License> {
        let files = self.find_files(filename)?;
        if files.license.text().is_some() {
            return Some(&files.license);
        }
        self.find_license_by_name(files.license.name().unwrap())
    }

    pub fn find_license_by_name(&self, name: &str) -> Option<&License> {
        self.licenses
            .iter()
            .find(|p| p.license.name() == Some(name))
            .map(|p| &p.license)
    }
}

impl std::fmt::Display for Copyright {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.header.to_paragraph())?;
        for files in &self.files {
            write!(f, "\n")?;
            write!(f, "{}", files.to_paragraph())?;
        }
        for license in &self.licenses {
            write!(f, "\n")?;
            write!(f, "{}", license.to_paragraph())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_not_machine_readable() {
        let s = r#"
This copyright file is not machine readable.
"#;
        let ret = s.parse::<super::Copyright>();
        assert!(ret.is_err());
        assert_eq!(ret.unwrap_err(), "Not machine readable".to_string());
    }

    #[test]
    fn test_new() {
        let n = super::Copyright::new();
        assert_eq!(
            n.to_string().as_str(),
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        );
    }

    #[test]
    fn test_parse() {
        let s = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: foo
Upstream-Contact: Joe Bloggs <joe@example.com>
Source: https://example.com/foo

Files: *
Copyright:
  2020 Joe Bloggs <joe@example.com>
License: GPL-3+

Files: debian/*
Comment: Debian packaging is licensed under the GPL-3+.
Copyright: 2023 Jelmer Vernooij
License: GPL-3+

License: GPL-3+
 This program is free software: you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation, either version 3 of the License, or
 (at your option) any later version.
"#;
        let copyright = s.parse::<super::Copyright>().expect("failed to parse");

        assert_eq!(
            "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/",
            copyright.header.format
        );
        assert_eq!(
            "Joe Bloggs <joe@example.com>",
            copyright.header.upstream_contact.as_ref().unwrap()
        );
        assert_eq!(
            "https://example.com/foo",
            copyright.header.source.as_ref().unwrap()
        );

        let files = &copyright.files;
        assert_eq!(2, files.len());
        assert_eq!("*", files[0].files.join(" "));
        assert_eq!("debian/*", files[1].files.join(" "));
        assert_eq!(
            "Debian packaging is licensed under the GPL-3+.",
            files[1].comment.as_ref().unwrap()
        );
        assert_eq!(
            vec!["2023 Jelmer Vernooij".to_string()],
            files[1].copyright
        );
        assert_eq!("GPL-3+", files[1].license.name().unwrap());
        assert_eq!(files[1].license.text(), None);

        let licenses = &copyright.licenses;
        assert_eq!(1, licenses.len());
        assert_eq!("GPL-3+", licenses[0].license.name().unwrap());
        assert_eq!(
            "This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.",
            licenses[0].license.text().unwrap()
        );

        let upstream_files = copyright.find_files(std::path::Path::new("foo.c")).unwrap();
        assert_eq!(vec!["*"], upstream_files.files);

        let debian_files = copyright
            .find_files(std::path::Path::new("debian/foo.c"))
            .unwrap();
        assert_eq!(vec!["debian/*"], debian_files.files);

        let gpl = copyright.find_license_by_name("GPL-3+");
        assert!(gpl.is_some());

        let gpl = copyright.find_license_for_file(std::path::Path::new("debian/foo.c"));
        assert_eq!(gpl.unwrap().name().unwrap(), "GPL-3+");
    }
}