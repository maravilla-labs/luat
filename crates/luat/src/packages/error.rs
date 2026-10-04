// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Errors of the package tooling.

use thiserror::Error;

/// Anything that can go wrong while packing, resolving, installing or
/// talking to a registry.
#[derive(Debug, Error)]
pub enum PackageError {
    /// `luat.toml` (or a lockfile) is malformed or breaks a package rule.
    #[error("{0}")]
    Manifest(String),
    /// A package name breaks the `@scope/name` rules.
    #[error("invalid package name '{0}': expected @scope/name with parts matching [a-z0-9][a-z0-9._-]{{0,63}}")]
    InvalidName(String),
    /// A tarball breaks the format rules (paths, links, limits).
    #[error("invalid tarball: {0}")]
    Tarball(String),
    /// Downloaded bytes do not match the expected checksum.
    #[error("checksum mismatch for {name}@{version}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Package name.
        name: String,
        /// Package version.
        version: String,
        /// Checksum from the index or lockfile.
        expected: String,
        /// Checksum of the downloaded bytes.
        actual: String,
    },
    /// No set of versions satisfies the requirements.
    #[error("{0}")]
    Resolution(String),
    /// `install --frozen` with a missing or out-of-date lockfile.
    #[error("{0}")]
    Frozen(String),
    /// A package or version the registry does not have.
    #[error("{0}")]
    NotFound(String),
    /// The registry answered with an error status.
    #[error("registry {url} returned {status}: {message}")]
    Registry {
        /// Request URL.
        url: String,
        /// HTTP status code.
        status: u16,
        /// The registry's `error` message, or the response body.
        message: String,
    },
    /// No token for an operation that needs one.
    #[error("not logged in to {0}; run `luat login {0}` or set LUAT_REGISTRY_TOKEN")]
    NoToken(String),
    /// Transport failure talking to a registry.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// Filesystem failure.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Result alias for the package tooling.
pub type Result<T> = std::result::Result<T, PackageError>;

/// Attaches context to I/O errors.
pub(crate) trait IoContext<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|source| PackageError::Io { context: context(), source })
    }
}
