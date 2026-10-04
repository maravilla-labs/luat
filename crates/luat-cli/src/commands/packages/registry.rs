// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Registry commands: `search`, `pack`, `publish`, `login`, `yank`.

use std::io::{BufRead, Write};

use console::style;
use luat::packages::{self, PackageManifest, PackageName, Settings, MANIFEST_NAME};

/// The registry for `scope` (or the default) per the current directory's
/// `luat.toml`, or the default registry without one.
fn registry_for(name: Option<&PackageName>) -> anyhow::Result<String> {
    let dir = std::env::current_dir()?;
    let manifest = if dir.join(MANIFEST_NAME).exists() {
        PackageManifest::load(&dir)?
    } else {
        PackageManifest::default()
    };
    let probe = name.cloned().unwrap_or_else(|| PackageName::parse("@default/default").expect("valid"));
    Ok(match name {
        Some(_) => manifest.registries.url_for(&probe),
        None => manifest
            .registries
            .default
            .clone()
            .unwrap_or_else(|| packages::DEFAULT_REGISTRY.to_string()),
    })
}

/// `luat search <text>`.
pub async fn search(query: &str, registry: Option<&str>) -> anyhow::Result<()> {
    let registry = match registry {
        Some(r) => r.to_string(),
        None => registry_for(None)?,
    };
    let results = packages::search(&registry, query, 1, 20, &Settings::default()).await?;
    if results.packages.is_empty() {
        println!("No packages found for '{query}'.");
    }
    for hit in &results.packages {
        println!(
            "{} {}  {}",
            style(&hit.name).cyan().bold(),
            style(hit.latest.as_deref().unwrap_or("")).dim(),
            hit.description.as_deref().unwrap_or("")
        );
    }
    if results.total > results.packages.len() as u64 {
        println!("{}", style(format!("… {} matches in total", results.total)).dim());
    }
    Ok(())
}

/// `luat pack`: writes `<scope>-<name>-<version>.tgz`.
pub fn pack() -> anyhow::Result<()> {
    let tarball = packages::pack(&std::env::current_dir()?)?;
    for skipped in &tarball.skipped {
        eprintln!("{} {skipped} (links are never packed)", style("Skipped").yellow());
    }
    let file = tarball.file_name();
    std::fs::write(&file, &tarball.bytes)?;
    println!("{} {file} ({} files, {} bytes)", style("Packed").green(), tarball.files.len(), tarball.bytes.len());
    println!("  {}", style(&tarball.checksum).dim());
    Ok(())
}

/// `luat publish [--dry-run]`.
pub async fn publish(dry_run: bool) -> anyhow::Result<()> {
    let outcome = packages::publish(&std::env::current_dir()?, dry_run, &Settings::default()).await?;
    let meta = outcome.tarball.manifest.package()?;
    for file in &outcome.tarball.files {
        println!("  {}", style(file).dim());
    }
    match outcome.result {
        Some(result) => println!(
            "{} {}@{} to {} ({})",
            style("Published").green().bold(),
            result.name,
            result.vers,
            outcome.registry,
            result.cksum
        ),
        None => println!(
            "{} {}@{} would be published to {} ({} bytes)",
            style("Dry run:").cyan(),
            meta.name,
            meta.version,
            outcome.registry,
            outcome.tarball.bytes.len()
        ),
    }
    Ok(())
}

/// `luat login [registry] [--token <token>]`; reads the token from stdin
/// when not given.
pub async fn login(registry: Option<&str>, token: Option<String>) -> anyhow::Result<()> {
    let registry = match registry {
        Some(r) => r.to_string(),
        None => registry_for(None)?,
    };
    let token = match token {
        Some(t) => t,
        None => {
            print!("Token for {registry}: ");
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line.trim().to_string()
        }
    };
    anyhow::ensure!(!token.is_empty(), "no token given");
    let me = packages::login(&registry, &token, &Settings::default()).await?;
    println!(
        "{} as {} on {} (scopes: {})",
        style("Logged in").green(),
        me.user,
        registry,
        me.scopes.join(", ")
    );
    Ok(())
}

/// `luat yank @scope/name@<version> [--undo]`.
pub async fn yank(spec: &str, undo: bool) -> anyhow::Result<()> {
    let (name, version) = spec
        .get(1..)
        .and_then(|rest| rest.find('@'))
        .map(|at| (&spec[..at + 1], &spec[at + 2..]))
        .ok_or_else(|| anyhow::anyhow!("expected @scope/name@<version>"))?;
    let name = PackageName::parse(name)?;
    let version = semver::Version::parse(version)?;
    let registry = registry_for(Some(&name))?;
    packages::yank(&registry, &name, &version, undo, &Settings::default()).await?;
    let verb = if undo { "Unyanked" } else { "Yanked" };
    println!("{} {name}@{version}", style(verb).green());
    Ok(())
}
