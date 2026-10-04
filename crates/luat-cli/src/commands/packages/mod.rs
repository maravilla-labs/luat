// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Package commands: `add`, `remove`, `install`, `update` (this file) and
//! `search`, `pack`, `publish`, `login`, `yank` ([`registry`]).

/// Registry commands.
pub mod registry;

use std::path::{Path, PathBuf};

use console::style;
use luat::packages::{InstallReport, Lockfile, PackageName, Project, Settings};

/// The project in the current directory.
pub fn current_project() -> anyhow::Result<Project> {
    Ok(Project::new(std::env::current_dir()?).with_settings(Settings::default()))
}

fn print_report(report: &InstallReport) {
    for (name, version) in &report.installed {
        println!("  {} {name}@{version}", style("+").green());
    }
    for name in &report.removed {
        println!("  {} {name}", style("-").red());
    }
    if report.installed.is_empty() && report.removed.is_empty() {
        println!("  {} packages up to date ({})", style("✓").green(), report.unchanged);
    }
}

/// `luat add @scope/name[@<req>]` or `luat add @scope/name --path <dir>`.
pub async fn add(spec: &str, path: Option<&str>) -> anyhow::Result<()> {
    let project = current_project()?;
    let report = match path {
        Some(path) => project.add_path(&PackageName::parse(spec)?, path).await?,
        None => project.add(spec).await?,
    };
    print_report(&report);
    Ok(())
}

/// `luat remove @scope/name`.
pub async fn remove(name: &str) -> anyhow::Result<()> {
    let report = current_project()?.remove(&PackageName::parse(name)?).await?;
    print_report(&report);
    Ok(())
}

/// `luat install [--frozen]`.
pub async fn install(frozen: bool) -> anyhow::Result<()> {
    let report = current_project()?.install(frozen).await?;
    print_report(&report);
    Ok(())
}

/// `luat update [name]`.
pub async fn update(name: Option<&str>) -> anyhow::Result<()> {
    let name = name.map(PackageName::parse).transpose()?;
    let report = current_project()?.update(name.as_ref()).await?;
    print_report(&report);
    Ok(())
}

/// Runs `install` when the lockfile or `.luat/packages` is missing or
/// stale (used by `luat build` and `luat dev`). Returns the packages
/// directory to build with.
pub async fn ensure_installed(root: &Path) -> anyhow::Result<PathBuf> {
    let project = Project::new(root).with_settings(Settings::default());
    if project.needs_install()? {
        println!("{}", style("Installing packages...").cyan());
        let report = project.install(false).await?;
        print_report(&report);
    }
    Ok(project.packages_dir())
}

/// Directories of the project's path dependencies, from `luat.lock`.
pub fn path_dependency_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(Some(lock)) = Lockfile::load(root) else { return Vec::new() };
    lock.packages
        .iter()
        .filter_map(|p| p.path_source())
        .filter_map(|rel| std::fs::canonicalize(root.join(rel)).ok())
        .collect()
}
