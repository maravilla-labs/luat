// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Form actions: non-GET requests (or `?/name`) to a page that has a
//! `+page.server.lua` with an `actions` table.

use std::collections::HashMap;

use crate::actions::{ActionContext, ActionExecutor, ActionResponse};
use crate::ctx_helpers::CookieJar;
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::request::LuatRequest;
use crate::resolver::ResourceResolver;
use crate::response::{Headers, LuatResponse};
use crate::router::Route;

use super::json_error;

/// Result of the steps shared by the sync and async action paths.
enum Prepared {
    /// Run this action.
    Run { server_path: String, source: String, ctx: ActionContext },
    /// The request is answered without running anything.
    Done(LuatResponse),
}

impl<R: ResourceResolver> Engine<R> {
    pub(super) fn is_action_request(&self, route: &Route, request: &LuatRequest) -> bool {
        if route.page_server.is_none() {
            return false;
        }
        if !request.method.eq_ignore_ascii_case("GET") {
            return true;
        }
        request.action_name().is_some()
    }

    pub(super) fn handle_action(&self, route: &Route, request: &LuatRequest, jar: &CookieJar) -> Result<LuatResponse> {
        let (server_path, source, ctx) = match self.prepare_action(route, request)? {
            Prepared::Run { server_path, source, ctx } => (server_path, source, ctx),
            Prepared::Done(response) => return Ok(response),
        };
        let response = ActionExecutor::new(&self.lua)
            .with_cookies(jar.clone())
            .execute(&source, &server_path, &ctx)
            .map_err(LuatError::LuaError)?;

        let rendered = match action_template(route, &ctx) {
            Some(template) => {
                let context = self.to_value(&response.data)?;
                let module = self.compile_entry(&template)?;
                Some(self.render(&module, &context)?)
            }
            None => None,
        };
        Ok(action_response(response, rendered))
    }

    #[cfg(feature = "async-lua")]
    pub(super) async fn handle_action_async(
        &self,
        route: &Route,
        request: &LuatRequest,
        jar: &CookieJar,
    ) -> Result<LuatResponse> {
        let (server_path, source, ctx) = match self.prepare_action(route, request)? {
            Prepared::Run { server_path, source, ctx } => (server_path, source, ctx),
            Prepared::Done(response) => return Ok(response),
        };
        let response = ActionExecutor::new(&self.lua)
            .with_cookies(jar.clone())
            .execute_async(&source, &server_path, &ctx)
            .await
            .map_err(LuatError::LuaError)?;

        let rendered = match action_template(route, &ctx) {
            Some(template) => {
                let context = self.to_value(&response.data)?;
                Some(self.render_template_async(&template, &context, None).await?)
            }
            None => None,
        };
        Ok(action_response(response, rendered))
    }

    fn prepare_action(&self, route: &Route, request: &LuatRequest) -> Result<Prepared> {
        let Some(server_path) = route.page_server.clone() else {
            return Ok(Prepared::Done(json_error(405, "No server handler")));
        };
        let ctx = match build_action_context(request, &route.params) {
            Ok(ctx) => ctx,
            // The body could not be parsed: the client's fault, and the
            // parser's message describes only their input.
            Err(message) => return Ok(Prepared::Done(json_error(400, &message))),
        };
        let source = self.resolve_server_source(&server_path)?;
        Ok(Prepared::Run { server_path, source, ctx })
    }
}

fn build_action_context(
    request: &LuatRequest,
    params: &HashMap<String, String>,
) -> std::result::Result<ActionContext, String> {
    let body = match request.body.as_ref() {
        Some(body) => crate::body::parse_action_body(body, request.content_type()).map_err(|e| e.to_string())?,
        None => serde_json::Value::Null,
    };

    let query = request
        .query
        .iter()
        .filter(|(k, _)| !k.starts_with('/'))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    Ok(ActionContext::new(&request.method, &action_url(request))
        .with_params(params.clone())
        .with_query(query)
        .with_headers(request.headers.clone())
        .with_cookies(request.cookie_map())
        .with_body(body)
        .with_action(request.action_name().map(|s| s.to_string())))
}

fn action_url(request: &LuatRequest) -> String {
    if request.query.is_empty() {
        return request.path.clone();
    }
    let pairs: Vec<String> = request
        .query
        .iter()
        .map(|(key, value)| {
            if value.is_empty() {
                key.clone()
            } else {
                format!("{}={}", key, value)
            }
        })
        .collect();
    format!("{}?{}", request.path, pairs.join("&"))
}

/// Finds the fragment template for this action among the route's
/// `(fragments)` templates: `METHOD-name` first, then `name`.
fn action_template(route: &Route, ctx: &ActionContext) -> Option<String> {
    let action_name = ctx.effective_action_name();
    let candidates = [
        format!("{}-{}", ctx.method.to_uppercase(), action_name),
        action_name.to_string(),
    ];
    candidates.iter().find_map(|candidate| {
        route
            .action_templates
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(candidate))
            .map(|(_, path)| path.clone())
    })
}

/// Turns the action's result into a response: a redirect, an HTML fragment
/// (marked with `x-luat-fragment`), or JSON.
fn action_response(response: ActionResponse, rendered_html: Option<String>) -> LuatResponse {
    let mut headers: Headers = response.headers.into();
    if (300..400).contains(&response.status) {
        if let Some(location) = headers.remove("location") {
            return LuatResponse::Redirect {
                status: response.status,
                location,
                headers,
            };
        }
    }
    if let Some(html) = rendered_html {
        headers.insert("x-luat-fragment", "1");
        return LuatResponse::html_with_headers(response.status, html, headers);
    }
    LuatResponse::json_with_headers(response.status, response.data, headers)
}
