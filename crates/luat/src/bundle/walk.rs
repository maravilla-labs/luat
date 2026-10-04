// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Walking source directories for a bundle without leaving them.
//!
//! Bundles are published, so a build must never pull in a file from outside
//! the directories it was given: symlinks are skipped (with a warning), every
//! file read is checked to canonically live inside the walked directory, and
//! module keys derived from paths only ever contain plain segments.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::Result;

/// A regular file found under a walked directory.
pub(super) struct WalkedFile {
    /// Path relative to the walked directory, `/`-separated, plain segments.
    pub rel: String,
    /// Canonical absolute path, inside the walked directory.
    pub abs: PathBuf,
}

/// Lists the regular files under `dir`, sorted by relative path.
///
/// Symlinks (to files or directories) are skipped with a warning, as are
/// names that would not make a plain module key and files whose canonical
/// path is outside `dir`.
pub(super) fn walk(dir: &Path, warnings: &mut Vec<String>) -> Result<Vec<WalkedFile>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    let root = fs::canonicalize(dir)?;
    walk_into(&root, &root, &mut files, warnings)?;
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

fn walk_into(root: &Path, dir: &Path, out: &mut Vec<WalkedFile>, warnings: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // `DirEntry::file_type` does not follow symlinks.
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            warnings.push(format!("{}: symlinks are not followed when bundling; skipped", path.display()));
            continue;
        }
        let Some(rel) = plain_relative(root, &path) else {
            warnings.push(format!("{}: not a plain path below {}; skipped", path.display(), root.display()));
            continue;
        };
        if file_type.is_dir() {
            walk_into(root, &path, out, warnings)?;
        } else if file_type.is_file() {
            let abs = fs::canonicalize(&path)?;
            if !abs.starts_with(root) {
                warnings.push(format!("{}: resolves outside {}; skipped", path.display(), root.display()));
                continue;
            }
            out.push(WalkedFile { rel, abs });
        }
    }
    Ok(())
}

/// `path` relative to `root` as a `/`-joined string of plain segments, or
/// `None` when any segment is not plain UTF-8 or contains a separator.
fn plain_relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut segments = Vec::new();
    for component in rel.components() {
        let Component::Normal(segment) = component else { return None };
        let segment = segment.to_str()?;
        if segment.is_empty() || segment.contains(['/', '\\', '\0']) || segment == ".." || segment == "." {
            return None;
        }
        segments.push(segment);
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Reads a walked file, re-checking that it is still a regular file inside
/// `root` (it may have been swapped for a symlink since the walk).
pub(super) fn read_contained(root: &Path, file: &WalkedFile) -> Result<String> {
    let meta = fs::symlink_metadata(&file.abs)?;
    let root = fs::canonicalize(root)?;
    if !meta.is_file() || !fs::canonicalize(&file.abs)?.starts_with(&root) {
        return Err(crate::error::LuatError::InvalidTemplate(format!(
            "{}: changed during the build or escapes {}",
            file.abs.display(),
            root.display()
        )));
    }
    Ok(fs::read_to_string(&file.abs)?)
}

/// True when `prefix` is `/`-separated plain segments (no `.`, `..`, empty
/// or absolute parts).
pub(super) fn is_plain_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(['\\', '\0']))
}

/// Installed packages under `packages_dir`: `(@scope/name, src dir)`,
/// sorted. Directories that are not valid `@scope/name` packages, symlinks
/// and `src/` directories resolving outside the package are skipped with a
/// warning.
pub(super) fn installed_packages(packages_dir: &Path, warnings: &mut Vec<String>) -> Result<Vec<(String, PathBuf)>> {
    let mut packages = Vec::new();
    if !packages_dir.is_dir() {
        return Ok(packages);
    }
    // Dot-entries at the top level are the installer's own state (temporary
    // directories); anything else that is not a package is reported.
    let valid_dir = |entry: &fs::DirEntry, name: &str, top: bool, warnings: &mut Vec<String>| -> Result<bool> {
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_dir() || !crate::package_paths::is_valid_name_part(name) {
            let installer_state = top && entry.file_name().to_string_lossy().starts_with('.');
            if !installer_state || file_type.is_symlink() {
                warnings.push(format!("{}: not an installed package; skipped", entry.path().display()));
            }
            return Ok(false);
        }
        Ok(true)
    };
    for scope in fs::read_dir(packages_dir)? {
        let scope = scope?;
        let scope_name = scope.file_name().to_string_lossy().into_owned();
        let scope_part = scope_name.strip_prefix('@').unwrap_or(".");
        if !valid_dir(&scope, scope_part, true, warnings)? {
            continue;
        }
        for package in fs::read_dir(scope.path())? {
            let package = package?;
            let name = package.file_name().to_string_lossy().into_owned();
            if !valid_dir(&package, &name, false, warnings)? {
                continue;
            }
            let root = fs::canonicalize(package.path())?;
            let src = package.path().join("src");
            match fs::symlink_metadata(&src) {
                Ok(meta) if meta.is_dir() && fs::canonicalize(&src)?.starts_with(&root) => {
                    packages.push((format!("{scope_name}/{name}"), src));
                }
                Ok(_) => warnings.push(format!("{}: not a directory inside the package; skipped", src.display())),
                Err(_) => {}
            }
        }
    }
    packages.sort();
    Ok(packages)
}
