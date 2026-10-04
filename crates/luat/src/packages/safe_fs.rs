// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Filesystem writes that never follow symlinks.
//!
//! Installing writes below `.luat/packages` of a project that may have been
//! cloned from anywhere. A symlink planted at `.luat`, `.luat/packages`, a
//! scope or a package directory must not redirect writes elsewhere, so
//! directories are created one level at a time and checked with
//! `symlink_metadata`, and files are created with `create_new` (which fails
//! on an existing path, symlink or not).

use std::io::Write;
use std::path::{Component, Path};

use super::error::{IoContext, PackageError, Result};

fn link_error(path: &Path) -> PackageError {
    PackageError::Manifest(format!(
        "{} is a symlink or not a directory; refusing to write through it",
        path.display()
    ))
}

/// Ensures `dir` is a real directory (not a symlink), creating it (one
/// level; its parent must exist) when missing.
pub(crate) fn real_dir(dir: &Path) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(link_error(dir)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match std::fs::create_dir(dir) {
            Ok(()) => Ok(()),
            // Lost a race with another creator: re-check what is there.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => real_dir(dir),
            Err(e) => Err(e).ctx(|| format!("creating {}", dir.display())),
        },
        Err(e) => Err(e).ctx(|| format!("inspecting {}", dir.display())),
    }
}

/// Fails when `path` exists as a symlink (anything else, or nothing, is ok).
pub(crate) fn not_a_link(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(link_error(path)),
        _ => Ok(()),
    }
}

/// Creates a new, empty directory; fails when anything exists at `dir`.
pub(crate) fn new_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir(dir).ctx(|| format!("creating {}", dir.display()))
}

/// Writes `data` to `root/rel` (plain `/`-separated segments), creating the
/// parent directories below `root` one by one as real directories, and the
/// file itself with `create_new`.
pub(crate) fn write_new_below(root: &Path, rel: &str, data: &[u8]) -> Result<()> {
    let rel_path = Path::new(rel);
    if rel_path.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(PackageError::Tarball(format!("'{rel}' is not a plain relative path")));
    }
    let mut dir = root.to_path_buf();
    if let Some(parent) = rel_path.parent() {
        for component in parent.components() {
            dir.push(component);
            real_dir(&dir)?;
        }
    }
    let target = root.join(rel_path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .ctx(|| format!("creating {}", target.display()))?;
    file.write_all(data).ctx(|| format!("writing {}", target.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn writes_never_follow_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("dir")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("f"), root.path().join("f")).unwrap();
        assert!(write_new_below(root.path(), "dir/x", b"x").is_err());
        assert!(write_new_below(root.path(), "f", b"x").is_err());
        assert!(real_dir(&root.path().join("dir")).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        write_new_below(root.path(), "a/b/c.lua", b"ok").unwrap();
        assert_eq!(std::fs::read(root.path().join("a/b/c.lua")).unwrap(), b"ok");
        assert!(write_new_below(root.path(), "../x", b"x").is_err());
    }
}
