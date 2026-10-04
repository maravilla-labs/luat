// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! A project's dependencies: `add`, `remove`, `install`, `update`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::error::{IoContext, PackageError, Result};
use super::install::{self, InstallReport};
use super::lockfile::{LockedPackage, Lockfile, LOCKFILE_NAME};
use super::manifest::{self, Dependency, PackageManifest, MANIFEST_NAME};
use super::name::{parse_spec, PackageName};
use super::path_deps::{self, PathPackage};
use super::registry::IndexEntry;
use super::resolve::{self, Input, Step};
use super::{Settings, PACKAGES_DIR};

/// A project directory (holding `luat.toml`) and the settings used to reach
/// registries.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    settings: Settings,
}

impl Project {
    /// The project rooted at `root`, with default [`Settings`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            settings: Settings::default(),
        }
    }

    /// Uses `settings` for registry access.
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }

    /// The project directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where packages are installed: `<root>/.luat/packages`. Pass this as
    /// `BuildOptions::packages_dir` / `FileSystemResolver::with_packages_dir`.
    pub fn packages_dir(&self) -> PathBuf {
        self.root.join(PACKAGES_DIR)
    }

    /// The project's `luat.toml`.
    pub fn manifest(&self) -> Result<PackageManifest> {
        PackageManifest::load(&self.root)
    }

    /// True when `install` has work to do: the lockfile is missing or does
    /// not match `luat.toml`, a locked package is not installed, or a path
    /// dependency exists (those are re-copied on every install). Never
    /// touches the network.
    pub fn needs_install(&self) -> Result<bool> {
        if !self.root.join(MANIFEST_NAME).exists() {
            return Ok(false);
        }
        let manifest = self.manifest()?;
        let lock = Lockfile::load(&self.root)?;
        if manifest.dependencies.is_empty() && lock.is_none() {
            // Nothing to install; only leftovers to remove.
            return Ok(!install::installed_names(&self.packages_dir()).is_empty());
        }
        let paths = path_deps::collect(&self.root, &manifest)?;
        Ok(match lock {
            Some(lock) => {
                !paths.is_empty() || !is_fresh(&manifest, &paths, &lock) || !install::is_installed(&self.packages_dir(), &lock)
            }
            None => true,
        })
    }

    /// Installs what `luat.lock` says, resolving first when it is missing
    /// or out of date (an error when `frozen`).
    pub async fn install(&self, frozen: bool) -> Result<InstallReport> {
        let manifest = self.manifest()?;
        let paths = path_deps::collect(&self.root, &manifest)?;
        let existing = Lockfile::load(&self.root)?;
        let lock = match &existing {
            Some(lock) if is_fresh(&manifest, &paths, lock) => lock.clone(),
            _ if frozen => {
                return Err(PackageError::Frozen(format!(
                    "{LOCKFILE_NAME} is missing or out of date with {MANIFEST_NAME}; run `luat install` without --frozen"
                )))
            }
            _ => self.resolve(&manifest, &paths, existing.as_ref(), None).await?,
        };
        let report = install::install(&self.root, &self.packages_dir(), &lock, &self.settings).await?;
        if !frozen {
            lock.save(&self.root)?;
        }
        Ok(report)
    }

    /// Re-resolves ignoring the lockfile for `name` (or for everything),
    /// then installs.
    pub async fn update(&self, name: Option<&PackageName>) -> Result<InstallReport> {
        let manifest = self.manifest()?;
        let paths = path_deps::collect(&self.root, &manifest)?;
        let existing = Lockfile::load(&self.root)?;
        let prefs = if name.is_some() { existing.as_ref() } else { None };
        let lock = self.resolve(&manifest, &paths, prefs, name).await?;
        let report = install::install(&self.root, &self.packages_dir(), &lock, &self.settings).await?;
        lock.save(&self.root)?;
        Ok(report)
    }

    /// Adds `@scope/name[@<req>]` to `[dependencies]` (with `^<latest>` when
    /// no requirement is given), resolves, installs and writes `luat.toml`
    /// and `luat.lock`. Nothing is written when resolution fails.
    pub async fn add(&self, spec: &str) -> Result<InstallReport> {
        let (name, req) = parse_spec(spec)?;
        let manifest = self.manifest()?;
        let req = match req {
            Some(req) => req,
            None => {
                let index = self.settings.client(&manifest.registries.url_for(&name)).index(&name).await?;
                let luat = self.settings.luat_version();
                let latest = index
                    .iter()
                    .filter(|e| !e.yanked && e.luat.as_ref().map_or(true, |r| r.matches(&luat)))
                    .max_by(|a, b| a.vers.cmp(&b.vers))
                    .ok_or_else(|| PackageError::Resolution(format!("{name} has no installable versions")))?;
                semver::VersionReq::parse(&format!("^{}", latest.vers)).expect("caret requirement")
            }
        };
        self.change_dependency(&name, Some(toml_edit::Value::from(req.to_string()))).await
    }

    /// Adds `name` as a path dependency on `path` (relative to the project).
    pub async fn add_path(&self, name: &PackageName, path: &str) -> Result<InstallReport> {
        let mut table = toml_edit::InlineTable::new();
        table.insert("path", path.into());
        self.change_dependency(name, Some(toml_edit::Value::InlineTable(table))).await
    }

    /// Removes `name` from `[dependencies]`, resolves, installs (removing
    /// packages that left the graph) and writes both files.
    pub async fn remove(&self, name: &PackageName) -> Result<InstallReport> {
        self.change_dependency(name, None).await
    }

    async fn change_dependency(&self, name: &PackageName, value: Option<toml_edit::Value>) -> Result<InstallReport> {
        let path = self.root.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&path).ctx(|| format!("reading {}", path.display()))?;
        let new_text = match value {
            Some(v) => manifest::set_dependency(&text, name, v)?,
            None => match manifest::remove_dependency(&text, name)? {
                (t, true) => t,
                (_, false) => return Err(PackageError::Manifest(format!("{name} is not a dependency"))),
            },
        };
        let manifest = PackageManifest::parse(&new_text)?;
        let paths = path_deps::collect(&self.root, &manifest)?;
        let existing = Lockfile::load(&self.root)?;
        let lock = self.resolve(&manifest, &paths, existing.as_ref(), None).await?;
        let report = install::install(&self.root, &self.packages_dir(), &lock, &self.settings).await?;
        std::fs::write(&path, new_text).ctx(|| format!("writing {}", path.display()))?;
        lock.save(&self.root)?;
        Ok(report)
    }

    /// Resolves the graph, keeping versions of `prefs` (except `unlock`).
    async fn resolve(
        &self,
        manifest: &PackageManifest,
        paths: &BTreeMap<PackageName, PathPackage>,
        prefs: Option<&Lockfile>,
        unlock: Option<&PackageName>,
    ) -> Result<Lockfile> {
        let registries = &manifest.registries;
        let locked = prefs
            .map(|lock| {
                lock.packages
                    .iter()
                    .filter(|p| Some(&p.name) != unlock && p.registry.as_deref() == Some(&registries.url_for(&p.name)))
                    .map(|p| (p.name.clone(), p.version.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let input = Input {
            roots: manifest.registry_dependencies().map(|(n, r)| (n.clone(), r.clone())).collect(),
            paths,
            locked,
            luat: self.settings.luat_version(),
        };
        let mut indexes: HashMap<PackageName, Vec<IndexEntry>> = HashMap::new();
        let selected = loop {
            match resolve::resolve(&input, &indexes)? {
                Step::Done(selected) => break selected,
                Step::Need(name) => {
                    let index = self.settings.client(&registries.url_for(&name)).index(&name).await?;
                    indexes.insert(name, index);
                }
            }
        };

        let mut packages: Vec<LockedPackage> = selected
            .into_values()
            .map(|e| LockedPackage {
                registry: Some(registries.url_for(&e.name)),
                checksum: Some(e.cksum),
                source: None,
                dependencies: e.deps.into_keys().collect(),
                name: e.name,
                version: e.vers,
            })
            .collect();
        packages.extend(paths.values().map(|p| LockedPackage {
            name: p.name.clone(),
            version: p.version.clone(),
            registry: None,
            checksum: None,
            source: Some(p.source.clone()),
            dependencies: p.manifest.dependencies.keys().cloned().collect(),
        }));
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Lockfile { packages, ..Lockfile::default() })
    }
}

/// True when `lock` still describes `manifest`: every requirement is met by
/// a locked package from the right source, every locked dependency is
/// locked, and nothing unreachable is locked. Offline.
fn is_fresh(manifest: &PackageManifest, paths: &BTreeMap<PackageName, PathPackage>, lock: &Lockfile) -> bool {
    let by_name: HashMap<&PackageName, &LockedPackage> = lock.packages.iter().map(|p| (&p.name, p)).collect();
    let registry_ok = |name: &PackageName, req: &semver::VersionReq| {
        by_name.get(name).is_some_and(|p| {
            if let Some(path) = paths.get(name) {
                return p.source.as_deref() == Some(&path.source) && req.matches(&path.version);
            }
            p.registry.as_deref() == Some(&manifest.registries.url_for(name)) && req.matches(&p.version)
        })
    };
    let path_ok = |path: &PathPackage| {
        by_name
            .get(&path.name)
            .is_some_and(|p| p.source.as_deref() == Some(&path.source) && p.version == path.version)
    };
    let roots_ok = manifest.dependencies.iter().all(|(name, dep)| match dep {
        Dependency::Registry(req) => registry_ok(name, req),
        Dependency::Path { .. } => paths.get(name).is_some_and(path_ok),
    });
    let paths_ok = paths.values().all(|p| path_ok(p) && p.manifest.registry_dependencies().all(|(n, r)| registry_ok(n, r)));
    if !(roots_ok && paths_ok) {
        return false;
    }
    // Reachability from the roots, and closure of locked dependencies.
    let mut seen = std::collections::BTreeSet::new();
    let mut stack: Vec<&PackageName> = manifest.dependencies.keys().collect();
    while let Some(name) = stack.pop() {
        let Some(p) = by_name.get(name) else { return false };
        if seen.insert(name) {
            stack.extend(p.dependencies.iter());
        }
    }
    seen.len() == lock.packages.len()
}
