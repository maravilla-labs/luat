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
mod safe_fs;
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

/// Environment variable holding a token for CI. It applies to the default
/// registry only, unless [`TOKEN_FOR_ENV`] names another registry URL.
pub const TOKEN_ENV: &str = "LUAT_REGISTRY_TOKEN";

/// Environment variable binding [`TOKEN_ENV`] to one registry URL instead
/// of the default registry.
pub const TOKEN_FOR_ENV: &str = "LUAT_REGISTRY_TOKEN_FOR";

/// Where installed packages live, relative to the project root.
pub const PACKAGES_DIR: &str = ".luat/packages";

/// Settings shared by every operation that talks to a registry.
///
/// Tokens are bound to the exact normalized registry URL they belong to and
/// are only sent by authenticated operations (publish, yank, login, me).
/// Reads (index, download, package info, search) are always anonymous, so
/// installing an untrusted project never sends credentials anywhere.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Credentials file. `None` uses [`default_credentials_path`].
    pub credentials_path: Option<std::path::PathBuf>,
    /// Tokens supplied by the host, keyed by registry URL (normalized on
    /// lookup). They take precedence over the environment and the file.
    pub tokens: std::collections::BTreeMap<String, String>,
    /// The luat version packages' `luat` requirements are checked against.
    /// `None` uses this crate's version.
    pub luat_version: Option<semver::Version>,
}

impl Settings {
    /// The token for `registry`, if one is bound to exactly that URL: a
    /// host-supplied token, else `LUAT_REGISTRY_TOKEN` (for the default
    /// registry, or the URL in `LUAT_REGISTRY_TOKEN_FOR`), else the
    /// credentials file.
    pub fn token_for(&self, registry: &str) -> Option<String> {
        let registry = normalize_registry_url(registry).ok()?;
        let bound = |url: &str| normalize_registry_url(url).ok().as_deref() == Some(registry.as_str());
        if let Some((_, token)) = self.tokens.iter().find(|(url, _)| bound(url)) {
            return Some(token.clone());
        }
        let env_for = std::env::var(TOKEN_FOR_ENV).ok().filter(|v| !v.is_empty());
        if env_token_applies(&registry, env_for.as_deref()) {
            if let Some(token) = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty()) {
                return Some(token);
            }
        }
        let path = self.credentials_path.clone().or_else(default_credentials_path)?;
        Credentials::load(&path).ok()?.token(&registry).map(str::to_string)
    }

    /// An anonymous client for `registry` (for reads).
    pub fn client(&self, registry: &str) -> Result<RegistryClient> {
        RegistryClient::new(registry)
    }

    /// A client for `registry` carrying the token bound to it, for publish,
    /// yank and me.
    pub fn auth_client(&self, registry: &str) -> Result<RegistryClient> {
        Ok(RegistryClient::new(registry)?.with_token(self.token_for(registry)))
    }

    pub(crate) fn luat_version(&self) -> semver::Version {
        self.luat_version.clone().unwrap_or_else(|| {
            semver::Version::parse(crate::bundle::LUAT_VERSION).expect("crate version is semver")
        })
    }
}

/// Whether `LUAT_REGISTRY_TOKEN` may be sent to `registry` (normalized):
/// only to the URL named by `LUAT_REGISTRY_TOKEN_FOR` when set, else only
/// to the default registry.
pub(crate) fn env_token_applies(registry: &str, env_for: Option<&str>) -> bool {
    let target = env_for.unwrap_or(DEFAULT_REGISTRY);
    normalize_registry_url(target).ok().as_deref() == Some(registry)
}

/// Validates and normalizes a registry URL: `https://`, or `http://` for
/// localhost / loopback only; no credentials, query or fragment; lowercase
/// host, default port dropped, no trailing slash. Tokens are bound to this
/// form.
pub fn normalize_registry_url(url: &str) -> Result<String> {
    let bad = |why: &str| PackageError::Manifest(format!("registry URL '{url}' {why}"));
    let parsed = reqwest::Url::parse(url.trim()).map_err(|e| bad(&format!("is invalid: {e}")))?;
    let host = parsed.host_str().ok_or_else(|| bad("has no host"))?.to_ascii_lowercase();
    match parsed.scheme() {
        "https" => {}
        "http" if matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]") => {}
        "http" => return Err(bad("must use https:// (plain http is only allowed for localhost)")),
        _ => return Err(bad("must be an https:// URL")),
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(bad("must not contain credentials"));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(bad("must not have a query or fragment"));
    }
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = parsed.path().trim_end_matches('/');
    Ok(format!("{}://{host}{port}{path}", parsed.scheme()))
}

/// [`normalize_registry_url`], falling back to the trimmed input for
/// invalid URLs (which are then rejected when a client is created).
pub(crate) fn normalize_url(url: &str) -> String {
    normalize_registry_url(url).unwrap_or_else(|_| url.trim().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_urls_are_normalized_and_checked() {
        let ok = |u: &str| normalize_registry_url(u).unwrap();
        assert_eq!(ok("https://Reg.Example.com:443/luat/"), "https://reg.example.com/luat");
        assert_eq!(ok("https://reg.example.com:8443"), "https://reg.example.com:8443");
        assert_eq!(ok("http://127.0.0.1:4000/"), "http://127.0.0.1:4000");
        assert_eq!(ok("http://localhost"), "http://localhost");
        for bad in ["http://reg.example.com", "ftp://x", "https://user:pw@x.com", "https://x.com/?a=1", "nope"] {
            assert!(normalize_registry_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tokens_are_bound_to_one_registry() {
        let settings = Settings {
            credentials_path: Some(std::path::PathBuf::from("/nonexistent/credentials.toml")),
            tokens: [("https://a.example.com/".to_string(), "t".to_string())].into(),
            luat_version: None,
        };
        assert_eq!(settings.token_for("https://A.example.com"), Some("t".into()));
        assert_eq!(settings.token_for("https://a.example.com/other"), None);
        assert_eq!(settings.token_for("https://evil.example.com"), None);
    }

    #[test]
    fn the_environment_token_only_goes_to_its_registry() {
        let default = normalize_registry_url(DEFAULT_REGISTRY).unwrap();
        assert!(env_token_applies(&default, None));
        assert!(!env_token_applies("https://evil.example.com", None));
        assert!(env_token_applies("https://ci.example.com", Some("https://ci.example.com/")));
        assert!(!env_token_applies(&default, Some("https://ci.example.com")));
    }
}
