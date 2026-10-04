// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Case-exact file lookups.
//!
//! macOS and Windows filesystems are usually case-insensitive: there
//! `Path::new("variants.luat").exists()` is true when the file on disk is
//! `Variants.luat`. Module resolution must not depend on that, or
//! `require("./variants")` works in development on one machine and fails in
//! production (or on Linux). The helpers here only accept a path whose
//! components match the directory entries on disk byte for byte.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// True when `path` exists and its last `depth` components (at least the
/// file name) are spelled exactly like the entries on disk.
///
/// `depth` covers the components that came from the module name; the
/// components above them (the project root, which the user configured) are
/// taken as given.
pub(crate) fn exists_exact(path: &Path, depth: usize) -> bool {
    if !path.exists() {
        return false;
    }
    let path = lexical_normalize(path);
    let mut current = path.as_path();
    for _ in 0..depth.max(1) {
        let (Some(name), Some(parent)) = (current.file_name(), current.parent()) else {
            break;
        };
        if !dir_has_entry(parent, name) {
            return false;
        }
        current = parent;
    }
    true
}

/// [`exists_exact`] for a regular file.
pub(crate) fn is_file_exact(path: &Path, depth: usize) -> bool {
    path.is_file() && exists_exact(path, depth)
}

/// Number of named components in `module` after removing `.` and resolving
/// `..` lexically: how many trailing components of a resolved path came from
/// the module name.
pub(crate) fn module_depth(module: &str) -> usize {
    let mut depth: usize = 0;
    for component in Path::new(module).components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth.max(1)
}

/// True when directory `dir` has an entry spelled exactly `name`.
pub(crate) fn dir_has_entry(dir: &Path, name: &OsStr) -> bool {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().any(|entry| entry.file_name() == name))
        .unwrap_or(false)
}

/// Removes `.` and resolves `..` without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exact_names_are_found() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("ui/x")).unwrap();
        fs::write(dir.path().join("ui/Card.luat"), "").unwrap();
        assert!(is_file_exact(&dir.path().join("ui/Card.luat"), 2));
        assert!(is_file_exact(&dir.path().join("ui/./x/../Card.luat"), 2));
        assert!(!is_file_exact(&dir.path().join("ui/Missing.luat"), 1));
    }

    #[test]
    fn entry_names_compare_exactly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Variants.luat"), "").unwrap();
        assert!(dir_has_entry(dir.path(), OsStr::new("Variants.luat")));
        assert!(!dir_has_entry(dir.path(), OsStr::new("variants.luat")));
    }

    /// On a case-insensitive filesystem (macOS, Windows) `exists()` is true
    /// for the wrong case; the exact check must still say no. On a
    /// case-sensitive one both say no.
    #[test]
    fn wrong_case_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Lib")).unwrap();
        fs::write(dir.path().join("Lib/Variants.luat"), "").unwrap();
        assert!(!is_file_exact(&dir.path().join("Lib/variants.luat"), 1));
        assert!(!is_file_exact(&dir.path().join("lib/Variants.luat"), 2));
        // The directory is above the module's depth: taken as given.
        let wrong_dir = dir.path().join("lib/Variants.luat");
        assert_eq!(is_file_exact(&wrong_dir, 1), wrong_dir.is_file());
    }

    #[test]
    fn depth_counts_module_components() {
        assert_eq!(module_depth("./variants"), 1);
        assert_eq!(module_depth("../ui/Card"), 2);
        assert_eq!(module_depth("a/../b"), 1);
        assert_eq!(module_depth("components/ui/Button.luat"), 3);
    }
}
