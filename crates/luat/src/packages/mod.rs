// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! The client side of Luat packages (see `docs/packages.md`).
//!
//! - [`PackageManifest`]: the `[package]`, `[dependencies]` and
//!   `[registries]` sections of `luat.toml`.
//! - [`pack`]: a project directory to a publishable [`Tarball`].
//! - [`RegistryClient`]: the registry HTTP API.
//! - [`Project`]: `add`, `remove`, `install`, `update` for a project, with
//!   resolution, `luat.lock` and installation into `.luat/packages`.
//! - [`search`], [`publish`], [`login`], [`yank`]: registry operations.
//!
//! Installed packages are found at build time through
//! [`BuildOptions::packages_dir`](crate::bundle::BuildOptions) and in
//! development through
//! [`FileSystemResolver::with_packages_dir`](crate::FileSystemResolver::with_packages_dir);
//! both need only the `filesystem` feature, not this one.

mod credentials;
mod error;
mod files;
mod install;
mod lockfile;
mod manifest;
mod name;
mod ops;
mod path_deps;
mod project;
mod registry;
mod resolve;
mod tarball;

pub use credentials::{default_credentials_path, Credentials};
pub use error::{PackageError, Result};
pub use files::{package_files, PackageFiles, DEFAULT_INCLUDE};
pub use install::InstallReport;
pub use lockfile::{LockedPackage, Lockfile, LOCKFILE_NAME, LOCKFILE_VERSION};
pub use manifest::{Dependency, PackageManifest, PackageMeta, Registries, MANIFEST_NAME};
pub use name::{parse_spec, PackageName};
pub use ops::{login, publish, search, yank, PublishOutcome};
pub use project::Project;
pub use registry::{
    IndexEntry, Me, PackageInfo, PublishResult, RegistryClient, SearchHit, SearchResults, VersionInfo,
};
pub use tarball::{
    checksum, extract, pack, read_tarball, Tarball, TarballFile, MAX_COMPRESSED_BYTES, MAX_FILES,
    MAX_UNCOMPRESSED_BYTES,
};

/// The registry used for scopes without an entry in `[registries]`, when the
/// project sets no `default`.
pub const DEFAULT_REGISTRY: &str = "https://luat.registry.maravilla.cloud";

/// Environment variable whose token overrides stored credentials (for CI).
pub const TOKEN_ENV: &str = "LUAT_REGISTRY_TOKEN";

/// Where installed packages live, relative to the project root.
pub const PACKAGES_DIR: &str = ".luat/packages";

/// Settings shared by every operation that talks to a registry.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Credentials file. `None` uses [`default_credentials_path`].
    pub credentials_path: Option<std::path::PathBuf>,
    /// Token to use for every registry, overriding stored credentials.
    /// `None` reads [`TOKEN_ENV`].
    pub token: Option<String>,
    /// The luat version packages' `luat` requirements are checked against.
    /// `None` uses this crate's version.
    pub luat_version: Option<semver::Version>,
}

impl Settings {
    /// The token for `registry`: [`Settings::token`], else
    /// `LUAT_REGISTRY_TOKEN`, else the credentials file.
    pub fn token_for(&self, registry: &str) -> Option<String> {
        if let Some(token) = &self.token {
            return Some(token.clone());
        }
        if let Ok(token) = std::env::var(TOKEN_ENV) {
            if !token.is_empty() {
                return Some(token);
            }
        }
        let path = self.credentials_path.clone().or_else(default_credentials_path)?;
        Credentials::load(&path).ok()?.token(registry).map(str::to_string)
    }

    /// A client for `registry`, carrying its token when one is known.
    pub fn client(&self, registry: &str) -> RegistryClient {
        RegistryClient::new(registry).with_token(self.token_for(registry))
    }

    pub(crate) fn luat_version(&self) -> semver::Version {
        self.luat_version.clone().unwrap_or_else(|| {
            semver::Version::parse(crate::bundle::LUAT_VERSION).expect("crate version is semver")
        })
    }
}

/// `url` without trailing slashes, the form registries are keyed by.
pub(crate) fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}
