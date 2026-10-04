// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Converts axum requests into engine requests.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use axum::response::Response;
use luat::LuatRequest;

/// Largest request body the servers accept.
const MAX_BODY_SIZE: usize = 1024 * 1024;

/// Reads an axum request into a [`LuatRequest`]: URL-decoded query, header
/// map, and the body for methods that carry one.
pub async fn to_luat_request(request: Request<Body>) -> Result<LuatRequest, Response> {
    let (parts, body) = request.into_parts();

    let query: HashMap<String, String> = parts
        .uri
        .query()
        .map(|q| form_urlencoded::parse(q.as_bytes()).into_owned().collect())
        .unwrap_or_default();

    let headers: HashMap<String, String> = parts
        .headers
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
        .collect();

    let mut luat_request = LuatRequest::new(parts.uri.path(), parts.method.as_str())
        .with_query(query)
        .with_headers(headers);

    if parts.method != Method::GET && parts.method != Method::HEAD {
        let bytes = axum::body::to_bytes(body, MAX_BODY_SIZE)
            .await
            .map_err(|_| super::response::error(413, "Request body too large"))?;
        if !bytes.is_empty() {
            luat_request = luat_request.with_body(bytes.to_vec());
        }
    }
    Ok(luat_request)
}
