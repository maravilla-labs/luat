// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Which files of a package directory ship (in a tarball, or copied for a
//! path dependency).

use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use super::error::{PackageError, Result};
use super::manifest::MANIFEST_NAME;

/// Files shipped when `[package] include` is not set. `luat.toml` is always
/// shipped, whatever `include` says.
pub const DEFAULT_INCLUDE: &[&str] = &["src/**", "README*", "LICENSE*", "CHANGELOG*", MANIFEST_NAME];

/// Directories never descended into.
const PRUNED: &[&str] = &[".git", ".luat", "node_modules"];

/// The files of a package directory selected by its include rules.
#[derive(Debug, Clone, Default)]
pub struct PackageFiles {
    /// `(relative path with / separators, absolute path)`, sorted.
    pub files: Vec<(String, PathBuf)>,
    /// Paths matched by `include` but skipped: symlinks and names that are
    /// not plain UTF-8 segments.
    pub skipped: Vec<String>,
}

/// Lists the files of the package rooted at `dir` matched by `include`
/// (or [`DEFAULT_INCLUDE`]). Symlinks are never followed nor shipped.
pub fn package_files(dir: &Path, include: Option<&[String]>) -> Result<PackageFiles> {
    let patterns: Vec<String> = match include {
        Some(globs) => globs.iter().cloned().chain([MANIFEST_NAME.to_string()]).collect(),
        None => DEFAULT_INCLUDE.iter().map(|s| s.to_string()).collect(),
    };
    let set = glob_set(&patterns)?;
    let mut out = PackageFiles::default();
    let walker = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !(e.file_type().is_dir() && PRUNED.iter().any(|p| e.file_name() == *p)));
    for entry in walker {
        let entry = entry.map_err(|e| PackageError::Manifest(format!("walking {}: {e}", dir.display())))?;
        if entry.depth() == 0 || entry.file_type().is_dir() {
            continue;
        }
        let rel_path = entry.path().strip_prefix(dir).expect("walked below dir");
        let rel = plain(rel_path);
        let display = rel.clone().unwrap_or_else(|| rel_path.display().to_string());
        if !set.is_match(&display) {
            continue;
        }
        match rel {
            Some(rel) if entry.file_type().is_file() => out.files.push((rel, entry.path().to_path_buf())),
            _ => out.skipped.push(display),
        }
    }
    Ok(out)
}

fn glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern.trim_start_matches("./"))
            .literal_separator(true)
            .build()
            .map_err(|e| PackageError::Manifest(format!("invalid include pattern '{pattern}': {e}")))?;
        builder.add(glob);
    }
    builder.build().map_err(|e| PackageError::Manifest(format!("invalid include patterns: {e}")))
}

/// `rel` as `/`-joined plain UTF-8 segments.
pub(crate) fn plain(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in rel.components() {
        let Component::Normal(part) = component else { return None };
        let part = part.to_str()?;
        if part.contains(['\\', '\0', ':']) {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn files_of(dir: &Path, include: Option<&[String]>) -> Vec<String> {
        package_files(dir, include).unwrap().files.into_iter().map(|(r, _)| r).collect()
    }

    #[test]
    fn default_and_custom_includes() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["luat.toml", "README.md", "LICENSE", "notes.txt", "src/a.lua", "src/x/b.luat", "docs/README.md", ".luat/packages/x"] {
            let p = dir.path().join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "x").unwrap();
        }
        assert_eq!(
            files_of(dir.path(), None),
            ["LICENSE", "README.md", "luat.toml", "src/a.lua", "src/x/b.luat"]
        );
        let include = vec!["src/x/**".to_string()];
        assert_eq!(files_of(dir.path(), Some(&include)), ["luat.toml", "src/x/b.luat"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("luat.toml"), "").unwrap();
        std::os::unix::fs::symlink("/etc/hosts", dir.path().join("src/hosts.lua")).unwrap();
        std::os::unix::fs::symlink("/etc", dir.path().join("src/etc")).unwrap();
        let found = package_files(dir.path(), None).unwrap();
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.skipped, ["src/etc", "src/hosts.lua"]);
    }
}
