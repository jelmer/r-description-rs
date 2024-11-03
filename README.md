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

use std::str::FromStr;
use r_description::lossy::RDescription;

let mut desc = RDescription::from_str(r###"Package: foo
Version: 1.0
Depends: R (>= 3.0.0)
Description: A foo package
Title: A foo package
License: GPL-3
"###).unwrap();

assert_eq!(desc.name, "foo");
assert_eq!(desc.version, "1.0".parse().unwrap());
assert_eq!(desc.depends, Some("R (>= 3.0.0)".parse().unwrap()));

desc.license = "MIT".to_string();
```

```rust
use r_description::Version;

let v1: Version = "1.2.3-alpha".parse().unwrap();
let v2: Version = "1.2.3".parse().unwrap();
assert!(v1 < v2);

```

```rust
use std::str::FromStr;
use r_description::lossy::Relations;

let v1 = r_description::Version::from_str("1.2.3").unwrap();
let rels: Relations = "cli (>= 2.0), crayon (= 1.3.4), testthat".parse().unwrap();
assert_eq!(3, rels.len());
assert_eq!(rels[0].name, "cli");
```
