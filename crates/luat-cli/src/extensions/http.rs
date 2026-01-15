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

/// Register the http module on the given Lua instance.
///
/// This makes `http.get()`, `http.post()`, `http.put()`, `http.delete()`,
/// and `http.request()` available in Lua code.
pub fn register_http_module(lua: &Lua) -> LuaResult<()> {
    let http_module = lua.create_table()?;

    // GET request
    let get_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
        let (url, options) = args;
        make_request(lua, "GET", &url, options)
    })?;
    http_module.set("get", get_fn)?;

    // POST request
    let post_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
        let (url, options) = args;
        make_request(lua, "POST", &url, options)
    })?;
    http_module.set("post", post_fn)?;

    // PUT request
    let put_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
        let (url, options) = args;
        make_request(lua, "PUT", &url, options)
    })?;
    http_module.set("put", put_fn)?;

    // DELETE request
    let delete_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
        let (url, options) = args;
        make_request(lua, "DELETE", &url, options)
    })?;
    http_module.set("delete", delete_fn)?;

    // PATCH request
    let patch_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
        let (url, options) = args;
        make_request(lua, "PATCH", &url, options)
    })?;
    http_module.set("patch", patch_fn)?;

    // Generic request
    let request_fn = lua.create_function(|lua, options: Table| {
        let method: String = options.get("method").unwrap_or_else(|_| "GET".to_string());
        let url: String = options.get("url").map_err(|_| {
            mlua::Error::external("http.request requires 'url' field")
        })?;
        make_request(lua, &method, &url, Some(options))
    })?;
    http_module.set("request", request_fn)?;

    // Register as global 'http'
    let globals = lua.globals();
    globals.set("http", http_module.clone())?;

    // Also register in package.preload for require("http")
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;

    let http_loader = lua.create_function(move |lua, _: ()| {
        let module = lua.create_table()?;

        let get_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
            let (url, options) = args;
            make_request(lua, "GET", &url, options)
        })?;
        module.set("get", get_fn)?;

        let post_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
            let (url, options) = args;
            make_request(lua, "POST", &url, options)
        })?;
        module.set("post", post_fn)?;

        let put_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
            let (url, options) = args;
            make_request(lua, "PUT", &url, options)
        })?;
        module.set("put", put_fn)?;

        let delete_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
            let (url, options) = args;
            make_request(lua, "DELETE", &url, options)
        })?;
        module.set("delete", delete_fn)?;

        let patch_fn = lua.create_function(|lua, args: (String, Option<Table>)| {
            let (url, options) = args;
            make_request(lua, "PATCH", &url, options)
        })?;
        module.set("patch", patch_fn)?;

        let request_fn = lua.create_function(|lua, options: Table| {
            let method: String = options.get("method").unwrap_or_else(|_| "GET".to_string());
            let url: String = options.get("url").map_err(|_| {
                mlua::Error::external("http.request requires 'url' field")
            })?;
            make_request(lua, &method, &url, Some(options))
        })?;
        module.set("request", request_fn)?;

        Ok(module)
    })?;
    preload.set("http", http_loader)?;

    Ok(())
}

/// Make an HTTP request and return the response as a Lua table.
fn make_request(lua: &Lua, method: &str, url: &str, options: Option<Table>) -> LuaResult<Table> {
    // Extract options
    let mut headers_map: HashMap<String, String> = HashMap::new();
    let mut body: Option<String> = None;
    let mut timeout_secs: Option<u64> = None;

    if let Some(ref opts) = options {
        // Extract headers
        if let Ok(headers_table) = opts.get::<Table>("headers") {
            for (k, v) in headers_table.pairs::<String, String>().flatten() {
                headers_map.insert(k, v);
            }
        }

        // Extract body
        body = opts.get::<String>("body").ok();

        // Extract timeout
        timeout_secs = opts.get::<u64>("timeout").ok();
    }

    // Build the request
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.unwrap_or(30)))
        .build()
        .map_err(|e| mlua::Error::external(format!("Failed to create HTTP client: {}", e)))?;

    let mut request_builder = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        "HEAD" => client.head(url),
        _ => return Err(mlua::Error::external(format!("Unsupported HTTP method: {}", method))),
    };

    // Add headers
    for (key, value) in headers_map {
        request_builder = request_builder.header(&key, &value);
    }

    // Add body
    if let Some(body_str) = body {
        request_builder = request_builder.body(body_str);
    }

    // Execute request
    let response = request_builder
        .send()
        .map_err(|e| mlua::Error::external(format!("HTTP request failed: {}", e)))?;

    // Build response table
    let result = lua.create_table()?;

    // Status code
    result.set("status", response.status().as_u16())?;
    result.set("ok", response.status().is_success())?;

    // Response headers
    let response_headers = lua.create_table()?;
    for (key, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            response_headers.set(key.as_str(), v)?;
        }
    }
    result.set("headers", response_headers)?;

    // Response body
    let body_text = response
        .text()
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
