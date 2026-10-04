// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Installing a lockfile's packages into `.luat/packages/@scope/name/`.
//!
//! Each package is written into a fresh temporary directory next to its
//! destination and swapped in by renaming, so a failed download or a bad
//! tarball never leaves a half-installed package behind.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use semver::Version;

use super::error::{IoContext, PackageError, Result};
use super::files::package_files;
use super::lockfile::{LockedPackage, Lockfile};
use super::manifest::PackageManifest;
use super::name::PackageName;
use super::tarball::extract;
use super::Settings;

/// Marker file inside an installed package: the checksum it was installed
/// from (`path` for path dependencies).
const MARKER: &str = ".luat-installed";

/// What an install changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstallReport {
    /// Packages written (downloaded, or copied from a path).
    pub installed: Vec<(PackageName, Version)>,
    /// Packages removed because they left the graph.
    pub removed: Vec<PackageName>,
    /// Packages already installed at the locked version.
    pub unchanged: usize,
}

/// True when every package of `lock` is installed as locked and nothing
/// else is (path dependencies count as installed once present).
pub(crate) fn is_installed(packages_dir: &Path, lock: &Lockfile) -> bool {
    let wanted: BTreeSet<String> = lock.packages.iter().map(|p| p.name.to_string()).collect();
    let all_present = lock.packages.iter().all(|p| {
        let marker = std::fs::read_to_string(packages_dir.join(p.name.to_string()).join(MARKER)).ok();
        marker.as_deref() == Some(expected_marker(p))
    });
    let installed: BTreeSet<String> = installed_names(packages_dir).into_iter().map(|n| n.to_string()).collect();
    all_present && installed == wanted
}

fn expected_marker(p: &LockedPackage) -> &str {
    p.checksum.as_deref().unwrap_or("path")
}

/// Installs every package of `lock` below `packages_dir` and removes the
/// ones not in it. Path dependencies are re-copied every time.
pub(crate) async fn install(project: &Path, packages_dir: &Path, lock: &Lockfile, settings: &Settings) -> Result<InstallReport> {
    ensure_real_dir(packages_dir)?;
    let mut report = InstallReport::default();
    for package in &lock.packages {
        let dest = packages_dir.join(package.name.to_string());
        if let Some(rel) = package.path_source() {
            copy_path_package(&project.join(rel), packages_dir, &dest)?;
        } else {
            let checksum = package.checksum.as_deref().expect("validated lockfile");
            if std::fs::read_to_string(dest.join(MARKER)).ok().as_deref() == Some(checksum) {
                report.unchanged += 1;
                continue;
            }
            let registry = package.registry.as_deref().expect("validated lockfile");
            let bytes = settings.client(registry).download(&package.name, &package.version, checksum).await?;
            let temp = temp_dir(packages_dir);
            let result = extract(&bytes, &temp).and_then(|_| check_manifest(&temp, package));
            if let Err(e) = result {
                let _ = std::fs::remove_dir_all(&temp);
                return Err(e);
            }
            std::fs::write(temp.join(MARKER), checksum).ctx(|| "writing install marker".to_string())?;
            swap_in(&temp, &dest)?;
        }
        report.installed.push((package.name.clone(), package.version.clone()));
    }

    let wanted: BTreeSet<&PackageName> = lock.packages.iter().map(|p| &p.name).collect();
    for name in installed_names(packages_dir) {
        if !wanted.contains(&name) {
            let dir = packages_dir.join(name.to_string());
            remove_any(&dir).ctx(|| format!("removing {}", dir.display()))?;
            report.removed.push(name);
        }
    }
    remove_leftovers(packages_dir);
    Ok(report)
}

/// The tarball's `luat.toml` must describe the locked package.
fn check_manifest(root: &Path, package: &LockedPackage) -> Result<()> {
    let manifest = PackageManifest::load(root)?;
    let meta = manifest.package()?;
    if meta.name != package.name || meta.version != package.version {
        return Err(PackageError::Tarball(format!(
            "tarball for {}@{} contains {}@{}",
            package.name, package.version, meta.name, meta.version
        )));
    }
    Ok(())
}

/// Copies what `pack` would ship from a path dependency's directory.
fn copy_path_package(source: &Path, packages_dir: &Path, dest: &Path) -> Result<()> {
    let source = std::fs::canonicalize(source).ctx(|| format!("resolving {}", source.display()))?;
    let manifest = PackageManifest::load(&source)?;
    let include = manifest.package()?.include.clone();
    let files = package_files(&source, include.as_deref())?;
    let temp = temp_dir(packages_dir);
    let copy = || -> Result<()> {
        for (rel, abs) in &files.files {
            let canonical = std::fs::canonicalize(abs).ctx(|| format!("resolving {}", abs.display()))?;
            if !canonical.starts_with(&source) {
                return Err(PackageError::Tarball(format!("{rel} escapes {}", source.display())));
            }
            let target = temp.join(rel);
            std::fs::create_dir_all(target.parent().expect("file below temp")).ctx(|| format!("creating dir for {rel}"))?;
            std::fs::copy(&canonical, &target).ctx(|| format!("copying {rel}"))?;
        }
        std::fs::write(temp.join(MARKER), "path").ctx(|| "writing install marker".to_string())
    };
    std::fs::create_dir_all(&temp).ctx(|| format!("creating {}", temp.display()))?;
    if let Err(e) = copy() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(e);
    }
    swap_in(&temp, dest)
}

/// Replaces `dest` with `temp` by renames.
fn swap_in(temp: &Path, dest: &Path) -> Result<()> {
    let scope = dest.parent().expect("dest has a scope dir");
    std::fs::create_dir_all(scope).ctx(|| format!("creating {}", scope.display()))?;
    ensure_real_dir(scope)?;
    let old = temp.with_extension("old");
    let had_old = match std::fs::symlink_metadata(dest) {
        Ok(_) => {
            std::fs::rename(dest, &old).ctx(|| format!("moving {} aside", dest.display()))?;
            true
        }
        Err(_) => false,
    };
    std::fs::rename(temp, dest).ctx(|| format!("installing into {}", dest.display()))?;
    if had_old {
        let _ = remove_any(&old);
    }
    Ok(())
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) => Err(e),
    }
}

/// Refuses to write through a symlinked packages or scope directory.
fn ensure_real_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).ctx(|| format!("creating {}", dir.display()))?;
    let meta = std::fs::symlink_metadata(dir).ctx(|| format!("inspecting {}", dir.display()))?;
    if !meta.is_dir() {
        return Err(PackageError::Manifest(format!("{} must be a directory, not a link", dir.display())));
    }
    Ok(())
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(packages_dir: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    packages_dir.join(format!(".tmp-{}-{n}-{nanos}", std::process::id()))
}

/// Package names installed below `packages_dir` (scope and package
/// directories that are valid names; symlinks count, so they get removed).
pub(crate) fn installed_names(packages_dir: &Path) -> Vec<PackageName> {
    let mut names = Vec::new();
    let Ok(scopes) = std::fs::read_dir(packages_dir) else { return names };
    for scope in scopes.flatten() {
        let scope_name = scope.file_name().to_string_lossy().into_owned();
        if !scope_name.starts_with('@') || !scope.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(packages) = std::fs::read_dir(scope.path()) else { continue };
        for package in packages.flatten() {
            let full = format!("{scope_name}/{}", package.file_name().to_string_lossy());
            if let Ok(name) = PackageName::parse(&full) {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

/// Removes temporary directories of interrupted installs and empty scopes.
fn remove_leftovers(packages_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(packages_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".tmp-") {
            let _ = remove_any(&entry.path());
        } else if name.starts_with('@') {
            let _ = std::fs::remove_dir(entry.path()); // only succeeds when empty
        }
    }
}
