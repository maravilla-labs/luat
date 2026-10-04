// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `luat.lock`: every package of the resolved graph, pinned.

use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

use super::error::{IoContext, PackageError, Result};
use super::name::PackageName;

/// File name of the lockfile.
pub const LOCKFILE_NAME: &str = "luat.lock";

/// The lockfile format version this crate reads and writes.
pub const LOCKFILE_VERSION: u32 = 1;

/// Prefix of a path dependency's `source`.
pub(crate) const PATH_SOURCE_PREFIX: &str = "path+";

/// A parsed `luat.lock`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    /// Format version, [`LOCKFILE_VERSION`].
    pub version: u32,
    /// Pinned packages, sorted by name.
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

/// One pinned package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedPackage {
    /// Package name.
    pub name: PackageName,
    /// Pinned version.
    pub version: Version,
    /// Registry URL (registry packages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// `sha256:<hex>` of the tarball (registry packages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// `path+<dir relative to the project>` (path dependencies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Names of the packages this one depends on.
    #[serde(default)]
    pub dependencies: Vec<PackageName>,
}

impl LockedPackage {
    /// The directory of a path dependency, relative to the project.
    pub fn path_source(&self) -> Option<&str> {
        self.source.as_deref()?.strip_prefix(PATH_SOURCE_PREFIX)
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self {
            version: LOCKFILE_VERSION,
            packages: Vec::new(),
        }
    }
}

impl Lockfile {
    /// Parses lockfile text.
    pub fn parse(text: &str) -> Result<Self> {
        let lock: Self = toml::from_str(text).map_err(|e| PackageError::Manifest(format!("{LOCKFILE_NAME}: {e}")))?;
        if lock.version != LOCKFILE_VERSION {
            return Err(PackageError::Manifest(format!(
                "{LOCKFILE_NAME} has version {}, this luat reads version {LOCKFILE_VERSION}",
                lock.version
            )));
        }
        for p in &lock.packages {
            let registry = p.registry.is_some() && p.checksum.is_some();
            if registry == p.path_source().is_some() {
                return Err(PackageError::Manifest(format!(
                    "{LOCKFILE_NAME}: {} needs either registry and checksum, or a path+ source",
                    p.name
                )));
            }
        }
        Ok(lock)
    }

    /// Reads `<project>/luat.lock`, `None` when it does not exist.
    pub fn load(project: &Path) -> Result<Option<Self>> {
        let path = project.join(LOCKFILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).ctx(|| format!("reading {}", path.display())),
        }
    }

    /// The lockfile as TOML text.
    pub fn to_toml(&self) -> String {
        let mut sorted = self.clone();
        sorted.packages.sort_by(|a, b| a.name.cmp(&b.name));
        toml::to_string(&sorted).expect("lockfiles serialize")
    }

    /// Writes `<project>/luat.lock` (only when the content changed).
    pub fn save(&self, project: &Path) -> Result<()> {
        let path = project.join(LOCKFILE_NAME);
        let text = self.to_toml();
        if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        std::fs::write(&path, text).ctx(|| format!("writing {}", path.display()))
    }

    /// The entry for `name`.
    pub fn get(&self, name: &PackageName) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| &p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_documented_shape() {
        let text = r#"version = 1

[[package]]
name = "@acme/ui"
version = "1.2.0"
registry = "https://example.com"
checksum = "sha256:00"
dependencies = ["@acme/icons"]

[[package]]
name = "@acme/local"
version = "0.1.0"
source = "path+../local"
dependencies = []
"#;
        let lock = Lockfile::parse(text).unwrap();
        assert_eq!(lock.packages.len(), 2);
        assert_eq!(lock.packages[1].path_source(), Some("../local"));
        let again = Lockfile::parse(&lock.to_toml()).unwrap();
        assert_eq!(again.packages.len(), 2);
        assert!(lock.to_toml().starts_with("version = 1\n\n[[package]]\nname = \"@acme/local\""));
    }

    #[test]
    fn rejects_incomplete_entries() {
        assert!(Lockfile::parse("version = 1\n[[package]]\nname = \"@a/b\"\nversion = \"1.0.0\"\n").is_err());
        assert!(Lockfile::parse("version = 2\n").is_err());
    }
}
