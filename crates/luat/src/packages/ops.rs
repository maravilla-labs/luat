// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Registry operations that are not about a project's dependencies.

use std::path::Path;

use semver::Version;

use super::credentials::{default_credentials_path, Credentials};
use super::error::{PackageError, Result};
use super::manifest::PackageManifest;
use super::name::PackageName;
use super::registry::{Me, PublishResult, RegistryClient, SearchResults};
use super::tarball::{pack, Tarball};
use super::Settings;

/// Searches `registry` (page 1-based, `per_page` at most 100).
pub async fn search(registry: &str, query: &str, page: u32, per_page: u32, settings: &Settings) -> Result<SearchResults> {
    let query = Some(query).filter(|q| !q.is_empty());
    settings.client(registry).search(query, page, per_page).await
}

/// What [`publish`] did.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    /// The packed tarball.
    pub tarball: Tarball,
    /// The registry it was (or would be) published to.
    pub registry: String,
    /// The registry's answer; `None` for a dry run.
    pub result: Option<PublishResult>,
}

/// Packs the package at `dir` and publishes it to the registry its
/// `[registries]` select for its scope. Packages with path dependencies (or
/// any dependency value that is not a plain requirement string) are
/// refused: published packages may only depend on registry versions.
pub async fn publish(dir: &Path, dry_run: bool, settings: &Settings) -> Result<PublishOutcome> {
    let manifest = PackageManifest::load(dir)?;
    let meta = manifest.package()?;
    let registry = manifest.registries.url_for(&meta.name);
    let tarball = pack(dir)?;
    if dry_run {
        return Ok(PublishOutcome { tarball, registry, result: None });
    }
    let client = settings.client(&registry);
    let result = client.publish(&meta.name, &meta.version, tarball.bytes.clone()).await?;
    Ok(PublishOutcome {
        tarball,
        registry,
        result: Some(result),
    })
}

/// Checks `token` against `registry` (`GET /api/v1/me`) and stores it in the
/// credentials file ([`Settings::credentials_path`] or the default).
pub async fn login(registry: &str, token: &str, settings: &Settings) -> Result<Me> {
    let me = RegistryClient::new(registry)
        .with_token(Some(token.to_string()))
        .me()
        .await
        .map_err(|e| match e {
            PackageError::Registry { status: 401, message, .. } => {
                PackageError::Manifest(format!("{registry} rejected the token ({message})"))
            }
            PackageError::Registry { status: 403, message, .. } => PackageError::Manifest(format!(
                "{registry} accepted the token but it is not bound to a user, so it cannot publish ({message})"
            )),
            other => other,
        })?;
    let path = settings
        .credentials_path
        .clone()
        .or_else(default_credentials_path)
        .ok_or_else(|| PackageError::Manifest("cannot find a config directory for credentials".to_string()))?;
    let mut credentials = Credentials::load(&path)?;
    credentials.set_token(registry, token);
    credentials.save()?;
    Ok(me)
}

/// Yanks (or with `undo`, unyanks) `name@version` on `registry`.
pub async fn yank(registry: &str, name: &PackageName, version: &Version, undo: bool, settings: &Settings) -> Result<()> {
    settings.client(registry).set_yanked(name, version, !undo).await
}
