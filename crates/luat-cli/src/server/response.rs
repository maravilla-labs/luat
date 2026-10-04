// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Maps finalized engine responses onto axum.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use luat::{AppShell, HttpResponse, LuatRequest, LuatResponse, ShellOptions};

/// Converts a finalized response into an axum response.
pub fn to_axum(response: HttpResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    for (name, value) in response.headers.iter() {
        match (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            (Ok(name), Ok(value)) => builder = builder.header(name, value),
            _ => tracing::warn!(header = name, "dropping invalid response header"),
        }
    }
    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// A standalone error page with the given status.
pub fn error(status: u16, message: impl Into<String>) -> Response {
    to_axum(luat::finalize(
        LuatResponse::error(status, message),
        &LuatRequest::default(),
        &AppShell::default(),
        &ShellOptions::default(),
    ))
}
