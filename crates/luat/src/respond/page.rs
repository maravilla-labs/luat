// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Page routes: layout and page `load` functions, then the page rendered
//! inside its layouts.

use mlua::Table;
use serde_json::{Map, Value as JsonValue};

use crate::ctx_helpers::CookieJar;
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::request::LuatRequest;
use crate::resolver::ResourceResolver;
use crate::response::{Headers, LuatResponse};
use crate::router::Route;
use crate::runtime::{LoadResult, Runtime};

impl<R: ResourceResolver> Engine<R> {
    /// Runs layout and page load functions (root to leaf), renders the page,
    /// then wraps it in its layouts from innermost to outermost.
    pub(super) fn handle_page_route(
        &self,
        route: &Route,
        request: &LuatRequest,
        jar: &CookieJar,
    ) -> Result<LuatResponse> {
        let request_runtime = self.new_request_runtime()?;
        let runtime =
            Runtime::with_request_runtime(&self.lua, request_runtime.clone()).with_cookies(jar.clone());
        let mut merged_props = Map::new();

        for server_path in route.layout_servers.iter().chain(route.page_server.iter()) {
            let source = self.resolve_server_source(server_path)?;
            let load_result = runtime
                .run_load(&source, server_path, request, &route.params)
                .map_err(LuatError::LuaError)?;
            if let Some(redirect) = merge_load_result(&mut merged_props, load_result, jar) {
                return Ok(redirect);
            }
        }

        let context = self.to_value(JsonValue::Object(merged_props.clone()))?;
        let mut body_html = self.render_template(page_path(route)?, &context, Some(&request_runtime))?;

        for layout_path in route.layouts.iter().rev() {
            let mut layout_props = merged_props.clone();
            layout_props.insert("children".to_string(), JsonValue::String(body_html));
            let layout_context = self.to_value(JsonValue::Object(layout_props))?;
            body_html = self.render_template(layout_path, &layout_context, Some(&request_runtime))?;
        }

        self.page_response(body_html, &request_runtime, jar)
    }

    /// Async variant of [`handle_page_route`](Self::handle_page_route), with
    /// fallback to bundle rendering.
    #[cfg(feature = "async-lua")]
    pub(super) async fn handle_page_route_async(
        &self,
        route: &Route,
        request: &LuatRequest,
        jar: &CookieJar,
    ) -> Result<LuatResponse> {
        let request_runtime = self.new_request_runtime()?;
        let runtime =
            Runtime::with_request_runtime(&self.lua, request_runtime.clone()).with_cookies(jar.clone());
        let mut merged_props = Map::new();

        for server_path in route.layout_servers.iter().chain(route.page_server.iter()) {
            let source = self.resolve_server_source(server_path)?;
            let load_result = runtime
                .run_load_async(&source, server_path, request, &route.params)
                .await
                .map_err(LuatError::LuaError)?;
            if let Some(redirect) = merge_load_result(&mut merged_props, load_result, jar) {
                return Ok(redirect);
            }
        }

        let context = self.to_value(JsonValue::Object(merged_props.clone()))?;
        let mut body_html = self
            .render_template_async(page_path(route)?, &context, Some(&request_runtime))
            .await?;

        for layout_path in route.layouts.iter().rev() {
            let mut layout_props = merged_props.clone();
            layout_props.insert("children".to_string(), JsonValue::String(body_html));
            let layout_context = self.to_value(JsonValue::Object(layout_props))?;
            body_html = self
                .render_template_async(layout_path, &layout_context, Some(&request_runtime))
                .await?;
        }

        self.page_response(body_html, &request_runtime, jar)
    }

    /// Creates the per-request runtime table shared by a request's load
    /// functions and templates (`context_stack`, `page_context`).
    pub(crate) fn new_request_runtime(&self) -> Result<Table> {
        let request_runtime = self.lua.create_table()?;
        let context_stack: Table = self.lua.create_sequence_from::<Table>(vec![])?;
        request_runtime.set("context_stack", context_stack)?;
        // Non-scoped page context for view_title etc.
        request_runtime.set("page_context", self.lua.create_table()?)?;
        Ok(request_runtime)
    }

    /// Builds the HTML response with the status set by `ctx.setStatus` (or
    /// a load function's `status`), 200 by default, adding `x-luat-title`
    /// when a template or load function set `view_title`. Headers from
    /// `ctx.setHeader` are added later, with the cookies.
    fn page_response(&self, body: String, request_runtime: &Table, jar: &CookieJar) -> Result<LuatResponse> {
        let mut headers = Headers::new();
        if let Some(title) = view_title(request_runtime) {
            headers.insert("x-luat-title", title);
        }
        Ok(LuatResponse::html_with_headers(jar.status().unwrap_or(200), body, headers))
    }
}

/// Applies a load result to the merged props, or returns the redirect
/// response the load function asked for. A `status` returned without
/// `redirect` sets the page status like `ctx.setStatus` (values that are not
/// 2xx, 4xx or 5xx are ignored, as they always were).
fn merge_load_result(
    merged_props: &mut Map<String, JsonValue>,
    load_result: LoadResult,
    jar: &CookieJar,
) -> Option<LuatResponse> {
    if let Some(redirect) = load_result.redirect {
        let status = load_result.status.unwrap_or(302);
        return Some(LuatResponse::redirect_with_status(status, redirect));
    }
    if let Some(status) = load_result
        .status
        .and_then(|s| crate::ctx_helpers::validate_status(i64::from(s)).ok())
    {
        jar.set_status(status);
    }
    if let JsonValue::Object(props) = load_result.props {
        merged_props.extend(props);
    }
    None
}

fn page_path(route: &Route) -> Result<&str> {
    route
        .page
        .as_deref()
        .ok_or_else(|| LuatError::InvalidTemplate("Page route has no +page.luat".to_string()))
}

/// Reads `view_title` from page_context (preferred) or, for backwards
/// compatibility, from the context stack (most recent scope first).
fn view_title(runtime: &Table) -> Option<String> {
    let as_string = |scope: &Table| match scope.get::<mlua::Value>("view_title") {
        Ok(mlua::Value::String(s)) => s.to_str().ok().map(|t| t.to_string()),
        _ => None,
    };

    if let Some(title) = runtime.get::<Table>("page_context").ok().as_ref().and_then(as_string) {
        return Some(title);
    }
    let stack: Table = runtime.get("context_stack").ok()?;
    (1..=stack.len().unwrap_or(0))
        .rev()
        .filter_map(|i| stack.get::<Table>(i).ok())
        .find_map(|scope| as_string(&scope))
}
