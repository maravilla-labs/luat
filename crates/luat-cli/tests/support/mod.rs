// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Shared helpers for the package tests.

#![allow(dead_code)]

pub mod registry;

use std::fs;
use std::path::{Path, PathBuf};

use luat::packages::{publish, Settings};

pub fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

/// Settings that never read the user's credentials or environment.
pub fn settings(dir: &Path) -> Settings {
    Settings {
        credentials_path: Some(dir.join("credentials.toml")),
        token: Some(registry::TOKEN.to_string()),
        luat_version: None,
    }
}

/// Writes a package below `root/<dir>` and publishes it to `url`.
pub async fn publish_package(root: &Path, url: &str, name: &str, version: &str, deps: &[(&str, &str)], files: &[(&str, &str)]) {
    let dir = root.join(format!("pkg-{}-{version}", name.replace(['@', '/'], "_")));
    let deps: String = deps.iter().map(|(n, r)| format!("\"{n}\" = \"{r}\"\n")).collect();
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"test\"\n\n[dependencies]\n{deps}\n[registries]\ndefault = \"{url}\"\n"
    );
    write(&dir, &[("luat.toml", &manifest), ("README.md", "# readme")]);
    write(&dir, files);
    let outcome = publish(&dir, false, &settings(root)).await.unwrap();
    assert_eq!(outcome.result.unwrap().vers, version);
}

/// A consuming project with `deps` and the test registry as default.
pub fn project(root: &Path, name: &str, url: &str, deps: &str) -> PathBuf {
    let dir = root.join(name);
    let manifest = format!("[project]\nname = \"{name}\"\n\n[dependencies]\n{deps}\n[registries]\ndefault = \"{url}\"\n");
    write(&dir, &[("luat.toml", &manifest)]);
    dir
}

/// A `.tar.gz` with one raw entry (path written verbatim, no validation).
pub fn raw_tarball(path: &str, entry_type: tar::EntryType, link: Option<&str>) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    let manifest = b"[package]\nname = \"@evil/pkg\"\nversion = \"1.0.0\"\n";
    let mut ok = tar::Header::new_gnu();
    ok.set_size(manifest.len() as u64);
    ok.set_mode(0o644);
    ok.set_entry_type(tar::EntryType::Regular);
    builder.append_data(&mut ok, "package/luat.toml", &manifest[..]).unwrap();

    let data = b"return 'pwned'";
    let mut header = tar::Header::new_old();
    let name = &mut header.as_old_mut().name;
    name[..path.len()].copy_from_slice(path.as_bytes());
    header.set_entry_type(entry_type);
    header.set_mode(0o644);
    if let Some(link) = link {
        let field = &mut header.as_old_mut().linkname;
        field[..link.len()].copy_from_slice(link.as_bytes());
        header.set_size(0);
        header.set_cksum();
        builder.append(&header, &[][..]).unwrap();
    } else {
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}
