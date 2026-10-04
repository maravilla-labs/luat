// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! API routes (`+server.lua`): one handler function per HTTP method.

use crate::ctx_helpers::CookieJar;
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::request::LuatRequest;
use crate::resolver::ResourceResolver;
use crate::response::LuatResponse;
use crate::router::Route;
use crate::runtime::{ApiResult, Runtime};

impl<R: ResourceResolver> Engine<R> {
    pub(super) fn handle_api_route(&self, route: &Route, request: &LuatRequest, jar: &CookieJar) -> Result<LuatResponse> {
        let api_path = api_path(route)?;
        let source = self.resolve_server_source(api_path)?;
        let result = Runtime::new(&self.lua)
            .with_cookies(jar.clone())
            .run_api(&source, api_path, request, &route.params)
            .map_err(LuatError::LuaError)?;
        Ok(api_response(result))
    }

    #[cfg(feature = "async-lua")]
    pub(super) async fn handle_api_route_async(
        &self,
        route: &Route,
        request: &LuatRequest,
        jar: &CookieJar,
    ) -> Result<LuatResponse> {
        let api_path = api_path(route)?;
        let source = self.resolve_server_source(api_path)?;
        let result = Runtime::new(&self.lua)
            .with_cookies(jar.clone())
            .run_api_async(&source, api_path, request, &route.params)
            .await
            .map_err(LuatError::LuaError)?;
        Ok(api_response(result))
    }
}

fn api_path(route: &Route) -> Result<&str> {
    route
        .api
        .as_deref()
        .ok_or_else(|| LuatError::InvalidTemplate("API route has no +server.lua".to_string()))
}

/// A `Location` header makes a redirect; a string body is sent raw;
/// anything else is JSON.
fn api_response(result: ApiResult) -> LuatResponse {
    let ApiResult { status, body, raw, mut headers } = result;
    if let Some(location) = headers.remove("location") {
        return LuatResponse::Redirect { status, location, headers };
    }
    match raw {
        Some(bytes) => LuatResponse::body(status, bytes, headers),
        None => LuatResponse::json_with_headers(status, body, headers),
    }
}
