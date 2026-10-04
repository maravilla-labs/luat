// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! A small in-process registry implementing the API of `docs/packages.md`,
//! for tests. State lives in memory.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use luat::packages::{checksum, read_tarball, Dependency, IndexEntry, PackageManifest, PackageName};
use serde_json::json;

pub const TOKEN: &str = "test-token";

#[derive(Default)]
pub struct Store {
    /// name -> versions, oldest first: (index entry, tarball).
    pub packages: BTreeMap<String, Vec<(IndexEntry, Vec<u8>)>>,
    /// `name@version`s whose downloads are corrupted.
    pub tampered: HashSet<String>,
}

pub type Shared = Arc<Mutex<Store>>;

/// Starts the registry; returns its URL and its state.
pub async fn start() -> (String, Shared) {
    let store: Shared = Arc::default();
    let app = Router::new()
        .route("/index/:scope/:name", get(index))
        .route("/api/v1/packages", get(search))
        .route("/api/v1/packages/:scope/:name", get(info))
        .route("/api/v1/packages/:scope/:name/:version", get(info_version).put(publish))
        .route("/api/v1/packages/:scope/:name/:version/download", get(download))
        .route("/api/v1/packages/:scope/:name/:version/yank", delete(yank))
        .route("/api/v1/packages/:scope/:name/:version/unyank", put(unyank))
        .route("/api/v1/me", get(me))
        .with_state(store.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, store)
}

/// Stores raw tarball bytes as `name@vers` without any validation (for
/// malicious-tarball tests).
pub fn inject(store: &Shared, name: &str, vers: &str, bytes: Vec<u8>) {
    let entry = IndexEntry {
        name: PackageName::parse(name).unwrap(),
        vers: semver::Version::parse(vers).unwrap(),
        deps: BTreeMap::new(),
        cksum: checksum(&bytes),
        luat: None,
        yanked: false,
    };
    store.lock().unwrap().packages.entry(name.to_string()).or_default().push((entry, bytes));
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn authorized(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {TOKEN}"))
}

async fn index(State(store): State<Shared>, Path((scope, name)): Path<(String, String)>) -> Response {
    let store = store.lock().unwrap();
    match store.packages.get(&format!("{scope}/{name}")) {
        Some(versions) => versions
            .iter()
            .map(|(e, _)| serde_json::to_string(e).unwrap() + "\n")
            .collect::<String>()
            .into_response(),
        None => error(StatusCode::NOT_FOUND, "no such package"),
    }
}

fn info_json(name: &str, versions: &[(IndexEntry, Vec<u8>)], which: Option<&str>) -> Option<serde_json::Value> {
    let latest = versions.iter().rev().find(|(e, _)| !e.yanked).or(versions.last())?;
    let described = match which {
        Some(v) => versions.iter().find(|(e, _)| e.vers.to_string() == v)?,
        None => latest,
    };
    let files: Vec<String> = read_tarball(&described.1).map(|f| f.into_iter().map(|f| f.path).collect()).unwrap_or_default();
    Some(json!({
        "name": name,
        "description": null,
        "latest": latest.0.vers.to_string(),
        "versions": versions.iter().map(|(e, b)| json!({"vers": e.vers.to_string(), "yanked": e.yanked, "size": b.len()})).collect::<Vec<_>>(),
        "dependencies": described.0.deps.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
        "files": files,
    }))
}

async fn info(State(store): State<Shared>, Path((scope, name)): Path<(String, String)>) -> Response {
    let full = format!("{scope}/{name}");
    let store = store.lock().unwrap();
    match store.packages.get(&full).and_then(|v| info_json(&full, v, None)) {
        Some(body) => Json(body).into_response(),
        None => error(StatusCode::NOT_FOUND, "no such package"),
    }
}

async fn info_version(State(store): State<Shared>, Path((scope, name, version)): Path<(String, String, String)>) -> Response {
    let full = format!("{scope}/{name}");
    let store = store.lock().unwrap();
    match store.packages.get(&full).and_then(|v| info_json(&full, v, Some(&version))) {
        Some(body) => Json(body).into_response(),
        None => error(StatusCode::NOT_FOUND, "no such version"),
    }
}

#[derive(serde::Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn search(State(store): State<Shared>, Query(query): Query<SearchQuery>) -> Response {
    let store = store.lock().unwrap();
    let hits: Vec<_> = store
        .packages
        .iter()
        .filter(|(name, _)| query.q.as_deref().map_or(true, |q| name.contains(q)))
        .map(|(name, versions)| json!({"name": name, "description": null, "latest": versions.last().unwrap().0.vers.to_string()}))
        .collect();
    Json(json!({ "total": hits.len(), "packages": hits })).into_response()
}

async fn download(State(store): State<Shared>, Path((scope, name, version)): Path<(String, String, String)>) -> Response {
    let full = format!("{scope}/{name}");
    let store = store.lock().unwrap();
    let Some((_, bytes)) = store.packages.get(&full).and_then(|v| v.iter().find(|(e, _)| e.vers.to_string() == version)) else {
        return error(StatusCode::NOT_FOUND, "no such version");
    };
    let mut bytes = bytes.clone();
    if store.tampered.contains(&format!("{full}@{version}")) {
        bytes.push(0);
    }
    ([("content-type", "application/gzip")], bytes).into_response()
}

async fn publish(
    State(store): State<Shared>,
    Path((scope, name, version)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&headers) {
        return error(StatusCode::UNAUTHORIZED, "bad token");
    }
    let full = format!("{scope}/{name}");
    let Ok(files) = read_tarball(&body) else { return error(StatusCode::BAD_REQUEST, "invalid tarball") };
    let Some(toml) = files.iter().find(|f| f.path == "luat.toml") else {
        return error(StatusCode::BAD_REQUEST, "no luat.toml");
    };
    let Ok(manifest) = PackageManifest::parse(std::str::from_utf8(&toml.data).unwrap_or("")) else {
        return error(StatusCode::BAD_REQUEST, "bad luat.toml");
    };
    let Some(meta) = manifest.package.clone() else { return error(StatusCode::BAD_REQUEST, "no [package]") };
    if meta.name.to_string() != full || meta.version.to_string() != version {
        return error(StatusCode::BAD_REQUEST, "name or version does not match the URL");
    }
    let mut deps = BTreeMap::new();
    for (dep, spec) in &manifest.dependencies {
        match spec {
            Dependency::Registry(req) => deps.insert(dep.clone(), req.clone()),
            Dependency::Path { .. } => return error(StatusCode::BAD_REQUEST, "path dependencies"),
        };
    }
    let mut store = store.lock().unwrap();
    let versions = store.packages.entry(full.clone()).or_default();
    if versions.iter().any(|(e, _)| e.vers == meta.version) {
        return error(StatusCode::CONFLICT, "version exists");
    }
    let cksum = checksum(&body);
    let entry = IndexEntry { name: meta.name, vers: meta.version, deps, cksum: cksum.clone(), luat: meta.luat, yanked: false };
    versions.push((entry, body.to_vec()));
    (StatusCode::CREATED, Json(json!({"name": full, "vers": version, "cksum": cksum}))).into_response()
}

fn set_yanked(store: &Shared, full: &str, version: &str, yanked: bool) -> Response {
    let mut store = store.lock().unwrap();
    match store.packages.get_mut(full).and_then(|v| v.iter_mut().find(|(e, _)| e.vers.to_string() == version)) {
        Some((entry, _)) => {
            entry.yanked = yanked;
            Json(json!({})).into_response()
        }
        None => error(StatusCode::NOT_FOUND, "no such version"),
    }
}

async fn yank(State(store): State<Shared>, Path((scope, name, version)): Path<(String, String, String)>, headers: HeaderMap) -> Response {
    if !authorized(&headers) {
        return error(StatusCode::UNAUTHORIZED, "bad token");
    }
    set_yanked(&store, &format!("{scope}/{name}"), &version, true)
}

async fn unyank(State(store): State<Shared>, Path((scope, name, version)): Path<(String, String, String)>, headers: HeaderMap) -> Response {
    if !authorized(&headers) {
        return error(StatusCode::UNAUTHORIZED, "bad token");
    }
    set_yanked(&store, &format!("{scope}/{name}"), &version, false)
}

async fn me(headers: HeaderMap) -> Response {
    if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer unbound") {
        return error(StatusCode::FORBIDDEN, "token is not bound to a user");
    }
    if !authorized(&headers) {
        return error(StatusCode::UNAUTHORIZED, "bad token");
    }
    Json(json!({"user": "tester", "scopes": ["acme"]})).into_response()
}
