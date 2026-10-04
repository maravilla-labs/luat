// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Package tarballs: gzip-compressed tar, every entry under `package/`,
//! regular files and directories only.

mod read;

use std::io::Write;
use std::path::Path;

use flate2::{write::GzEncoder, Compression};
use sha2::{Digest, Sha256};

use super::error::{IoContext, PackageError, Result};
use super::files::package_files;
use super::manifest::PackageManifest;

pub use read::{extract, read_tarball, TarballFile};

/// Largest tarball, compressed: 10 MiB, inclusive.
pub const MAX_COMPRESSED_BYTES: u64 = 10 * 1024 * 1024;
/// Largest total size of a tarball's files: 50 MiB, inclusive.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 50 * 1024 * 1024;
/// Most files a tarball may hold, inclusive.
pub const MAX_FILES: usize = 5000;

/// A packed package.
#[derive(Debug, Clone)]
pub struct Tarball {
    /// The `.tar.gz` bytes.
    pub bytes: Vec<u8>,
    /// `sha256:<hex>` of [`bytes`](Self::bytes).
    pub checksum: String,
    /// Shipped files, relative to the package root (without `package/`).
    pub files: Vec<String>,
    /// The package's manifest.
    pub manifest: PackageManifest,
    /// Files matched by `include` but left out (symlinks, odd names).
    pub skipped: Vec<String>,
}

impl Tarball {
    /// The conventional file name, `<scope>-<name>-<version>.tgz`.
    pub fn file_name(&self) -> String {
        let p = self.manifest.package.as_ref().expect("packed packages have [package]");
        format!("{}-{}-{}.tgz", p.name.scope(), p.name.name(), p.version)
    }
}

/// `sha256:<hex>` of `bytes`, the checksum format of indexes and lockfiles.
pub fn checksum(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Packs the package rooted at `dir` (which must have a `luat.toml` with a
/// `[package]` section) into a tarball, honouring `include` and the size
/// limits. The output is deterministic: sorted entries, fixed metadata.
pub fn pack(dir: &Path) -> Result<Tarball> {
    let manifest = PackageManifest::load(dir)?;
    let meta = manifest.package()?;
    check_publishable(dir, &manifest)?;
    let selected = package_files(dir, meta.include.as_deref())?;
    if selected.files.len() > MAX_FILES {
        return Err(PackageError::Tarball(format!(
            "{} files exceed the limit of {MAX_FILES}",
            selected.files.len()
        )));
    }

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut total: u64 = 0;
    for (rel, abs) in &selected.files {
        let data = std::fs::read(abs).ctx(|| format!("reading {}", abs.display()))?;
        total += data.len() as u64;
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err(PackageError::Tarball(format!("files exceed {MAX_UNCOMPRESSED_BYTES} bytes uncompressed")));
        }
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(&mut header, format!("package/{rel}"), data.as_slice())
            .ctx(|| format!("adding {rel}"))?;
    }
    let encoder = builder.into_inner().ctx(|| "writing tarball".to_string())?;
    let mut bytes = encoder.finish().ctx(|| "compressing tarball".to_string())?;
    bytes.flush().ok();
    if bytes.len() as u64 > MAX_COMPRESSED_BYTES {
        return Err(PackageError::Tarball(format!(
            "tarball is {} bytes, over the {MAX_COMPRESSED_BYTES} byte limit",
            bytes.len()
        )));
    }
    Ok(Tarball {
        checksum: checksum(&bytes),
        bytes,
        files: selected.files.into_iter().map(|(rel, _)| rel).collect(),
        manifest,
        skipped: selected.skipped,
    })
}

/// Registries accept only plain requirement strings as dependency values,
/// never path dependencies, tables or the package itself.
fn check_publishable(dir: &Path, manifest: &PackageManifest) -> Result<()> {
    let meta = manifest.package()?;
    let path_deps: Vec<String> = manifest
        .dependencies
        .iter()
        .filter(|(_, d)| matches!(d, super::manifest::Dependency::Path { .. }))
        .map(|(n, _)| n.to_string())
        .collect();
    if !path_deps.is_empty() {
        return Err(PackageError::Manifest(format!(
            "{} has path dependencies ({}); published packages can only depend on registry versions",
            meta.name,
            path_deps.join(", ")
        )));
    }
    if manifest.dependencies.contains_key(&meta.name) {
        return Err(PackageError::Manifest(format!("{} depends on itself", meta.name)));
    }
    let path = dir.join(super::manifest::MANIFEST_NAME);
    let text = std::fs::read_to_string(&path).ctx(|| format!("reading {}", path.display()))?;
    let raw: toml::Table = toml::from_str(&text).map_err(|e| PackageError::Manifest(e.to_string()))?;
    if let Some(deps) = raw.get("dependencies").and_then(|d| d.as_table()) {
        if let Some((name, _)) = deps.iter().find(|(_, v)| !v.is_str()) {
            return Err(PackageError::Manifest(format!(
                "dependency {name} must be a plain version requirement string (e.g. \"^1.2\") to be published"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn packs_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("luat.toml"), "[package]\nname = \"@acme/ui\"\nversion = \"1.0.0\"\n").unwrap();
        fs::write(dir.path().join("src/Card.luat"), "<div/>").unwrap();
        fs::write(dir.path().join("notes.txt"), "not shipped").unwrap();

        let a = pack(dir.path()).unwrap();
        let b = pack(dir.path()).unwrap();
        assert_eq!(a.checksum, b.checksum, "packing is deterministic");
        assert!(a.checksum.starts_with("sha256:") && a.checksum.len() == 7 + 64);
        assert_eq!(a.files, ["luat.toml", "src/Card.luat"]);
        assert_eq!(a.file_name(), "acme-ui-1.0.0.tgz");

        let files = read_tarball(&a.bytes).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, ["luat.toml", "src/Card.luat"]);
        assert_eq!(files[1].data, b"<div/>");
    }

    #[test]
    fn packing_needs_plain_registry_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let base = "[package]\nname = \"@acme/ui\"\nversion = \"1.0.0\"\n[dependencies]\n";
        for (deps, msg) in [
            ("\"@acme/x\" = { version = \"^1\" }", "plain version requirement"),
            ("\"@acme/x\" = { path = \"../x\" }", "path dependencies (@acme/x)"),
            ("\"@acme/ui\" = \"^1\"", "depends on itself"),
        ] {
            fs::write(dir.path().join("luat.toml"), format!("{base}{deps}\n")).unwrap();
            let err = pack(dir.path()).unwrap_err().to_string();
            assert!(err.contains(msg), "{err}");
        }
    }

    #[test]
    fn packing_needs_a_package_section() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("luat.toml"), "[project]\nname = \"x\"\n").unwrap();
        assert!(pack(dir.path()).unwrap_err().to_string().contains("[package]"));
    }
}
