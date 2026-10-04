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
use super::{safe_fs, Settings};

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
        let dir = packages_dir.join(p.name.to_string());
        if safe_fs::not_a_link(&dir).is_err() || safe_fs::not_a_link(dir.parent().expect("scope")).is_err() {
            return false; // install refuses it
        }
        let marker = std::fs::read_to_string(dir.join(MARKER)).ok();
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
    prepare_packages_dir(project, packages_dir)?;
    let mut report = InstallReport::default();
    for package in &lock.packages {
        let dest = packages_dir.join(package.name.to_string());
        safe_fs::real_dir(dest.parent().expect("scope dir"))?;
        safe_fs::not_a_link(&dest)?;
        if let Some(rel) = package.path_source() {
            copy_path_package(&project.join(rel), packages_dir, &dest)?;
        } else {
            let checksum = package.checksum.as_deref().expect("validated lockfile");
            if std::fs::read_to_string(dest.join(MARKER)).ok().as_deref() == Some(checksum) {
                report.unchanged += 1;
                continue;
            }
            let registry = package.registry.as_deref().expect("validated lockfile");
            // Reads are anonymous: installing never sends credentials.
            let client = settings.client(registry)?;
            verify_index_checksum(&client, package, checksum).await?;
            let bytes = client.download(&package.name, &package.version, checksum).await?;
            let temp = temp_dir(packages_dir);
            let result = extract(&bytes, &temp)
                .and_then(|_| check_manifest(&temp, package))
                .and_then(|_| safe_fs::write_new_below(&temp, MARKER, checksum.as_bytes()));
            if let Err(e) = result {
                let _ = remove_any(&temp);
                return Err(e);
            }
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

/// `<project>/.luat` and `.luat/packages` must be real directories.
fn prepare_packages_dir(project: &Path, packages_dir: &Path) -> Result<()> {
    if packages_dir == project.join(super::PACKAGES_DIR) {
        safe_fs::real_dir(&project.join(".luat"))?;
    } else if let Some(parent) = packages_dir.parent() {
        std::fs::create_dir_all(parent).ctx(|| format!("creating {}", parent.display()))?;
    }
    safe_fs::real_dir(packages_dir)
}

/// The lockfile's checksum must also be what the registry's index says
/// for that version, so a tampered lockfile cannot pin other bytes.
async fn verify_index_checksum(client: &super::RegistryClient, package: &LockedPackage, checksum: &str) -> Result<()> {
    let index = client.index(&package.name).await?;
    let entry = index.iter().find(|e| e.vers == package.version).ok_or_else(|| {
        PackageError::NotFound(format!("{}@{} is not in the index of {}", package.name, package.version, client.base_url()))
    })?;
    if !entry.cksum.eq_ignore_ascii_case(checksum) {
        return Err(PackageError::ChecksumMismatch {
            name: package.name.to_string(),
            version: format!("{} (luat.lock vs registry index)", package.version),
            expected: checksum.to_string(),
            actual: entry.cksum.clone(),
        });
    }
    Ok(())
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

/// Copies what `pack` would ship from a path dependency's directory:
/// regular files only (symlinks are skipped by `package_files`, and each
/// file is re-checked), canonically inside the package, written without
/// following links.
fn copy_path_package(source: &Path, packages_dir: &Path, dest: &Path) -> Result<()> {
    let source = std::fs::canonicalize(source).ctx(|| format!("resolving {}", source.display()))?;
    let manifest = PackageManifest::load(&source)?;
    let include = manifest.package()?.include.clone();
    let files = package_files(&source, include.as_deref())?;
    let temp = temp_dir(packages_dir);
    safe_fs::new_dir(&temp)?;
    let copy = || -> Result<()> {
        for (rel, abs) in &files.files {
            let meta = std::fs::symlink_metadata(abs).ctx(|| format!("inspecting {}", abs.display()))?;
            let canonical = std::fs::canonicalize(abs).ctx(|| format!("resolving {}", abs.display()))?;
            if !meta.is_file() || !canonical.starts_with(&source) {
                return Err(PackageError::Tarball(format!("{rel} is not a regular file inside {}", source.display())));
            }
            let data = std::fs::read(&canonical).ctx(|| format!("reading {rel}"))?;
            safe_fs::write_new_below(&temp, rel, &data)?;
        }
        safe_fs::write_new_below(&temp, MARKER, b"path")
    };
    if let Err(e) = copy() {
        let _ = remove_any(&temp);
        return Err(e);
    }
    swap_in(&temp, dest)
}

/// Replaces `dest` with `temp` by renames. `dest`'s scope directory and
/// `dest` itself must not be symlinks.
fn swap_in(temp: &Path, dest: &Path) -> Result<()> {
    let scope = dest.parent().expect("dest has a scope dir");
    safe_fs::real_dir(scope)?;
    safe_fs::not_a_link(dest)?;
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
