// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Client assets with content-hashed file names.
//!
//! The browser side of a project (JavaScript, TypeScript, CSS) is built from
//! a list of entries into [`IMMUTABLE_DIR`] with the content hash in every
//! file name, so hosts can cache those files forever: a changed file is a new
//! URL. esbuild bundles JS/TS entries (npm imports included, dynamic imports
//! as chunks of their own); CSS entries go through Tailwind when it is given,
//! otherwise through esbuild.
//!
//! The [`AssetManifest`] maps each entry, by its project-relative source
//! path, to the URLs it produced. It is embedded into the bundle, where
//! templates read it with `asset("src/client/app.js")` and the app shell
//! gets the entry tags in `%luat.head%`.

mod manifest;
mod tools;

pub use manifest::{AssetEntry, AssetManifest};
pub use tools::find_project_tool;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use std::collections::BTreeMap;

use crate::error::{LuatError, Result};

/// Directory (relative to the static output root) holding the hashed files.
/// Its URL is `/` followed by this path.
pub const IMMUTABLE_DIR: &str = "_luat/immutable";

/// What to build.
#[derive(Debug, Clone)]
pub struct AssetOptions {
    /// Project root; entries are relative to it.
    pub project_dir: PathBuf,
    /// Entry files, relative to the project root (`src/client/app.js`).
    pub entries: Vec<String>,
    /// Static output root; files are written to `<out_dir>/_luat/immutable`.
    /// A successful build replaces what a previous build left there; a
    /// failed one leaves it alone.
    pub out_dir: PathBuf,
    /// The esbuild executable.
    pub esbuild: PathBuf,
    /// The Tailwind CSS executable; when set, CSS entries go through it.
    pub tailwind: Option<PathBuf>,
    /// Minify, and write source maps as separate files.
    pub production: bool,
}

/// Builds the entries and returns the manifest.
pub fn build_assets(options: &AssetOptions) -> Result<AssetManifest> {
    // Build next to the previous output, then swap.
    let staging = options.out_dir.join("_luat").join(".staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    let out = staging.join(IMMUTABLE_DIR);
    fs::create_dir_all(&out)?;
    let built = build_into(options, &out);
    if built.is_ok() {
        let target = options.out_dir.join(IMMUTABLE_DIR);
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(&out, &target)?;
    }
    fs::remove_dir_all(&staging)?;
    built
}

fn build_into(options: &AssetOptions, out: &Path) -> Result<AssetManifest> {
    let (css, scripts): (Vec<&String>, Vec<&String>) = options
        .entries
        .iter()
        .partition(|e| e.ends_with(".css") && options.tailwind.is_some());

    let mut manifest = AssetManifest::default();
    if !scripts.is_empty() {
        esbuild(options, &scripts, out, &mut manifest)?;
    }
    if let Some(tailwind) = &options.tailwind {
        for entry in css {
            let file = tailwind_css(options, tailwind, entry, out)?;
            manifest.insert(entry.clone(), AssetEntry { file, ..Default::default() });
        }
    }
    Ok(manifest)
}

/// The URL of a built file, from its path (any prefix) ending in
/// `_luat/immutable/<file>`.
fn url_of(path: &str) -> String {
    let path = path.replace('\\', "/");
    let marker = format!("{IMMUTABLE_DIR}/");
    let rel = path.rfind(&marker).map_or(path.as_str(), |i| &path[i + marker.len()..]);
    format!("/{IMMUTABLE_DIR}/{rel}")
}

#[derive(Deserialize)]
struct Metafile {
    outputs: BTreeMap<String, MetaOutput>,
}

#[derive(Deserialize)]
struct MetaOutput {
    #[serde(rename = "entryPoint")]
    entry_point: Option<String>,
    #[serde(default)]
    imports: Vec<MetaImport>,
    #[serde(rename = "cssBundle")]
    css_bundle: Option<String>,
}

#[derive(Deserialize)]
struct MetaImport {
    path: String,
    kind: String,
}

fn esbuild(options: &AssetOptions, entries: &[&String], out: &Path, manifest: &mut AssetManifest) -> Result<()> {
    let metafile = out.join(".metafile.json");
    let mut cmd = Command::new(&options.esbuild);
    cmd.current_dir(&options.project_dir)
        .args(entries.iter().map(|e| e.as_str()))
        .arg("--bundle")
        .arg("--format=esm")
        .arg("--splitting")
        .arg(format!("--outdir={}", out.display()))
        .arg(format!("--public-path=/{IMMUTABLE_DIR}"))
        .arg("--entry-names=[name]-[hash]")
        .arg("--chunk-names=chunks/[name]-[hash]")
        .arg("--asset-names=assets/[name]-[hash]")
        .arg(format!("--metafile={}", metafile.display()))
        .arg("--log-level=warning");
    for ext in ["woff", "woff2", "ttf", "otf", "eot", "svg", "png", "jpg", "jpeg", "gif", "webp", "avif"] {
        cmd.arg(format!("--loader:.{ext}=file"));
    }
    if options.production {
        cmd.arg("--minify").arg("--sourcemap=external");
    } else {
        cmd.arg("--sourcemap=linked");
    }
    run(cmd, "esbuild")?;

    let meta: Metafile = serde_json::from_slice(&fs::read(&metafile)?)
        .map_err(|e| LuatError::InvalidTemplate(format!("esbuild metafile: {e}")))?;
    fs::remove_file(&metafile)?;

    for (path, output) in &meta.outputs {
        let Some(entry) = &output.entry_point else { continue };
        if !entries.iter().any(|e| normalize(e) == normalize(entry)) {
            continue;
        }
        manifest.insert(
            normalize(entry),
            AssetEntry {
                file: url_of(path),
                css: output.css_bundle.iter().map(|c| url_of(c)).collect(),
                imports: output
                    .imports
                    .iter()
                    .filter(|i| i.kind == "import-statement")
                    .map(|i| url_of(&i.path))
                    .collect(),
            },
        );
    }
    Ok(())
}

/// Builds one CSS entry with Tailwind and writes it under its content hash.
fn tailwind_css(options: &AssetOptions, tailwind: &Path, entry: &str, out: &Path) -> Result<String> {
    let tmp = out.join(".tailwind.css");
    let mut cmd = Command::new(tailwind);
    cmd.current_dir(&options.project_dir)
        .arg("-i")
        .arg(entry)
        .arg("-o")
        .arg(&tmp);
    if options.production {
        cmd.arg("--minify");
    }
    run(cmd, "tailwind")?;

    let css = fs::read(&tmp)?;
    fs::remove_file(&tmp)?;
    let stem = Path::new(entry).file_stem().and_then(|s| s.to_str()).unwrap_or("style");
    let name = format!("{stem}-{}.css", content_hash(&css));
    fs::write(out.join(&name), css)?;
    Ok(format!("/{IMMUTABLE_DIR}/{name}"))
}

fn run(mut cmd: Command, tool: &str) -> Result<()> {
    let output = cmd
        .output()
        .map_err(|e| LuatError::InvalidTemplate(format!("{tool} could not be started: {e}")))?;
    if !output.status.success() {
        return Err(LuatError::InvalidTemplate(format!(
            "{tool} failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn normalize(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

/// An 8-character content hash in esbuild's alphabet (FNV-1a, 40 bits).
fn content_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    (0..8).map(|i| ALPHABET[((h >> (i * 5)) & 31) as usize] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_and_changes_with_content() {
        assert_eq!(content_hash(b"a{}"), content_hash(b"a{}"));
        assert_ne!(content_hash(b"a{}"), content_hash(b"b{}"));
        assert_eq!(content_hash(b"").len(), 8);
    }

    #[test]
    fn urls_come_from_the_path_below_the_immutable_dir() {
        assert_eq!(url_of("../build/_luat/.staging/_luat/immutable/app-ABC.js"), "/_luat/immutable/app-ABC.js");
        assert_eq!(url_of("x\\_luat\\immutable\\chunks\\c-D.js"), "/_luat/immutable/chunks/c-D.js");
    }
}
