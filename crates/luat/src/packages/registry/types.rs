// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! JSON shapes of the registry API.

use std::collections::BTreeMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::packages::name::PackageName;

/// One line of `GET /index/@scope/name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Package name.
    pub name: PackageName,
    /// Version.
    pub vers: Version,
    /// Dependencies of this version.
    #[serde(default)]
    pub deps: BTreeMap<PackageName, VersionReq>,
    /// `sha256:<hex>` of the tarball.
    pub cksum: String,
    /// Luat versions this version works with.
    #[serde(default)]
    pub luat: Option<VersionReq>,
    /// Skipped by new resolutions when true.
    #[serde(default)]
    pub yanked: bool,
}

/// One version in [`PackageInfo::versions`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Version.
    pub vers: Version,
    /// RFC 3339 publish time.
    #[serde(default)]
    pub published_at: Option<String>,
    /// Whether the version is yanked.
    #[serde(default)]
    pub yanked: bool,
    /// Tarball size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
}

/// `GET /api/v1/packages/@scope/name[/<version>]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Package name.
    pub name: PackageName,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Latest version.
    #[serde(default)]
    pub latest: Option<Version>,
    /// License.
    #[serde(default)]
    pub license: Option<String>,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Keywords.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// README of the described version.
    #[serde(default)]
    pub readme: Option<String>,
    /// All versions.
    #[serde(default)]
    pub versions: Vec<VersionInfo>,
    /// Dependencies of the described version.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    /// Files of the described version.
    #[serde(default)]
    pub files: Vec<String>,
}

/// One hit of [`SearchResults`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Package name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Latest version.
    #[serde(default)]
    pub latest: Option<String>,
    /// Last update time.
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// `GET /api/v1/packages?q=...`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    /// The page of hits.
    pub packages: Vec<SearchHit>,
    /// Total number of matches.
    #[serde(default)]
    pub total: u64,
}

/// `201` body of a publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishResult {
    /// Package name.
    pub name: String,
    /// Published version.
    pub vers: String,
    /// Checksum the registry computed.
    pub cksum: String,
}

/// `GET /api/v1/me`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Me {
    /// User name.
    pub user: String,
    /// Scopes the token may publish to.
    #[serde(default)]
    pub scopes: Vec<String>,
}
