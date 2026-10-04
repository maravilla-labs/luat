// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Path dependencies: `"@acme/ui" = { path = "../ui" }`.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use semver::Version;

use super::error::{IoContext, PackageError, Result};
use super::lockfile::PATH_SOURCE_PREFIX;
use super::manifest::{Dependency, PackageManifest};
use super::name::PackageName;

/// A package used from a local directory.
#[derive(Debug, Clone)]
pub(crate) struct PathPackage {
    pub name: PackageName,
    pub version: Version,
    /// Canonical package root.
    pub dir: PathBuf,
    pub manifest: PackageManifest,
    /// Lockfile `source`, `path+<dir relative to the project>`.
    pub source: String,
    /// Who first asked for it, for error messages.
    pub required_by: String,
}

/// Collects every path dependency reachable from the project's manifest
/// through other path dependencies. Each path is relative to the manifest
/// that declares it, and the target's `[package] name` must equal the key.
pub(crate) fn collect(project: &Path, manifest: &PackageManifest) -> Result<BTreeMap<PackageName, PathPackage>> {
    let project = std::fs::canonicalize(project).ctx(|| format!("resolving {}", project.display()))?;
    let mut found: BTreeMap<PackageName, PathPackage> = BTreeMap::new();
    let mut queue: Vec<(PathBuf, PackageManifest, String)> = vec![(project.clone(), manifest.clone(), "luat.toml".into())];

    while let Some((base, manifest, requirer)) = queue.pop() {
        for (name, dep) in &manifest.dependencies {
            let Dependency::Path { path, version } = dep else { continue };
            let dir = std::fs::canonicalize(base.join(path)).map_err(|e| {
                PackageError::Manifest(format!(
                    "path dependency {name} ({}) of {requirer}: {e}",
                    path.display()
                ))
            })?;
            if let Some(existing) = found.get(name) {
                if existing.dir != dir {
                    return Err(PackageError::Resolution(format!(
                        "{name} is used from two paths: {} (by {}) and {} (by {requirer})",
                        existing.dir.display(),
                        existing.required_by,
                        dir.display()
                    )));
                }
                continue;
            }
            let target = PackageManifest::load(&dir)?;
            let meta = target.package().map_err(|_| {
                PackageError::Manifest(format!("path dependency {name} at {} has no [package] section", dir.display()))
            })?;
            if &meta.name != name {
                return Err(PackageError::Manifest(format!(
                    "path dependency {name} of {requirer} points to {}, which is package {}",
                    dir.display(),
                    meta.name
                )));
            }
            if let Some(req) = version.as_ref().filter(|r| !r.matches(&meta.version)) {
                return Err(PackageError::Resolution(format!(
                    "{name} at {} is version {}, but {requirer} requires {req}",
                    dir.display(),
                    meta.version
                )));
            }
            let package = PathPackage {
                name: name.clone(),
                version: meta.version.clone(),
                source: format!("{PATH_SOURCE_PREFIX}{}", relative(&project, &dir)),
                dir: dir.clone(),
                manifest: target.clone(),
                required_by: requirer.clone(),
            };
            let requirer = format!("{name}@{}", meta.version);
            queue.push((dir, target, requirer));
            found.insert(name.clone(), package);
        }
    }
    Ok(found)
}

/// `to` relative to `from` (both absolute), with `/` separators.
pub(crate) fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat("..".to_string()).take(from.len() - common).collect();
    parts.extend(to[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/a/app"), Path::new("/a/ui")), "../ui");
        assert_eq!(relative(Path::new("/a/app"), Path::new("/a/app/vendor/ui")), "vendor/ui");
        assert_eq!(relative(Path::new("/a"), Path::new("/a")), ".");
    }
}
