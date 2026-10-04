// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP client module for Lua.
//!
//! Provides `http.get`, `http.post`, `http.put`, `http.delete`, and `http.request`
//! for making HTTP requests from Lua code.
//!
//! # Example
//!
//! ```lua
//! local http = require("http")
//! local json = require("json")
//!
//! -- Simple GET request
//! local response = http.get("https://api.example.com/users")
//! local users = json.decode(response.body)
//!
//! -- POST with JSON body
//! local response = http.post("https://api.example.com/users", {
//!     body = json.encode({ name = "John" }),
//!     headers = { ["Content-Type"] = "application/json" }
//! })
//! ```

use mlua::{Lua, Result as LuaResult, Table};
use std::collections::HashMap;
use std::time::Duration;

/// Default per-request timeout when the caller doesn't pass `timeout`.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Register the http module on the given Lua instance.
///
/// This makes `http.get()`, `http.post()`, `http.put()`, `http.delete()`,
/// `http.patch()` and `http.request()` available in Lua code, both as the
/// global `http` and via `require("http")`.
///
/// The functions are async: they suspend the calling coroutine instead of
/// blocking the thread, so they must be called from code the engine runs
/// asynchronously (`Engine::respond_async`).
pub fn register_http_module(lua: &Lua) -> LuaResult<()> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| mlua::Error::external(format!("Failed to create HTTP client: {}", e)))?;

    let http_module = lua.create_table()?;
    for method in ["GET", "POST", "PUT", "DELETE", "PATCH"] {
        let client = client.clone();
        let func = lua.create_async_function(move |lua, (url, options): (String, Option<Table>)| {
            let client = client.clone();
            async move {
                let request = RequestSpec::from_options(method, url, options.as_ref())?;
                send(&lua, &client, request).await
            }
        })?;
        http_module.set(method.to_lowercase(), func)?;
    }

    let request_fn = lua.create_async_function(move |lua, options: Table| {
        let client = client.clone();
        async move {
            let method: String = options.get("method").unwrap_or_else(|_| "GET".to_string());
            let url: String = options
                .get("url")
                .map_err(|_| mlua::Error::external("http.request requires 'url' field"))?;
            let request = RequestSpec::from_options(&method, url, Some(&options))?;
            send(&lua, &client, request).await
        }
    })?;
    http_module.set("request", request_fn)?;

    let globals = lua.globals();
    globals.set("http", http_module.clone())?;

    // require("http") returns the same table as the global.
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let loader = lua.create_function(move |_, _: ()| Ok(http_module.clone()))?;
    preload.set("http", loader)?;

    Ok(())
}

/// A request extracted from Lua arguments, owned so it can cross an await.
struct RequestSpec {
    method: reqwest::Method,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    timeout: Duration,
}

impl RequestSpec {
    fn from_options(method: &str, url: String, options: Option<&Table>) -> LuaResult<Self> {
        let method = match method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            other => {
                return Err(mlua::Error::external(format!("Unsupported HTTP method: {}", other)))
            }
        };

        let mut headers = HashMap::new();
        let mut body = None;
        let mut timeout_secs = DEFAULT_TIMEOUT_SECS;
        if let Some(opts) = options {
            if let Ok(headers_table) = opts.get::<Table>("headers") {
                headers.extend(headers_table.pairs::<String, String>().flatten());
            }
            body = opts.get::<String>("body").ok();
            timeout_secs = opts.get::<u64>("timeout").unwrap_or(DEFAULT_TIMEOUT_SECS);
        }

        Ok(Self {
            method,
            url,
            headers,
            body,
            timeout: Duration::from_secs(timeout_secs),
        })
    }
}

/// Sends the request and returns the response as a Lua table with
/// `status`, `ok`, `headers` and `body`.
async fn send(lua: &Lua, client: &reqwest::Client, spec: RequestSpec) -> LuaResult<Table> {
    let mut builder = client.request(spec.method, &spec.url).timeout(spec.timeout);
    for (key, value) in &spec.headers {
        builder = builder.header(key, value);
    }
    if let Some(body) = spec.body {
        builder = builder.body(body);
    }

    let response = builder
        .send()
        .await
        .map_err(|e| mlua::Error::external(format!("HTTP request failed: {}", e)))?;

    let result = lua.create_table()?;
    result.set("status", response.status().as_u16())?;
    result.set("ok", response.status().is_success())?;

    let response_headers = lua.create_table()?;
    for (key, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            response_headers.set(key.as_str(), v)?;
        }
    }
    result.set("headers", response_headers)?;

    let body_text = response
        .text()
        .await
        .map_err(|e| mlua::Error::external(format!("Failed to read response body: {}", e)))?;
    result.set("body", body_text)?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_module_registration() {
        let lua = Lua::new();
        register_http_module(&lua).expect("Failed to register http module");

        // Check that the module is accessible
        let result: bool = lua
            .load("return http ~= nil")
            .eval()
            .expect("Failed to check http global");
        assert!(result);

        // Check that methods exist
        let result: bool = lua
            .load("return type(http.get) == 'function'")
            .eval()
            .expect("Failed to check http.get");
        assert!(result);

        let result: bool = lua
            .load("return type(http.post) == 'function'")
            .eval()
            .expect("Failed to check http.post");
        assert!(result);
    }
}
