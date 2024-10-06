# R DESCRIPTION parser

This crate provides a parser and editor for the `DESCRIPTION` files used in R
packages.

See <https://r-pkgs.org/description.html> and
<https://cran.r-project.org/doc/manuals/R-exts.html> for more information on
the format.

Besides parsing the control files it also supports parsing and comparison
of version strings according to the R package versioning scheme as well
as relations between versions.

## Example

```rust

use r_description::RDescription;

let desc = r_description::parse(r###"Package: foo
Version: 1.0
# Inline comment that will be preserved.
Depends: R (>= 3.0.0)
"###).unwrap();

assert_eq!(desc.get("Package"), Some("foo"));
assert_eq!(desc.get("Version"), Some("1.0"));
assert_eq!(desc.get("Depends"), Some("R (>= 3.0.0"));

desc.insert("License", "MIT");
```

```rust
use r_description::Version;

let v1 = Version::parse("1.2.3-alpha").unwrap();
let v2 = Version::parse("1.2.3").unwrap();
assert!(v1 < v2);

```

```rust
use r_description::Relations;

let v1 = r_description::Version::parse("1.2.3").unwrap();
let rels: Relations = "cli (>= 2.0), crayon (= 1.3.4), testthat".parse().unwrap();
assert_eq!(3, rels.len());
assert_eq!(rels[0].name, "cli");
```
