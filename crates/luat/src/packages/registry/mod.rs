// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP client for the registry API in `docs/packages.md`.

mod types;

use reqwest::{Method, RequestBuilder, Response, StatusCode};
use semver::Version;
use serde::de::DeserializeOwned;

pub use types::{IndexEntry, Me, PackageInfo, PublishResult, SearchHit, SearchResults, VersionInfo};

use super::error::{PackageError, Result};
use super::name::PackageName;
use super::normalize_registry_url;
use super::tarball::{checksum, MAX_COMPRESSED_BYTES};

/// A client for one registry.
#[derive(Debug, Clone)]
pub struct RegistryClient {
    base: String,
    token: Option<String>,
    /// Anonymous reads; may follow redirects (e.g. to a download host).
    http: reqwest::Client,
    /// Authenticated requests; never follows redirects, so the token only
    /// ever goes to `base`'s origin.
    auth_http: reqwest::Client,
}

impl RegistryClient {
    /// A client for the registry at `base_url` (e.g.
    /// `https://registry.example.com/luat`). Plain `http://` is refused
    /// except for localhost.
    pub fn new(base_url: &str) -> Result<Self> {
        let base = normalize_registry_url(base_url)?;
        let builder = || reqwest::Client::builder().user_agent(concat!("luat/", env!("CARGO_PKG_VERSION")));
        Ok(Self {
            base,
            token: None,
            http: builder().redirect(reqwest::redirect::Policy::limited(5)).build()?,
            auth_http: builder().redirect(reqwest::redirect::Policy::none()).build()?,
        })
    }

    /// Uses `token` as bearer token for authenticated operations (publish,
    /// yank, me). Reads never send it.
    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    /// The registry URL without trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// An anonymous request.
    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base))
    }

    /// An authenticated request to `base` (redirects are not followed).
    fn authed(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let token = self.token.as_ref().ok_or_else(|| PackageError::NoToken(self.base.clone()))?;
        Ok(self.auth_http.request(method, format!("{}{path}", self.base)).bearer_auth(token))
    }

    /// Every version of `name`, oldest first (`GET /index/@scope/name`).
    /// A package the registry does not know is a [`PackageError::NotFound`].
    pub async fn index(&self, name: &PackageName) -> Result<Vec<IndexEntry>> {
        let response = self.request(Method::GET, &format!("/index/{}", name.url_path())).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(PackageError::NotFound(format!("package {name} not found in {}", self.base)));
        }
        let text = check(response).await?.text().await?;
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<IndexEntry>(line).map_err(|e| {
                    PackageError::Manifest(format!("malformed index line for {name} from {}: {e}", self.base))
                })
            })
            .collect()
    }

    /// Downloads a tarball and verifies it against `expected` (`sha256:…`).
    pub async fn download(&self, name: &PackageName, version: &Version, expected: &str) -> Result<Vec<u8>> {
        let path = format!("/api/v1/packages/{}/{version}/download", name.url_path());
        let mut response = check(self.request(Method::GET, &path).send().await?).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > MAX_COMPRESSED_BYTES {
                return Err(PackageError::Tarball(format!("{name}@{version} download exceeds the size limit")));
            }
        }
        let actual = checksum(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(PackageError::ChecksumMismatch {
                name: name.to_string(),
                version: version.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
        Ok(bytes)
    }

    /// Package page data for the latest version.
    pub async fn package(&self, name: &PackageName) -> Result<PackageInfo> {
        self.get_json(&format!("/api/v1/packages/{}", name.url_path())).await
    }

    /// Package page data for one version.
    pub async fn package_version(&self, name: &PackageName, version: &Version) -> Result<PackageInfo> {
        self.get_json(&format!("/api/v1/packages/{}/{version}", name.url_path())).await
    }

    /// Searches packages (`q = None` lists all, most recently updated first).
    pub async fn search(&self, query: Option<&str>, page: u32, per_page: u32) -> Result<SearchResults> {
        let mut params = vec![("page", page.max(1).to_string()), ("per_page", per_page.clamp(1, 100).to_string())];
        if let Some(q) = query {
            params.insert(0, ("q", q.to_string()));
        }
        let response = self.request(Method::GET, "/api/v1/packages").query(&params).send().await?;
        Ok(check(response).await?.json().await?)
    }

    /// Publishes a tarball as `name@version`.
    pub async fn publish(&self, name: &PackageName, version: &Version, tarball: Vec<u8>) -> Result<PublishResult> {
        let response = self
            .authed(Method::PUT, &format!("/api/v1/packages/{}/{version}", name.url_path()))?
            .header(reqwest::header::CONTENT_TYPE, "application/gzip")
            .body(tarball)
            .send()
            .await?;
        Ok(check(response).await?.json().await?)
    }

    /// Yanks (`yanked = true`) or unyanks a version.
    pub async fn set_yanked(&self, name: &PackageName, version: &Version, yanked: bool) -> Result<()> {
        let (method, action) = if yanked { (Method::DELETE, "yank") } else { (Method::PUT, "unyank") };
        let path = format!("/api/v1/packages/{}/{version}/{action}", name.url_path());
        check(self.authed(method, &path)?.send().await?).await?;
        Ok(())
    }

    /// Who the token belongs to (`GET /api/v1/me`).
    pub async fn me(&self) -> Result<Me> {
        let response = self.authed(Method::GET, "/api/v1/me")?.send().await?;
        Ok(check(response).await?.json().await?)
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self.request(Method::GET, path).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(PackageError::NotFound(format!("{}{path}: not found", self.base)));
        }
        Ok(check(response).await?.json().await?)
    }
}

/// Turns error statuses into [`PackageError::Registry`] with the registry's
/// `{"error": "..."}` message.
async fn check(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let url = response.url().to_string();
    if status.is_redirection() {
        return Err(PackageError::Registry {
            url,
            status: status.as_u16(),
            message: "authenticated requests do not follow redirects; use the registry's final URL".to_string(),
        });
    }
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(500).collect());
    Err(PackageError::Registry {
        url,
        status: status.as_u16(),
        message,
    })
}
