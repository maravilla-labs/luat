// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Package names: `@scope/name`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::error::PackageError;
use crate::package_paths::is_valid_name_part;

/// A validated package name, `@scope/name`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageName {
    scope: String,
    name: String,
}

impl PackageName {
    /// Parses and validates `@scope/name`.
    pub fn parse(s: &str) -> Result<Self, PackageError> {
        let invalid = || PackageError::InvalidName(s.to_string());
        let (scope, name) = s.strip_prefix('@').and_then(|r| r.split_once('/')).ok_or_else(invalid)?;
        if !is_valid_name_part(scope) || !is_valid_name_part(name) {
            return Err(invalid());
        }
        Ok(Self {
            scope: scope.to_string(),
            name: name.to_string(),
        })
    }

    /// The scope without `@`, e.g. `acme`.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The name part, e.g. `ui`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// URL path of this package, `@scope/name` (both parts are URL-safe).
    pub(crate) fn url_path(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}/{}", self.scope, self.name)
    }
}

impl FromStr for PackageName {
    type Err = PackageError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for PackageName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackageName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Splits `luat add` input, `@scope/name[@<requirement>]`.
pub fn parse_spec(spec: &str) -> Result<(PackageName, Option<semver::VersionReq>), PackageError> {
    let (name, req) = match spec.get(1..).and_then(|rest| rest.find('@')) {
        Some(at) => (&spec[..at + 1], Some(&spec[at + 2..])),
        None => (spec, None),
    };
    let name = PackageName::parse(name)?;
    let req = req
        .map(|r| {
            semver::VersionReq::parse(r)
                .map_err(|e| PackageError::Manifest(format!("invalid version requirement '{r}' for {name}: {e}")))
        })
        .transpose()?;
    Ok((name, req))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        let n = PackageName::parse("@acme/ui").unwrap();
        assert_eq!((n.scope(), n.name()), ("acme", "ui"));
        assert_eq!(n.to_string(), "@acme/ui");
        for bad in ["acme/ui", "@acme", "@acme/", "@/ui", "@Acme/ui", "@acme/ui/x", "@acme/.ui"] {
            assert!(PackageName::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_specs() {
        let (n, r) = parse_spec("@acme/ui").unwrap();
        assert_eq!((n.to_string(), r), ("@acme/ui".to_string(), None));
        let (n, r) = parse_spec("@acme/ui@^1.2").unwrap();
        assert_eq!(n.to_string(), "@acme/ui");
        assert_eq!(r.unwrap().to_string(), "^1.2");
        assert!(parse_spec("@acme/ui@not a version").is_err());
    }
}
