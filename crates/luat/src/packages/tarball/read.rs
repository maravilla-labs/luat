// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Reading and extracting tarballs, validating every entry.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path};

use flate2::read::GzDecoder;

use super::{MAX_COMPRESSED_BYTES, MAX_FILES, MAX_UNCOMPRESSED_BYTES};
use crate::packages::error::{PackageError, Result};

/// A file read from a tarball.
#[derive(Debug, Clone)]
pub struct TarballFile {
    /// Path relative to the package root (`package/` stripped), plain
    /// `/`-separated segments.
    pub path: String,
    /// File contents.
    pub data: Vec<u8>,
}

fn bad(msg: impl Into<String>) -> PackageError {
    PackageError::Tarball(msg.into())
}

/// Reads and validates a tarball: gzip, entries under `package/`, regular
/// files and directories only, no absolute paths or `..`, no duplicates,
/// within the size and file limits. Returns the files, sorted by path.
pub fn read_tarball(bytes: &[u8]) -> Result<Vec<TarballFile>> {
    if bytes.len() as u64 > MAX_COMPRESSED_BYTES {
        return Err(bad(format!("{} bytes compressed, over the limit", bytes.len())));
    }
    // Bound decompression itself, not just what entries claim.
    let decoder = GzDecoder::new(bytes).take(MAX_UNCOMPRESSED_BYTES + 64 * 1024 * 1024);
    let mut archive = tar::Archive::new(decoder);
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total: u64 = 0;

    let entries = archive.entries().map_err(|e| bad(format!("not a tar.gz: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| bad(format!("corrupt entry: {e}")))?;
        let raw_path = entry.path().map_err(|e| bad(format!("bad entry path: {e}")))?.into_owned();
        let shown = raw_path.display().to_string();
        let entry_type = entry.header().entry_type();
        let rel = package_relative(&raw_path).ok_or_else(|| bad(format!("entry '{shown}' is not a plain path under package/")))?;

        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(bad(format!("entry '{shown}' is not a regular file ({entry_type:?})")));
        }
        let Some(rel) = rel else {
            return Err(bad(format!("entry '{shown}' is the package directory itself")));
        };
        if !seen.insert(rel.clone()) {
            return Err(bad(format!("duplicate entry '{shown}'")));
        }
        if seen.len() > MAX_FILES {
            return Err(bad(format!("more than {MAX_FILES} files")));
        }
        let remaining = MAX_UNCOMPRESSED_BYTES.saturating_sub(total);
        let mut data = Vec::new();
        (&mut entry)
            .take(remaining + 1)
            .read_to_end(&mut data)
            .map_err(|e| bad(format!("reading '{shown}': {e}")))?;
        total += data.len() as u64;
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err(bad("files exceed the uncompressed size limit"));
        }
        files.push(TarballFile { path: rel, data });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// `Some(None)` for `package` itself, `Some(Some(rel))` for paths below it,
/// `None` for anything else (absolute, `..`, outside `package/`, odd names).
fn package_relative(path: &Path) -> Option<Option<String>> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "package" => {}
        _ => return None,
    }
    let mut parts = Vec::new();
    for component in components {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                if part.contains(['\\', '\0', ':']) {
                    return None;
                }
                parts.push(part);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some((!parts.is_empty()).then(|| parts.join("/")))
}

/// Validates `bytes` and writes its files below `dest`, which must not
/// exist yet (its parent must). Nothing is written through a symlink:
/// directories are created one level at a time and files with
/// `create_new`.
pub fn extract(bytes: &[u8], dest: &Path) -> Result<Vec<String>> {
    let files = read_tarball(bytes)?;
    crate::packages::safe_fs::new_dir(dest)?;
    for file in &files {
        crate::packages::safe_fs::write_new_below(dest, &file.path, &file.data)?;
    }
    Ok(files.into_iter().map(|f| f.path).collect())
}
