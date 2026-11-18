//! Parsing of the `Remotes` field.
//!
//! The syntax is defined by the remotes R package; see
//! <https://remotes.r-lib.org/articles/dependencies.html>.
//!
//! The `Debug` implementations redact credentials embedded in URLs and Bioconductor
//! specifications, but `Display` writes them out, since they are part of the field value.

use std::fmt;
use std::str::FromStr;

/// A single entry in the `Remotes` field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Remote {
    /// Package name, if given with a `pkgname=` prefix.
    pub package: Option<String>,

    /// Where to obtain the package from.
    pub source: RemoteSource,
}

/// The source of a remote package.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum RemoteSource {
    /// A CRAN package, e.g. `cran::dplyr` or `dplyr`.
    Cran(String),

    /// A GitHub repository, e.g. `github::r-lib/remotes` or `r-lib/remotes`.
    GitHub(HostedRepository),

    /// A GitLab repository, e.g. `gitlab::group/subgroup/repo`.
    GitLab(HostedRepository),

    /// A Bitbucket repository, e.g. `bitbucket::owner/repo`.
    Bitbucket(HostedRepository),

    /// A generic git repository, e.g. `git::https://example.com/repo.git@main`.
    Git(GitRepository),

    /// A package archive at a URL, e.g. `url::https://example.com/pkg.tar.gz`.
    Url(url::Url),

    /// A package on the local filesystem, e.g. `local::/pkgs/testthat`.
    Local(String),

    /// A Subversion repository, e.g. `svn::https://svn.example.com/pkg/trunk`.
    Svn(url::Url),

    /// A Bioconductor package, e.g. `bioc::release/Biobase`.
    Bioc(BiocRepository),

    /// An explicitly typed source that this crate does not know about.
    Unknown {
        /// The source type, as written.
        kind: String,
        /// Custom host, given as `kind@host::`.
        host: Option<String>,
        /// Everything after the `::`.
        payload: String,
    },
}

/// A repository on GitHub, GitLab or Bitbucket.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostedRepository {
    /// Custom host, given as `github@host::`.
    pub host: Option<String>,

    /// User or organization owning the repository.
    pub owner: String,

    /// Repository name. For GitLab this includes any nested groups.
    pub repo: String,

    /// Subdirectory containing the package. Always `None` for GitLab, where
    /// additional path components are part of the repository name.
    pub subdir: Option<String>,

    /// Reference to install.
    pub reference: Option<GitReference>,
}

/// A reference in a hosted repository specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GitReference {
    /// A branch, tag or commit, written as `@ref`.
    Ref(String),

    /// A pull request, written as `#123`.
    PullRequest(u64),

    /// The latest release, written as `@*release`.
    LatestRelease,
}

/// A generic git repository.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GitRepository {
    /// URL to clone from. This may be an scp-style address such as
    /// `git@example.com:repo.git`.
    pub url: String,

    /// Branch, tag or commit to install.
    pub reference: Option<String>,
}

/// A package in the Bioconductor git repository.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BiocRepository {
    /// Credentials for the Bioconductor git server.
    pub credentials: Option<BiocCredentials>,

    /// Bioconductor release: `release`, `devel` or a version number.
    pub release: Option<String>,

    /// Package name.
    pub package: String,

    /// Commit to install.
    pub commit: Option<String>,
}

/// Username and password for a Bioconductor git server.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BiocCredentials {
    /// Username.
    pub username: String,

    /// Password.
    pub password: String,
}

/// Error parsing a `Remotes` entry.
///
/// The variants deliberately do not include the input, since it may contain credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteParseError {
    /// An entry is empty, e.g. because of two consecutive commas.
    Empty,

    /// An entry contains whitespace, usually because of a missing comma.
    Whitespace,

    /// An entry contains a comma, which separates entries in the field.
    Comma,

    /// The `pkgname=` prefix is not a valid package name.
    InvalidPackageName,

    /// More than one `::` separator.
    MultipleSeparators,

    /// The source type before `::` is empty.
    MissingKind,

    /// The custom host in `kind@host::` is empty or contains another `@`.
    InvalidHost,

    /// A custom host was given for a source type that does not support one.
    HostNotSupported,

    /// Nothing follows the `::` separator.
    MissingPayload,

    /// A hosted repository specification is not of the form `owner/repo[/subdir]`.
    InvalidRepository,

    /// A reference (`@ref` or `#pr`) is empty or malformed.
    InvalidReference,

    /// The URL of a `git::` remote is empty.
    MissingGitUrl,

    /// A `url::` or `svn::` URL is invalid.
    InvalidUrl(url::ParseError),

    /// A `bioc::` specification is malformed.
    InvalidBioc,

    /// A remote can not be written in a form that parses back to the same value.
    Unrepresentable,
}

impl fmt::Display for RemoteParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty remote specification"),
            Self::Whitespace => f.write_str("whitespace in remote specification (missing comma?)"),
            Self::Comma => f.write_str("comma in remote specification"),
            Self::InvalidPackageName => f.write_str("invalid package name in remote"),
            Self::MultipleSeparators => f.write_str("multiple '::' separators in remote"),
            Self::MissingKind => f.write_str("missing remote type before '::'"),
            Self::InvalidHost => f.write_str("invalid custom host in remote type"),
            Self::HostNotSupported => f.write_str("remote type does not support a custom host"),
            Self::MissingPayload => f.write_str("missing remote after '::'"),
            Self::InvalidRepository => f.write_str("invalid repository in remote"),
            Self::InvalidReference => f.write_str("invalid reference in remote"),
            Self::MissingGitUrl => f.write_str("missing URL in git remote"),
            Self::InvalidUrl(e) => write!(f, "invalid URL in remote: {e}"),
            Self::InvalidBioc => f.write_str("invalid Bioconductor remote"),
            Self::Unrepresentable => f.write_str("remote can not be represented"),
        }
    }
}

impl std::error::Error for RemoteParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUrl(e) => Some(e),
            _ => None,
        }
    }
}

/// Check that `name` is a valid R package name, as accepted by remotes for the `pkgname=` prefix.
fn is_valid_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() >= 2
        && !name.ends_with('.')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '.')
}

impl FromStr for Remote {
    type Err = RemoteParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(RemoteParseError::Empty);
        }
        if s.chars().any(char::is_whitespace) {
            return Err(RemoteParseError::Whitespace);
        }
        if s.contains(',') {
            return Err(RemoteParseError::Comma);
        }

        // An '=' only introduces a package name if it comes before anything that is part of
        // the source itself, so that e.g. query strings in URLs are left alone.
        let source_start = s.find(['/', ':', '@', '#']).unwrap_or(s.len());
        let (package, rest) = match s[..source_start].split_once('=') {
            Some((name, _)) if !is_valid_package_name(name) => {
                return Err(RemoteParseError::InvalidPackageName)
            }
            Some((name, _)) => (Some(name.to_string()), &s[name.len() + 1..]),
            None => (None, s),
        };

        let source = match rest.split_once("::") {
            None if rest.is_empty() => return Err(RemoteParseError::Empty),
            // TODO: remotes also accepts full GitHub URLs such as https://github.com/owner/repo
            // here; those are currently rejected.
            None if rest.contains('/') => RemoteSource::GitHub(parse_hosted(rest, None, false)?),
            None => RemoteSource::Cran(rest.to_string()),
            Some((prefix, payload)) => parse_typed(prefix, payload)?,
        };

        Ok(Remote { package, source })
    }
}

fn parse_typed(prefix: &str, payload: &str) -> Result<RemoteSource, RemoteParseError> {
    if payload.contains("::") {
        return Err(RemoteParseError::MultipleSeparators);
    }
    let (kind, host) = match prefix.split_once('@') {
        Some((_, host)) if host.is_empty() || host.contains('@') => {
            return Err(RemoteParseError::InvalidHost)
        }
        Some((kind, host)) => (kind, Some(host.to_string())),
        None => (prefix, None),
    };
    if kind.is_empty() {
        return Err(RemoteParseError::MissingKind);
    }
    if payload.is_empty() {
        return Err(RemoteParseError::MissingPayload);
    }

    let lower = kind.to_ascii_lowercase();
    let source = match lower.as_str() {
        "github" => return Ok(RemoteSource::GitHub(parse_hosted(payload, host, false)?)),
        "gitlab" => return Ok(RemoteSource::GitLab(parse_hosted(payload, host, true)?)),
        "bitbucket" => return Ok(RemoteSource::Bitbucket(parse_hosted(payload, host, false)?)),
        "cran" => RemoteSource::Cran(payload.to_string()),
        "git" => RemoteSource::Git(parse_git(payload)?),
        "url" => RemoteSource::Url(url::Url::parse(payload).map_err(RemoteParseError::InvalidUrl)?),
        "local" => RemoteSource::Local(payload.to_string()),
        "svn" => RemoteSource::Svn(url::Url::parse(payload).map_err(RemoteParseError::InvalidUrl)?),
        "bioc" => RemoteSource::Bioc(parse_bioc(payload)?),
        _ => {
            return Ok(RemoteSource::Unknown {
                kind: kind.to_string(),
                host,
                payload: payload.to_string(),
            })
        }
    };
    // remotes passes the host on to the source, but only hosted sources make use of it.
    if host.is_some() {
        return Err(RemoteParseError::HostNotSupported);
    }
    Ok(source)
}

/// Parse `owner/repo[/subdir][@ref|#pull|@*release]`.
fn parse_hosted(
    payload: &str,
    host: Option<String>,
    nested_groups: bool,
) -> Result<HostedRepository, RemoteParseError> {
    let (path, reference) = match payload.find(['@', '#']) {
        Some(i) => (&payload[..i], Some(parse_reference(&payload[i..])?)),
        None => (payload, None),
    };

    // remotes allows a trailing slash after a subdirectory, but not directly after the
    // repository name.
    let path = match path.strip_suffix('/') {
        Some(stripped) if stripped.matches('/').count() >= 2 => stripped,
        _ => path,
    };
    let mut parts = path.split('/');
    let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else {
        return Err(RemoteParseError::InvalidRepository);
    };
    let rest = parts.collect::<Vec<_>>();
    if owner.is_empty() || repo.is_empty() || rest.iter().any(|p| p.is_empty()) {
        return Err(RemoteParseError::InvalidRepository);
    }

    let (repo, subdir) = if rest.is_empty() {
        (repo.to_string(), None)
    } else if nested_groups {
        (format!("{repo}/{}", rest.join("/")), None)
    } else {
        (repo.to_string(), Some(rest.join("/")))
    };

    Ok(HostedRepository {
        host,
        owner: owner.to_string(),
        repo,
        subdir,
        reference,
    })
}

fn parse_reference(s: &str) -> Result<GitReference, RemoteParseError> {
    if s == "@*release" {
        return Ok(GitReference::LatestRelease);
    }
    if let Some(pull) = s.strip_prefix('#') {
        if !pull.bytes().all(|b| b.is_ascii_digit()) {
            return Err(RemoteParseError::InvalidReference);
        }
        return pull
            .parse()
            .map(GitReference::PullRequest)
            .map_err(|_| RemoteParseError::InvalidReference);
    }
    match s.strip_prefix('@') {
        Some(r) if !r.is_empty() && !r.starts_with('*') => Ok(GitReference::Ref(r.to_string())),
        _ => Err(RemoteParseError::InvalidReference),
    }
}

/// Parse a git URL with an optional `@ref` suffix.
///
/// Like remotes, the reference is only looked for after the first slash following the
/// scheme, so that user names in URLs are not mistaken for references.
fn parse_git(payload: &str) -> Result<GitRepository, RemoteParseError> {
    let path_start = match payload.find("://") {
        Some(i) if !payload[..i].contains('/') => i + 3,
        _ => 0,
    };
    let ref_start = payload[path_start..].find('/').and_then(|slash| {
        let path_start = path_start + slash;
        payload[path_start..].find('@').map(|at| path_start + at)
    });
    let (url, reference) = match ref_start {
        Some(i) if i + 1 == payload.len() => return Err(RemoteParseError::InvalidReference),
        Some(i) => (&payload[..i], Some(payload[i + 1..].to_string())),
        None => (payload, None),
    };
    if url.is_empty() {
        return Err(RemoteParseError::MissingGitUrl);
    }
    Ok(GitRepository {
        url: url.to_string(),
        reference,
    })
}

/// Parse `[username:password@][release/]package[#commit]`.
fn parse_bioc(payload: &str) -> Result<BiocRepository, RemoteParseError> {
    let (rest, commit) = match payload.split_once('#') {
        Some((_, commit))
            if commit.is_empty() || !commit.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            return Err(RemoteParseError::InvalidBioc)
        }
        Some((rest, commit)) => (rest, Some(commit.to_string())),
        None => (payload, None),
    };

    let (credentials, rest) = match rest.rsplit_once('@') {
        Some((credentials, rest)) => {
            let (username, password) = credentials
                .split_once(':')
                .filter(|(u, p)| !u.is_empty() && !p.is_empty() && !p.contains(':'))
                .ok_or(RemoteParseError::InvalidBioc)?;
            let credentials = BiocCredentials {
                username: username.to_string(),
                password: password.to_string(),
            };
            (Some(credentials), rest)
        }
        None => (None, rest),
    };

    let (release, package) = match rest.split_once('/') {
        Some((release, _))
            if !(release == "release"
                || release == "devel"
                || (!release.is_empty()
                    && release.bytes().all(|b| b.is_ascii_digit() || b == b'.'))) =>
        {
            return Err(RemoteParseError::InvalidBioc)
        }
        Some((release, package)) => (Some(release.to_string()), package),
        None => (None, rest),
    };
    if package.is_empty() || package.contains('/') {
        return Err(RemoteParseError::InvalidBioc);
    }

    Ok(BiocRepository {
        credentials,
        release,
        package: package.to_string(),
        commit,
    })
}

impl fmt::Display for GitReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Ref(r) => write!(f, "@{r}"),
            Self::PullRequest(n) => write!(f, "#{n}"),
            Self::LatestRelease => f.write_str("@*release"),
        }
    }
}

fn write_kind(f: &mut fmt::Formatter, kind: &str, host: Option<&str>) -> fmt::Result {
    f.write_str(kind)?;
    if let Some(host) = host {
        write!(f, "@{host}")?;
    }
    f.write_str("::")
}

fn write_hosted(f: &mut fmt::Formatter, kind: &str, repo: &HostedRepository) -> fmt::Result {
    write_kind(f, kind, repo.host.as_deref())?;
    write!(f, "{}/{}", repo.owner, repo.repo)?;
    if let Some(subdir) = &repo.subdir {
        write!(f, "/{subdir}")?;
    }
    if let Some(reference) = &repo.reference {
        write!(f, "{reference}")?;
    }
    Ok(())
}

impl fmt::Display for Remote {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(package) = &self.package {
            write!(f, "{package}=")?;
        }
        match &self.source {
            RemoteSource::Cran(name) => write!(f, "cran::{name}"),
            RemoteSource::GitHub(repo) if repo.host.is_none() => {
                // Untyped owner/repo defaults to GitHub; that is the form used in the docs.
                write!(f, "{}/{}", repo.owner, repo.repo)?;
                if let Some(subdir) = &repo.subdir {
                    write!(f, "/{subdir}")?;
                }
                if let Some(reference) = &repo.reference {
                    write!(f, "{reference}")?;
                }
                Ok(())
            }
            RemoteSource::GitHub(repo) => write_hosted(f, "github", repo),
            RemoteSource::GitLab(repo) => write_hosted(f, "gitlab", repo),
            RemoteSource::Bitbucket(repo) => write_hosted(f, "bitbucket", repo),
            RemoteSource::Git(git) => {
                write!(f, "git::{}", git.url)?;
                if let Some(reference) = &git.reference {
                    write!(f, "@{reference}")?;
                }
                Ok(())
            }
            RemoteSource::Url(url) => write!(f, "url::{url}"),
            RemoteSource::Local(path) => write!(f, "local::{path}"),
            RemoteSource::Svn(url) => write!(f, "svn::{url}"),
            RemoteSource::Bioc(bioc) => {
                f.write_str("bioc::")?;
                if let Some(credentials) = &bioc.credentials {
                    write!(f, "{}:{}@", credentials.username, credentials.password)?;
                }
                if let Some(release) = &bioc.release {
                    write!(f, "{release}/")?;
                }
                f.write_str(&bioc.package)?;
                if let Some(commit) = &bioc.commit {
                    write!(f, "#{commit}")?;
                }
                Ok(())
            }
            RemoteSource::Unknown {
                kind,
                host,
                payload,
            } => {
                write_kind(f, kind, host.as_deref())?;
                f.write_str(payload)
            }
        }
    }
}

struct RedactedUrl<'a>(&'a url::Url);

impl fmt::Debug for RedactedUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut url = self.0.clone();
        if !url.username().is_empty() || url.password().is_some() {
            // These only fail for URLs that can not have credentials in the first place.
            let _ = url.set_username("REDACTED");
            let _ = url.set_password(None);
        }
        fmt::Debug::fmt(url.as_str(), f)
    }
}

impl fmt::Debug for RemoteSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Cran(name) => f.debug_tuple("Cran").field(name).finish(),
            Self::GitHub(repo) => f.debug_tuple("GitHub").field(repo).finish(),
            Self::GitLab(repo) => f.debug_tuple("GitLab").field(repo).finish(),
            Self::Bitbucket(repo) => f.debug_tuple("Bitbucket").field(repo).finish(),
            Self::Git(git) => f.debug_tuple("Git").field(git).finish(),
            Self::Url(url) => f.debug_tuple("Url").field(&RedactedUrl(url)).finish(),
            Self::Local(path) => f.debug_tuple("Local").field(path).finish(),
            Self::Svn(url) => f.debug_tuple("Svn").field(&RedactedUrl(url)).finish(),
            Self::Bioc(bioc) => f.debug_tuple("Bioc").field(bioc).finish(),
            Self::Unknown {
                kind,
                host,
                payload,
            } => f
                .debug_struct("Unknown")
                .field("kind", kind)
                .field("host", host)
                .field("payload", payload)
                .finish(),
        }
    }
}

impl fmt::Debug for GitRepository {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut debug = f.debug_struct("GitRepository");
        match url::Url::parse(&self.url) {
            Ok(url) => debug.field("url", &RedactedUrl(&url)),
            // scp-style addresses only carry a user name, which is not secret.
            Err(_) => debug.field("url", &self.url),
        };
        debug.field("reference", &self.reference).finish()
    }
}

impl fmt::Debug for BiocCredentials {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BiocCredentials")
            .field("username", &self.username)
            .field("password", &"REDACTED")
            .finish()
    }
}

/// Parse the value of a `Remotes` field.
///
/// Entries are separated by commas and may be surrounded by whitespace, including newlines.
/// As with remotes, a single trailing comma is allowed but other empty entries are an error.
/// A value consisting only of whitespace contains no remotes.
pub fn parse_remotes(s: &str) -> Result<Vec<Remote>, RemoteParseError> {
    if s.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut entries = s.split(',').map(str::trim).collect::<Vec<_>>();
    if entries.len() > 1 && entries.last() == Some(&"") {
        entries.pop();
    }
    entries.into_iter().map(str::parse).collect()
}

/// Format a list of remotes as the value of a `Remotes` field.
///
/// Fails if a remote can not be written in a form that parses back to the same value, which
/// can happen for values that were constructed rather than parsed.
pub fn format_remotes(remotes: &[Remote]) -> Result<String, RemoteParseError> {
    let entries = remotes
        .iter()
        .map(|remote| {
            let s = remote.to_string();
            if s.parse::<Remote>()? != *remote {
                return Err(RemoteParseError::Unrepresentable);
            }
            Ok(s)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Remote {
        s.parse().unwrap()
    }

    fn hosted(owner: &str, repo: &str) -> HostedRepository {
        HostedRepository {
            host: None,
            owner: owner.to_string(),
            repo: repo.to_string(),
            subdir: None,
            reference: None,
        }
    }

    #[test]
    fn test_github_shorthand() {
        assert_eq!(
            parse("r-lib/testthat"),
            Remote {
                package: None,
                source: RemoteSource::GitHub(hosted("r-lib", "testthat")),
            }
        );
    }

    #[test]
    fn test_github_references() {
        let cases = [
            ("r-lib/httr@v0.4", GitReference::Ref("v0.4".to_string())),
            ("klutometis/roxygen#142", GitReference::PullRequest(142)),
            ("r-lib/testthat@*release", GitReference::LatestRelease),
            (
                "r-lib/remotes@feature/a@b",
                GitReference::Ref("feature/a@b".to_string()),
            ),
        ];
        for (input, reference) in cases {
            let RemoteSource::GitHub(repo) = parse(input).source else {
                panic!("expected GitHub remote for {input}");
            };
            assert_eq!(repo.reference, Some(reference), "{input}");
        }
    }

    #[test]
    fn test_github_explicit_with_host_and_subdir() {
        let remote = parse("pkg=github@ghe.example.com::owner/repo/sub/dir/@main");
        assert_eq!(
            remote,
            Remote {
                package: Some("pkg".to_string()),
                source: RemoteSource::GitHub(HostedRepository {
                    host: Some("ghe.example.com".to_string()),
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                    subdir: Some("sub/dir".to_string()),
                    reference: Some(GitReference::Ref("main".to_string())),
                }),
            }
        );
        assert_eq!(
            remote.to_string(),
            "pkg=github@ghe.example.com::owner/repo/sub/dir@main"
        );
    }

    #[test]
    fn test_github_explicit_without_host_is_written_as_shorthand() {
        assert_eq!(
            parse("GitHub::tidyverse/ggplot2").to_string(),
            "tidyverse/ggplot2"
        );
    }

    #[test]
    fn test_gitlab_nested_groups() {
        assert_eq!(
            parse("gitlab::group/subgroup/repo@main").source,
            RemoteSource::GitLab(HostedRepository {
                reference: Some(GitReference::Ref("main".to_string())),
                ..hosted("group", "subgroup/repo")
            })
        );
    }

    #[test]
    fn test_bitbucket() {
        assert_eq!(
            parse("bitbucket::sulab/mygene.r@default").source,
            RemoteSource::Bitbucket(HostedRepository {
                reference: Some(GitReference::Ref("default".to_string())),
                ..hosted("sulab", "mygene.r")
            })
        );
    }

    #[test]
    fn test_git() {
        let cases = [
            (
                "git::git@bitbucket.org:djnavarro/lsr.git",
                "git@bitbucket.org:djnavarro/lsr.git",
                None,
            ),
            (
                "git::https://github.com/igraph/rigraph.git@main",
                "https://github.com/igraph/rigraph.git",
                Some("main"),
            ),
            (
                "git::ssh://git@git.example.com/team/pkg.git@feature@candidate",
                "ssh://git@git.example.com/team/pkg.git",
                Some("feature@candidate"),
            ),
            (
                "git::git@example.com:team/pkg.git@v1",
                "git@example.com:team/pkg.git",
                Some("v1"),
            ),
            // Without a slash, remotes does not look for a reference at all.
            (
                "git::git@example.com:pkg.git",
                "git@example.com:pkg.git",
                None,
            ),
        ];
        for (input, url, reference) in cases {
            assert_eq!(
                parse(input).source,
                RemoteSource::Git(GitRepository {
                    url: url.to_string(),
                    reference: reference.map(str::to_string),
                }),
                "{input}"
            );
        }
    }

    #[test]
    fn test_url_svn_local_cran() {
        assert_eq!(
            parse("url::https://github.com/tidyverse/stringr/archive/HEAD.zip").source,
            RemoteSource::Url(
                url::Url::parse("https://github.com/tidyverse/stringr/archive/HEAD.zip").unwrap()
            )
        );
        assert_eq!(
            parse("svn::https://svn.example.com/stringr/trunk").source,
            RemoteSource::Svn(url::Url::parse("https://svn.example.com/stringr/trunk").unwrap())
        );
        assert_eq!(
            parse("local::/pkgs/testthat").source,
            RemoteSource::Local("/pkgs/testthat".to_string())
        );
        assert_eq!(
            parse("cran::dplyr").source,
            RemoteSource::Cran("dplyr".to_string())
        );
        assert_eq!(
            parse("dplyr").source,
            RemoteSource::Cran("dplyr".to_string())
        );
    }

    #[test]
    fn test_equals_in_url_is_not_package_name() {
        let remote = parse("url::https://example.com/get?pkg=foo");
        assert_eq!(remote.package, None);
        assert_eq!(remote.to_string(), "url::https://example.com/get?pkg=foo");
    }

    #[test]
    fn test_bioc() {
        assert_eq!(
            parse("bioc::3.3/SummarizedExperiment#117513").source,
            RemoteSource::Bioc(BiocRepository {
                credentials: None,
                release: Some("3.3".to_string()),
                package: "SummarizedExperiment".to_string(),
                commit: Some("117513".to_string()),
            })
        );
        assert_eq!(
            parse("bioc::user:pass@devel/Biobase").source,
            RemoteSource::Bioc(BiocRepository {
                credentials: Some(BiocCredentials {
                    username: "user".to_string(),
                    password: "pass".to_string(),
                }),
                release: Some("devel".to_string()),
                package: "Biobase".to_string(),
                commit: None,
            })
        );
        assert_eq!(
            parse("bioc::Biobase").source,
            RemoteSource::Bioc(BiocRepository {
                credentials: None,
                release: None,
                package: "Biobase".to_string(),
                commit: None,
            })
        );
    }

    #[test]
    fn test_unknown() {
        let remote = parse("forge@forge.example.com::team/pkg@edge");
        assert_eq!(
            remote.source,
            RemoteSource::Unknown {
                kind: "forge".to_string(),
                host: Some("forge.example.com".to_string()),
                payload: "team/pkg@edge".to_string(),
            }
        );
        assert_eq!(remote.to_string(), "forge@forge.example.com::team/pkg@edge");
    }

    #[test]
    fn test_round_trip() {
        let cases = [
            "r-lib/testthat",
            "pkg=r-lib/testthat/sub@v1",
            "r-lib/testthat#12",
            "r-lib/testthat@*release",
            "github@ghe.example.com::owner/repo",
            "gitlab::group/subgroup/repo@main",
            "gitlab@gitlab.example.com::team/helper@stable",
            "bitbucket::owner/repo/pkg@stable",
            "git::https://example.com/repo.git@v1.0",
            "git::git@example.com:team/repo.git",
            "url::https://example.com/pkg.tar.gz",
            "local::../pkg",
            "svn::https://svn.example.com/pkg/trunk",
            "bioc::user:pass@3.19/SummarizedExperiment#abc123",
            "bioc::release/Biobase",
            "cran::dplyr",
            "forge::anything",
        ];
        for case in cases {
            assert_eq!(parse(case).to_string(), case);
        }
    }

    #[test]
    fn test_invalid() {
        let cases = [
            ("", RemoteParseError::Empty),
            ("a/b c/d", RemoteParseError::Whitespace),
            ("a/b,c/d", RemoteParseError::Comma),
            ("bad-name=owner/repo", RemoteParseError::InvalidPackageName),
            ("a.=owner/repo", RemoteParseError::InvalidPackageName),
            ("pkg=", RemoteParseError::Empty),
            ("git::a::b", RemoteParseError::MultipleSeparators),
            ("::owner/repo", RemoteParseError::MissingKind),
            ("@host::owner/repo", RemoteParseError::MissingKind),
            ("github@::owner/repo", RemoteParseError::InvalidHost),
            ("github@a@b::owner/repo", RemoteParseError::InvalidHost),
            (
                "url@host::https://example.com/",
                RemoteParseError::HostNotSupported,
            ),
            ("github::", RemoteParseError::MissingPayload),
            ("github::owner", RemoteParseError::InvalidRepository),
            ("owner//repo", RemoteParseError::InvalidRepository),
            ("owner/repo/", RemoteParseError::InvalidRepository),
            ("/repo", RemoteParseError::InvalidRepository),
            ("owner/repo@", RemoteParseError::InvalidReference),
            ("owner/repo#abc", RemoteParseError::InvalidReference),
            ("owner/repo@*latest", RemoteParseError::InvalidReference),
            (
                "git::https://example.com/repo.git@",
                RemoteParseError::InvalidReference,
            ),
            ("bioc::foo/Biobase", RemoteParseError::InvalidBioc),
            ("bioc::user@Biobase", RemoteParseError::InvalidBioc),
            ("bioc::Biobase#", RemoteParseError::InvalidBioc),
            ("bioc::release/", RemoteParseError::InvalidBioc),
        ];
        for (input, error) in cases {
            assert_eq!(input.parse::<Remote>(), Err(error), "{input}");
        }
        assert!(matches!(
            "url::relative/path".parse::<Remote>(),
            Err(RemoteParseError::InvalidUrl(_))
        ));
    }

    #[test]
    fn test_debug_redacts_credentials() {
        for input in [
            "bioc::someuser:secretpw@release/Biobase",
            "url::https://someuser:secretpw@example.com/pkg.tar.gz",
            "svn::https://someuser:secretpw@example.com/trunk",
            "git::https://someuser:secretpw@example.com/repo.git@main",
        ] {
            let debug = format!("{:?}", parse(input));
            assert!(!debug.contains("secretpw"), "{debug}");
        }
    }

    #[test]
    fn test_parse_remotes() {
        assert_eq!(
            parse_remotes("r-lib/httr@v0.4,\n  klutometis/roxygen#142 ,\n  cran::dplyr,\n"),
            Ok(vec![
                parse("r-lib/httr@v0.4"),
                parse("klutometis/roxygen#142"),
                parse("cran::dplyr"),
            ])
        );
        assert_eq!(parse_remotes(""), Ok(vec![]));
        assert_eq!(parse_remotes(" \n "), Ok(vec![]));
        assert_eq!(parse_remotes("a/b,,c/d"), Err(RemoteParseError::Empty));
        assert_eq!(parse_remotes(",a/b"), Err(RemoteParseError::Empty));
        assert_eq!(parse_remotes("a/b,,"), Err(RemoteParseError::Empty));
        assert_eq!(parse_remotes(","), Err(RemoteParseError::Empty));
        assert_eq!(parse_remotes("a/b\nc/d"), Err(RemoteParseError::Whitespace));
    }

    #[test]
    fn test_format_remotes() {
        assert_eq!(
            format_remotes(&[parse("r-lib/httr@v0.4"), parse("cran::dplyr")]),
            Ok("r-lib/httr@v0.4, cran::dplyr".to_string())
        );
        assert_eq!(format_remotes(&[]), Ok(String::new()));
        let unrepresentable = Remote {
            package: None,
            source: RemoteSource::Url(url::Url::parse("https://example.com/a,b").unwrap()),
        };
        assert_eq!(
            format_remotes(&[unrepresentable]),
            Err(RemoteParseError::Comma)
        );
        let unrepresentable = Remote {
            package: None,
            source: RemoteSource::GitHub(hosted("owner/with/slashes", "repo")),
        };
        assert_eq!(
            format_remotes(&[unrepresentable]),
            Err(RemoteParseError::Unrepresentable)
        );
    }
}
