// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! The package-related sections of `luat.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use serde::Deserialize;

use super::error::{IoContext, PackageError, Result};
use super::name::PackageName;
use super::{normalize_url, DEFAULT_REGISTRY};

/// File name of the manifest.
pub const MANIFEST_NAME: &str = "luat.toml";

/// `[package]`: what a published package is.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageMeta {
    /// `@scope/name`.
    pub name: PackageName,
    /// The package version.
    pub version: Version,
    /// One-line description.
    pub description: Option<String>,
    /// License expression.
    pub license: Option<String>,
    /// Source repository URL.
    pub repository: Option<String>,
    /// Search keywords.
    pub keywords: Vec<String>,
    /// Luat versions the package works with.
    pub luat: Option<VersionReq>,
    /// Files to ship (globs relative to the package root); `None` means
    /// [`DEFAULT_INCLUDE`](super::DEFAULT_INCLUDE).
    pub include: Option<Vec<String>>,
}

/// One `[dependencies]` entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Dependency {
    /// `"@acme/ui" = "^1.2"` (or `{ version = "^1.2" }`): from a registry.
    Registry(VersionReq),
    /// `"@acme/ui" = { path = "../ui" }`: a local directory, relative to the
    /// manifest that declares it. An optional `version` is checked against
    /// the target's version.
    Path {
        /// The directory, as written.
        path: PathBuf,
        /// Optional requirement on the target's version.
        version: Option<VersionReq>,
    },
}

/// `[registries]`: which registry serves which scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registries {
    /// `default = "..."`: the registry for scopes without their own entry.
    pub default: Option<String>,
    /// `"@scope" = "..."`, keyed by scope without `@`.
    pub scopes: BTreeMap<String, String>,
}

impl Registries {
    /// The registry URL (without trailing slash) serving `name`.
    pub fn url_for(&self, name: &PackageName) -> String {
        let url = self
            .scopes
            .get(name.scope())
            .or(self.default.as_ref())
            .map(String::as_str)
            .unwrap_or(DEFAULT_REGISTRY);
        normalize_url(url)
    }
}

/// The package-related parts of a `luat.toml`. Other sections (`[project]`,
/// `[dev]`, …) are ignored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageManifest {
    /// `[package]`, present for packages.
    pub package: Option<PackageMeta>,
    /// `[dependencies]`.
    pub dependencies: BTreeMap<PackageName, Dependency>,
    /// `[registries]`.
    pub registries: Registries,
}

#[derive(Deserialize)]
struct Raw {
    package: Option<RawPackage>,
    #[serde(default)]
    dependencies: BTreeMap<String, RawDependency>,
    #[serde(default)]
    registries: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    luat: Option<String>,
    include: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDependency {
    Version(String),
    Table { version: Option<String>, path: Option<String> },
}

fn req(s: &str, what: &str) -> Result<VersionReq> {
    VersionReq::parse(s).map_err(|e| PackageError::Manifest(format!("invalid version requirement '{s}' for {what}: {e}")))
}

impl PackageManifest {
    /// Parses the package-related sections of a `luat.toml`.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: Raw = toml::from_str(text).map_err(|e| PackageError::Manifest(format!("{MANIFEST_NAME}: {e}")))?;
        let package = raw.package.map(RawPackage::validate).transpose()?;

        let mut dependencies = BTreeMap::new();
        for (name, dep) in raw.dependencies {
            let parsed = PackageName::parse(&name)?;
            let dep = match dep {
                RawDependency::Version(v) => Dependency::Registry(req(&v, &name)?),
                RawDependency::Table { path: Some(path), version } => Dependency::Path {
                    path: PathBuf::from(path),
                    version: version.map(|v| req(&v, &name)).transpose()?,
                },
                RawDependency::Table { path: None, version: Some(v) } => Dependency::Registry(req(&v, &name)?),
                RawDependency::Table { path: None, version: None } => {
                    return Err(PackageError::Manifest(format!(
                        "dependency {name} needs a version requirement or a path"
                    )))
                }
            };
            dependencies.insert(parsed, dep);
        }

        let mut registries = Registries::default();
        for (key, url) in raw.registries {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(PackageError::Manifest(format!("registry '{key}' must be an http(s) URL, got '{url}'")));
            }
            if key == "default" {
                registries.default = Some(normalize_url(&url));
            } else {
                let scope = key.strip_prefix('@').filter(|s| crate::package_paths::is_valid_name_part(s));
                let scope = scope.ok_or_else(|| {
                    PackageError::Manifest(format!("[registries] keys are \"default\" or \"@scope\", got '{key}'"))
                })?;
                registries.scopes.insert(scope.to_string(), normalize_url(&url));
            }
        }
        Ok(Self { package, dependencies, registries })
    }

    /// Reads `<dir>/luat.toml`.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&path).ctx(|| format!("reading {}", path.display()))?;
        Self::parse(&text).map_err(|e| match e {
            PackageError::Manifest(m) => PackageError::Manifest(format!("{}: {m}", path.display())),
            other => other,
        })
    }

    /// The `[package]` section, or an error saying it is required.
    pub fn package(&self) -> Result<&PackageMeta> {
        self.package
            .as_ref()
            .ok_or_else(|| PackageError::Manifest(format!("{MANIFEST_NAME} has no [package] section")))
    }

    /// Registry dependencies only.
    pub fn registry_dependencies(&self) -> impl Iterator<Item = (&PackageName, &VersionReq)> {
        self.dependencies.iter().filter_map(|(n, d)| match d {
            Dependency::Registry(r) => Some((n, r)),
            Dependency::Path { .. } => None,
        })
    }
}

impl RawPackage {
    fn validate(self) -> Result<PackageMeta> {
        let name = PackageName::parse(&self.name)?;
        let version = Version::parse(&self.version)
            .map_err(|e| PackageError::Manifest(format!("invalid version '{}' for {name}: {e}", self.version)))?;
        Ok(PackageMeta {
            luat: self.luat.as_deref().map(|l| req(l, "luat")).transpose()?,
            name,
            version,
            description: self.description,
            license: self.license,
            repository: self.repository,
            keywords: self.keywords,
            include: self.include,
        })
    }
}

/// Sets `[dependencies]."<name>"` in a `luat.toml` text, keeping its
/// formatting and comments.
pub(crate) fn set_dependency(text: &str, name: &PackageName, value: toml_edit::Value) -> Result<String> {
    let mut doc = edit_doc(text)?;
    let deps = doc
        .entry("dependencies")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| PackageError::Manifest("[dependencies] is not a table".to_string()))?;
    deps.insert(&name.to_string(), toml_edit::Item::Value(value));
    Ok(doc.to_string())
}

/// Removes `[dependencies]."<name>"`; returns the new text and whether it
/// was present.
pub(crate) fn remove_dependency(text: &str, name: &PackageName) -> Result<(String, bool)> {
    let mut doc = edit_doc(text)?;
    let removed = doc
        .get_mut("dependencies")
        .and_then(|d| d.as_table_like_mut())
        .and_then(|d| d.remove(&name.to_string()))
        .is_some();
    Ok((doc.to_string(), removed))
}

fn edit_doc(text: &str) -> Result<toml_edit::DocumentMut> {
    text.parse().map_err(|e| PackageError::Manifest(format!("{MANIFEST_NAME}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = r#"
[package]
name = "@acme/ui"
version = "1.2.0"
luat = ">=0.1"
include = ["src/**", "README.md"]

[dependencies]
"@acme/icons" = "^2.0"
"@acme/local" = { path = "../local" }

[registries]
default = "https://example.com/"
"@private" = "https://private.example.com/luat"
"#;

    #[test]
    fn parses_a_package_manifest() {
        let m = PackageManifest::parse(PACKAGE).unwrap();
        let p = m.package().unwrap();
        assert_eq!((p.name.to_string(), p.version.to_string()), ("@acme/ui".into(), "1.2.0".into()));
        assert_eq!(m.dependencies.len(), 2);
        assert!(matches!(m.dependencies[&PackageName::parse("@acme/local").unwrap()], Dependency::Path { .. }));
        assert_eq!(m.registries.url_for(&PackageName::parse("@acme/x").unwrap()), "https://example.com");
        assert_eq!(
            m.registries.url_for(&PackageName::parse("@private/x").unwrap()),
            "https://private.example.com/luat"
        );
        assert_eq!(PackageManifest::default().registries.url_for(&p.name), DEFAULT_REGISTRY);
    }

    #[test]
    fn rejects_bad_manifests() {
        for bad in [
            "[package]\nname = \"ui\"\nversion = \"1.0.0\"",
            "[package]\nname = \"@a/ui\"\nversion = \"1.0\"",
            "[dependencies]\n\"@a/b\" = \"not a req\"",
            "[dependencies]\n\"a/b\" = \"^1\"",
            "[registries]\nscope = \"https://x\"",
            "[registries]\ndefault = \"ftp://x\"",
        ] {
            assert!(PackageManifest::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn edits_keep_formatting() {
        let text = "# my app\n[project]\nname = \"x\"\n";
        let name = PackageName::parse("@acme/ui").unwrap();
        let added = set_dependency(text, &name, "^1.2.0".into()).unwrap();
        assert!(added.starts_with("# my app\n[project]"));
        assert!(added.contains("\"@acme/ui\" = \"^1.2.0\""), "{added}");
        let (removed, was) = remove_dependency(&added, &name).unwrap();
        assert!(was);
        assert!(!removed.contains("@acme/ui"));
    }
}
