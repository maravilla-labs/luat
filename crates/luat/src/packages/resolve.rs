// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Dependency resolution: one version per package name, the highest
//! non-yanked version matching every requirement, locked versions kept
//! while they still satisfy (even when yanked).
//!
//! The core is synchronous and works on indexes it is given; when it needs
//! one it does not have it says so ([`Step::Need`]) and the caller fetches
//! it and runs it again.

use std::collections::{BTreeMap, HashMap};

use semver::{Version, VersionReq};

use super::error::{PackageError, Result};
use super::name::PackageName;
use super::path_deps::PathPackage;
use super::registry::IndexEntry;

/// A version requirement and who stated it.
#[derive(Debug, Clone)]
struct Requirement {
    req: VersionReq,
    by: String,
}

/// What to resolve.
pub(crate) struct Input<'a> {
    /// Registry dependencies of the project.
    pub roots: Vec<(PackageName, VersionReq)>,
    /// Path dependencies (fixed versions), with their manifests.
    pub paths: &'a BTreeMap<PackageName, PathPackage>,
    /// Versions to keep when they still satisfy every requirement.
    pub locked: BTreeMap<PackageName, Version>,
    /// The luat version packages must accept.
    pub luat: Version,
}

/// The outcome of one resolution attempt.
pub(crate) enum Step {
    /// The registry packages of the graph (path packages are fixed).
    Done(BTreeMap<PackageName, IndexEntry>),
    /// The index of this package is needed first.
    Need(PackageName),
}

const MAX_ROUNDS: usize = 100;

/// Resolves `input` against `indexes`. A package missing from `indexes` is
/// requested with [`Step::Need`]; one present with no versions is an error.
pub(crate) fn resolve(input: &Input, indexes: &HashMap<PackageName, Vec<IndexEntry>>) -> Result<Step> {
    let mut selected: BTreeMap<PackageName, IndexEntry> = BTreeMap::new();
    for _ in 0..MAX_ROUNDS {
        let requirements = requirements(input, &selected);
        let mut next = BTreeMap::new();
        for (name, reqs) in &requirements {
            if let Some(path) = input.paths.get(name) {
                check_path(path, reqs)?;
                continue;
            }
            let Some(index) = indexes.get(name) else {
                return Ok(Step::Need(name.clone()));
            };
            let entry = choose(name, reqs, index, input, selected.get(name))?;
            next.insert(name.clone(), entry.clone());
        }
        if next == selected {
            return Ok(Step::Done(selected));
        }
        selected = next;
    }
    Err(PackageError::Resolution("dependency resolution did not settle".to_string()))
}

fn requirements(input: &Input, selected: &BTreeMap<PackageName, IndexEntry>) -> BTreeMap<PackageName, Vec<Requirement>> {
    let mut out: BTreeMap<PackageName, Vec<Requirement>> = BTreeMap::new();
    let mut add = |name: &PackageName, req: &VersionReq, by: String| {
        out.entry(name.clone()).or_default().push(Requirement { req: req.clone(), by });
    };
    for (name, req) in &input.roots {
        add(name, req, "luat.toml".to_string());
    }
    for path in input.paths.values() {
        for (name, req) in path.manifest.registry_dependencies() {
            add(name, req, format!("{}@{}", path.name, path.version));
        }
    }
    for entry in selected.values() {
        for (name, req) in &entry.deps {
            add(name, req, format!("{}@{}", entry.name, entry.vers));
        }
    }
    out
}

fn check_path(path: &PathPackage, reqs: &[Requirement]) -> Result<()> {
    match reqs.iter().find(|r| !r.req.matches(&path.version)) {
        Some(r) => Err(PackageError::Resolution(format!(
            "{} is used from {} at version {}, but {} requires {}",
            path.name,
            path.dir.display(),
            path.version,
            r.by,
            r.req
        ))),
        None => Ok(()),
    }
}

fn choose<'i>(
    name: &PackageName,
    reqs: &[Requirement],
    index: &'i [IndexEntry],
    input: &Input,
    previous: Option<&IndexEntry>,
) -> Result<&'i IndexEntry> {
    let matching = |e: &&IndexEntry| reqs.iter().all(|r| r.req.matches(&e.vers));
    if let Some(locked) = input.locked.get(name) {
        if let Some(entry) = index.iter().filter(matching).find(|e| &e.vers == locked) {
            return Ok(entry);
        }
    }
    if let Some(previous) = previous {
        if let Some(entry) = index.iter().filter(matching).find(|e| e.vers == previous.vers && !e.yanked) {
            return Ok(entry);
        }
    }
    let luat_ok = |e: &&IndexEntry| e.luat.as_ref().map_or(true, |r| r.matches(&input.luat));
    index
        .iter()
        .filter(matching)
        .filter(|e| !e.yanked)
        .filter(luat_ok)
        .max_by(|a, b| a.vers.cmp(&b.vers))
        .ok_or_else(|| no_match(name, reqs, index, &input.luat))
}

/// Explains why nothing matched: an unsatisfiable requirement, a pair of
/// conflicting ones, or the luat version.
fn no_match(name: &PackageName, reqs: &[Requirement], index: &[IndexEntry], luat: &Version) -> PackageError {
    let usable: Vec<&IndexEntry> = index.iter().filter(|e| !e.yanked).collect();
    if usable.is_empty() {
        return PackageError::Resolution(format!("{name} has no installable (non-yanked) versions"));
    }
    let available = || usable.iter().map(|e| e.vers.to_string()).collect::<Vec<_>>().join(", ");
    for r in reqs {
        if !usable.iter().any(|e| r.req.matches(&e.vers)) {
            return PackageError::Resolution(format!(
                "no version of {name} matches {} (required by {}); available: {}",
                r.req,
                r.by,
                available()
            ));
        }
    }
    for (i, a) in reqs.iter().enumerate() {
        for b in &reqs[i + 1..] {
            if !usable.iter().any(|e| a.req.matches(&e.vers) && b.req.matches(&e.vers)) {
                return PackageError::Resolution(format!(
                    "version conflict for {name}: {} (required by {}) and {} (required by {}) have no version in common",
                    a.req, a.by, b.req, b.by
                ));
            }
        }
    }
    let all = reqs.iter().map(|r| format!("{} (required by {})", r.req, r.by)).collect::<Vec<_>>();
    let only_luat = usable.iter().any(|e| reqs.iter().all(|r| r.req.matches(&e.vers)));
    if only_luat {
        return PackageError::Resolution(format!(
            "every version of {name} matching {} requires a different luat version than {luat}",
            all.join(", ")
        ));
    }
    PackageError::Resolution(format!("no version of {name} satisfies all of: {}", all.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> PackageName {
        PackageName::parse(s).unwrap()
    }

    fn e(name: &str, vers: &str, deps: &[(&str, &str)], yanked: bool) -> IndexEntry {
        IndexEntry {
            name: n(name),
            vers: Version::parse(vers).unwrap(),
            deps: deps.iter().map(|(d, r)| (n(d), VersionReq::parse(r).unwrap())).collect(),
            cksum: "sha256:00".into(),
            luat: None,
            yanked,
        }
    }

    fn indexes() -> HashMap<PackageName, Vec<IndexEntry>> {
        HashMap::from([
            (n("@a/ui"), vec![e("@a/ui", "1.0.0", &[("@a/icons", "^1")], false), e("@a/ui", "1.1.0", &[("@a/icons", "^2")], false)]),
            (
                n("@a/icons"),
                vec![e("@a/icons", "1.0.0", &[], false), e("@a/icons", "2.0.0", &[], false), e("@a/icons", "2.1.0", &[], true)],
            ),
        ])
    }

    fn run(roots: &[(&str, &str)], locked: &[(&str, &str)]) -> Result<BTreeMap<String, String>> {
        let paths = BTreeMap::new();
        let input = Input {
            roots: roots.iter().map(|(a, b)| (n(a), VersionReq::parse(b).unwrap())).collect(),
            paths: &paths,
            locked: locked.iter().map(|(a, b)| (n(a), Version::parse(b).unwrap())).collect(),
            luat: Version::new(0, 1, 0),
        };
        match resolve(&input, &indexes())? {
            Step::Done(map) => Ok(map.into_iter().map(|(k, v)| (k.to_string(), v.vers.to_string())).collect()),
            Step::Need(name) => panic!("needs {name}"),
        }
    }

    #[test]
    fn picks_highest_non_yanked_transitively() {
        let got = run(&[("@a/ui", "^1")], &[]).unwrap();
        assert_eq!(got["@a/ui"], "1.1.0");
        assert_eq!(got["@a/icons"], "2.0.0");
    }

    #[test]
    fn keeps_locked_versions_even_yanked() {
        let got = run(&[("@a/ui", "^1")], &[("@a/ui", "1.1.0"), ("@a/icons", "2.1.0")]).unwrap();
        assert_eq!(got["@a/icons"], "2.1.0");
        let got = run(&[("@a/ui", "^1")], &[("@a/ui", "1.0.0")]).unwrap();
        assert_eq!((got["@a/ui"].as_str(), got["@a/icons"].as_str()), ("1.0.0", "1.0.0"));
    }

    #[test]
    fn conflicts_name_both_requirements() {
        let err = run(&[("@a/ui", "=1.1.0"), ("@a/icons", "^1")], &[]).unwrap_err().to_string();
        assert!(err.contains("^1 (required by luat.toml)") && err.contains("^2 (required by @a/ui@1.1.0)"), "{err}");
    }

    #[test]
    fn unsatisfiable_requirements_list_versions() {
        let err = run(&[("@a/icons", "^9")], &[]).unwrap_err().to_string();
        assert!(err.contains("no version of @a/icons matches ^9") && err.contains("1.0.0, 2.0.0"), "{err}");
    }
}
