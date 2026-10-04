// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Request handling: routes a request to its page, form action or API
//! handler, collects cookies, and turns failures into error responses.
//!
//! # Errors
//!
//! - `ctx.error(status, message)` becomes a response with that status and
//!   message.
//! - Any other failure becomes a 500. Its details are logged; clients only
//!   see them in development mode (see `Engine::set_development_mode`).
//! - Page requests render the nearest `+error.luat` with `props.status` and
//!   `props.message`; API and form-action requests get `{ "error": message }`.
//! - Execution limit errors are returned to the host as `Err`, so it can
//!   apply its own policy (and discard the engine).

mod action;
mod api;
mod page;

use mlua::Table;

use crate::ctx_helpers::{CookieJar, HttpError};
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::request::LuatRequest;
use crate::resolver::ResourceResolver;
use crate::response::LuatResponse;
use crate::router::Route;

/// Message shown to clients for unexpected failures outside development mode.
const INTERNAL_ERROR_MESSAGE: &str = "Internal Server Error";

impl<R: ResourceResolver> Engine<R> {
    /// Handles a request using a pre-matched route.
    ///
    /// API routes (`+server.lua`) run their method handler; non-GET requests
    /// (or `?/name`) to a page with `+page.server.lua` run a form action; all
    /// other requests run layout and page `load` functions, render the page
    /// and wrap it in its layouts. Cookies set with `ctx.setCookie` are
    /// returned as `Set-Cookie` headers.
    ///
    /// Use [`respond_async`](Self::respond_async) when host functions are
    /// async.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let request = LuatRequest::new("/blog/hello", "GET");
    /// if let Some(route) = router.match_url(&request.path) {
    ///     let response = engine.respond(&route, &request)?;
    /// }
    /// ```
    pub fn respond(&self, route: &Route, request: &LuatRequest) -> Result<LuatResponse> {
        let jar = CookieJar::new();
        let response = match self.dispatch(route, request, &jar) {
            Ok(response) => response,
            Err(err) => {
                let (status, message) = self.classify_error(err)?;
                self.error_response(route, request, status, message)
            }
        };
        Ok(with_cookies(response, &jar))
    }

    /// Async variant of [`respond`](Self::respond). Server code and
    /// templates run as coroutines, so they may call async host functions.
    #[cfg(feature = "async-lua")]
    pub async fn respond_async(&self, route: &Route, request: &LuatRequest) -> Result<LuatResponse> {
        let jar = CookieJar::new();
        let response = match self.dispatch_async(route, request, &jar).await {
            Ok(response) => response,
            Err(err) => {
                let (status, message) = self.classify_error(err)?;
                self.error_response_async(route, request, status, message)
                    .await
            }
        };
        Ok(with_cookies(response, &jar))
    }

    fn dispatch(&self, route: &Route, request: &LuatRequest, jar: &CookieJar) -> Result<LuatResponse> {
        if route.is_api_route() {
            return self.handle_api_route(route, request, jar);
        }
        if self.is_action_request(route, request) {
            return self.handle_action(route, request, jar);
        }
        self.handle_page_route(route, request, jar)
    }

    #[cfg(feature = "async-lua")]
    async fn dispatch_async(
        &self,
        route: &Route,
        request: &LuatRequest,
        jar: &CookieJar,
    ) -> Result<LuatResponse> {
        if route.is_api_route() {
            return self.handle_api_route_async(route, request, jar).await;
        }
        if self.is_action_request(route, request) {
            return self.handle_action_async(route, request, jar).await;
        }
        self.handle_page_route_async(route, request, jar).await
    }

    /// Maps a failure to the status and client-safe message to respond with,
    /// or hands it back to the host (execution limits).
    fn classify_error(&self, err: LuatError) -> Result<(u16, String)> {
        #[cfg(not(target_arch = "wasm32"))]
        if crate::limits::LimitExceeded::from_error(&err).is_some() {
            return Err(err);
        }
        if let LuatError::LuaError(lua_err) = &err {
            if let Some(http) = HttpError::find(lua_err) {
                return Ok((http.status, http.message.clone()));
            }
        }
        tracing::error!(error = %err, "luat request failed");
        let message = if self.is_development() {
            err.to_string()
        } else {
            INTERNAL_ERROR_MESSAGE.to_string()
        };
        Ok((500, message))
    }

    /// Builds the error response for a page, API or action request.
    fn error_response(&self, route: &Route, request: &LuatRequest, status: u16, message: String) -> LuatResponse {
        if self.wants_json_errors(route, request) {
            return json_error(status, &message);
        }
        let Some(error_page) = route.error.as_deref() else {
            return LuatResponse::error(status, message);
        };
        let rendered = self
            .error_props(status, &message)
            .and_then(|props| self.render_page_module(error_page, &props));
        self.error_page_or_fallback(rendered, status, message)
    }

    #[cfg(feature = "async-lua")]
    async fn error_response_async(
        &self,
        route: &Route,
        request: &LuatRequest,
        status: u16,
        message: String,
    ) -> LuatResponse {
        if self.wants_json_errors(route, request) {
            return json_error(status, &message);
        }
        let Some(error_page) = route.error.as_deref() else {
            return LuatResponse::error(status, message);
        };
        let rendered = match self.error_props(status, &message) {
            Ok(props) => self.render_template_async(error_page, &props, None).await,
            Err(e) => Err(e),
        };
        self.error_page_or_fallback(rendered, status, message)
    }

    fn wants_json_errors(&self, route: &Route, request: &LuatRequest) -> bool {
        route.is_api_route() || self.is_action_request(route, request)
    }

    fn error_props(&self, status: u16, message: &str) -> Result<mlua::Value> {
        self.to_value(serde_json::json!({ "status": status, "message": message }))
    }

    fn render_page_module(&self, path: &str, props: &mlua::Value) -> Result<String> {
        let module = self.compile_entry(path)?;
        self.render(&module, props)
    }

    fn error_page_or_fallback(&self, rendered: Result<String>, status: u16, message: String) -> LuatResponse {
        match rendered {
            Ok(body) => LuatResponse::html(status, body),
            Err(err) => {
                tracing::error!(error = %err, "rendering +error.luat failed");
                LuatResponse::error(status, message)
            }
        }
    }

    /// Reads a `+page.server.lua`, `+layout.server.lua` or `+server.lua`
    /// source, from the resolver or from a preloaded bundle.
    fn resolve_server_source(&self, path: &str) -> Result<String> {
        match self.resolver.resolve("", path) {
            Ok(resolved) => Ok(resolved.source),
            Err(err) => self.server_source_from_bundle(path).ok_or(err),
        }
    }

    fn server_source_from_bundle(&self, path: &str) -> Option<String> {
        let server_sources: Table = self.lua.globals().get("__server_sources").ok()?;
        server_sources.get::<String>(path).ok()
    }

    fn is_not_found_error(&self, err: &LuatError) -> bool {
        match err {
            LuatError::ResolutionError(_) | LuatError::ModuleNotFound(_) => true,
            LuatError::LuaError(lua_err) => {
                let msg = lua_err.to_string();
                // Match both bundle errors and Lua's standard "module not found" errors
                msg.contains("not found in bundle") || msg.contains("not found:")
            }
            _ => false,
        }
    }

    /// Renders a template, falling back to a module preloaded from a bundle.
    #[cfg(feature = "async-lua")]
    async fn render_template_async(
        &self,
        module_path: &str,
        context: &mlua::Value,
        request_runtime: Option<&Table>,
    ) -> Result<String> {
        match self.compile_entry(module_path) {
            Ok(module) => self.render_async_in(&module, context, request_runtime).await,
            Err(err) if self.is_not_found_error(&err) => {
                self.render_from_bundle_in(module_path, context, request_runtime)
                    .await
            }
            Err(err) => Err(err),
        }
    }
}

/// Appends the request's `Set-Cookie` headers to `response`.
fn with_cookies(mut response: LuatResponse, jar: &CookieJar) -> LuatResponse {
    for cookie in jar.take() {
        response.headers_mut().append("set-cookie", cookie);
    }
    response
}

fn json_error(status: u16, message: &str) -> LuatResponse {
    LuatResponse::json(status, serde_json::json!({ "error": message }))
}
