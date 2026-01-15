// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Server-side load function execution for routes.
//!
//! Handles executing `+page.server.lua` and `+layout.server.lua` files
//! to fetch data for page rendering.

use axum::http::{HeaderMap, Method};
use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

/// Context passed to load functions
#[derive(Debug, Clone)]
pub struct LoadContext {
    /// URL parameters extracted from the route
    pub params: HashMap<String, String>,

    /// The full request URL
    pub url: String,

    /// HTTP method
    pub method: Method,

    /// Request headers
    pub headers: HeaderMap,

    /// Form data (for POST requests)
    pub form: Option<HashMap<String, String>>,

    /// Query parameters
    pub query: HashMap<String, String>,
}

impl LoadContext {
    /// Creates a new load context with URL, method, and route parameters.
    pub fn new(url: String, method: Method, params: Vec<(String, String)>) -> Self {
        Self {
            params: params.into_iter().collect(),
            url,
            method,
            headers: HeaderMap::new(),
            form: None,
            query: HashMap::new(),
        }
    }

    /// Adds request headers to the context.
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    /// Adds form data to the context.
    pub fn with_form(mut self, form: HashMap<String, String>) -> Self {
        self.form = Some(form);
        self
    }

    /// Adds query parameters to the context.
    pub fn with_query(mut self, query: HashMap<String, String>) -> Self {
        self.query = query;
        self
    }
}

/// Result of running a load function
#[derive(Debug)]
pub struct LoadResult {
    /// The props returned by the load function
    pub props: JsonValue,

    /// Optional redirect URL
    pub redirect: Option<String>,

    /// Optional HTTP status code
    pub status: Option<u16>,
}

impl Default for LoadResult {
    fn default() -> Self {
        Self {
            props: JsonValue::Object(serde_json::Map::new()),
            redirect: None,
            status: None,
        }
    }
}

/// Sets up package.path to include the lib directory for require() calls
fn setup_package_path(lua: &Lua, lib_dir: &Path) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let current_path: String = package.get("path")?;

    let lib_path = lib_dir.to_string_lossy();
    let new_path = format!(
        "{}/?.lua;{}/?/init.lua;{}",
        lib_path, lib_path, current_path
    );

    package.set("path", new_path)?;
    Ok(())
}

/// Runs a load function from a server file and returns the props
pub fn run_load_function(
    lua: &Lua,
    server_file: &Path,
    ctx: &LoadContext,
    lib_dir: Option<&Path>,
) -> LuaResult<LoadResult> {
    // Set up package.path if lib_dir is provided
    if let Some(lib_dir) = lib_dir {
        setup_package_path(lua, lib_dir)?;
    }

    // Read and execute the server file
    let source = std::fs::read_to_string(server_file)?;
    lua.load(&source).set_name(server_file.to_string_lossy()).exec()?;

    // Get the load function
    let globals = lua.globals();
    let load_fn: Option<Function> = globals.get("load").ok();

    let Some(load_fn) = load_fn else {
        // No load function, return empty props
        return Ok(LoadResult::default());
    };

    // Create context table for Lua
    let ctx_table = lua.create_table()?;

    // Add params
    let params_table = lua.create_table()?;
    for (key, value) in &ctx.params {
        params_table.set(key.as_str(), value.as_str())?;
    }
    ctx_table.set("params", params_table)?;

    // Add URL
    ctx_table.set("url", ctx.url.as_str())?;

    // Add method
    ctx_table.set("method", ctx.method.as_str())?;

    // Add query params
    let query_table = lua.create_table()?;
    for (key, value) in &ctx.query {
        query_table.set(key.as_str(), value.as_str())?;
    }
    ctx_table.set("query", query_table)?;

    // Add form data if present
    if let Some(form) = &ctx.form {
        let form_table = lua.create_table()?;
        for (key, value) in form {
            form_table.set(key.as_str(), value.as_str())?;
        }
        ctx_table.set("form", form_table)?;
    }

    // Call the load function
    let result: Value = load_fn.call(ctx_table)?;

    // Convert result to LoadResult
    parse_load_result(lua, result)
}

/// Runs an API handler function (GET, POST, etc.)
pub fn run_api_handler(
    lua: &Lua,
    server_file: &Path,
    ctx: &LoadContext,
    lib_dir: Option<&Path>,
) -> LuaResult<ApiResponse> {
    // Set up package.path if lib_dir is provided
    if let Some(lib_dir) = lib_dir {
        setup_package_path(lua, lib_dir)?;
    }

    // Read and execute the server file
    let source = std::fs::read_to_string(server_file)?;
    lua.load(&source).set_name(server_file.to_string_lossy()).exec()?;

    // Get the handler function based on method
    let globals = lua.globals();
    let method_name = ctx.method.as_str();
    let handler_fn: Option<Function> = globals.get(method_name).ok();

    let Some(handler_fn) = handler_fn else {
        return Ok(ApiResponse {
            status: 405,
            body: JsonValue::Object({
                let mut map = serde_json::Map::new();
                map.insert("error".to_string(), JsonValue::String(format!("Method {} not allowed", method_name)));
                map
            }),
            headers: HashMap::new(),
        });
    };

    // Create context table for Lua
    let ctx_table = lua.create_table()?;

    // Add params
    let params_table = lua.create_table()?;
    for (key, value) in &ctx.params {
        params_table.set(key.as_str(), value.as_str())?;
    }
    ctx_table.set("params", params_table)?;

    // Add URL and method
    ctx_table.set("url", ctx.url.as_str())?;
    ctx_table.set("method", method_name)?;

    // Add query params
    let query_table = lua.create_table()?;
    for (key, value) in &ctx.query {
        query_table.set(key.as_str(), value.as_str())?;
    }
    ctx_table.set("query", query_table)?;

    // Add form/json data if present
    if let Some(form) = &ctx.form {
        let form_table = lua.create_table()?;
        for (key, value) in form {
            form_table.set(key.as_str(), value.as_str())?;
        }
        ctx_table.set("form", form_table.clone())?;
        ctx_table.set("json", form_table)?;
    }

    // Call the handler function
    let result: Value = handler_fn.call(ctx_table)?;

    // Parse API response
    parse_api_response(lua, result)
}

/// Response from an API handler.
#[derive(Debug)]
pub struct ApiResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body as JSON.
    pub body: JsonValue,
    /// Response headers.
    pub headers: HashMap<String, String>,
}

impl Default for ApiResponse {
    fn default() -> Self {
        Self {
            status: 200,
            body: JsonValue::Null,
            headers: HashMap::new(),
        }
    }
}

/// Parse a Lua value into LoadResult
fn parse_load_result(lua: &Lua, value: Value) -> LuaResult<LoadResult> {
    let mut result = LoadResult::default();

    match value {
        Value::Table(table) => {
            // Check for special keys
            if let Ok(redirect) = table.get::<String>("redirect") {
                result.redirect = Some(redirect);
            }

            if let Ok(status) = table.get::<u16>("status") {
                result.status = Some(status);
            }

            // Convert the rest to JSON props
            result.props = table_to_json(lua, &table)?;
        }
        Value::Nil => {
            // Return empty props
        }
        _ => {
            // Wrap non-table values
            result.props = lua_to_json(lua, &value)?;
        }
    }

    Ok(result)
}

/// Parse a Lua value into ApiResponse
fn parse_api_response(lua: &Lua, value: Value) -> LuaResult<ApiResponse> {
    let mut response = ApiResponse::default();

    match value {
        Value::Table(table) => {
            // Check for status
            if let Ok(status) = table.get::<u16>("status") {
                response.status = status;
            }

            // Check for body
            if let Ok(body) = table.get::<Value>("body") {
                response.body = lua_to_json(lua, &body)?;
            } else {
                // If no body key, the whole table is the body
                response.body = table_to_json(lua, &table)?;
            }

            // Check for headers
            if let Ok(headers) = table.get::<Table>("headers") {
                for (k, v) in headers.pairs::<String, String>().flatten() {
                    response.headers.insert(k, v);
                }
            }
        }
        Value::Nil => {
            response.status = 204; // No Content
        }
        _ => {
            response.body = lua_to_json(lua, &value)?;
        }
    }

    Ok(response)
}

/// Convert a Lua table to JSON
fn table_to_json(lua: &Lua, table: &Table) -> LuaResult<JsonValue> {
    let mut map = serde_json::Map::new();

    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key_str = match key {
            Value::String(s) => s.to_str()?.to_string(),
            Value::Integer(i) => i.to_string(),
            _ => continue,
        };

        // Skip special keys
        if key_str == "redirect" || key_str == "status" {
            continue;
        }

        map.insert(key_str, lua_to_json(lua, &value)?);
    }

    Ok(JsonValue::Object(map))
}

/// Convert a Lua value to JSON
fn lua_to_json(_lua: &Lua, value: &Value) -> LuaResult<JsonValue> {
    Ok(match value {
        Value::Nil => JsonValue::Null,
        Value::Boolean(b) => JsonValue::Bool(*b),
        Value::Integer(i) => JsonValue::Number((*i).into()),
        Value::Number(n) => {
            serde_json::Number::from_f64(*n)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)
        }
        Value::String(s) => JsonValue::String(s.to_str()?.to_string()),
        Value::Table(t) => {
            // Check if it's an array (sequential integer keys starting at 1)
            let mut is_array = true;
            let mut max_index = 0i64;

            for pair in t.clone().pairs::<Value, Value>() {
                let (key, _) = pair?;
                match key {
                    Value::Integer(i) if i > 0 => {
                        max_index = max_index.max(i);
                    }
                    _ => {
                        is_array = false;
                        break;
                    }
                }
            }

            if is_array && max_index > 0 {
                let mut arr = Vec::with_capacity(max_index as usize);
                for i in 1..=max_index {
                    let v: Value = t.get(i)?;
                    arr.push(lua_to_json(_lua, &v)?);
                }
                JsonValue::Array(arr)
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.clone().pairs::<Value, Value>() {
                    let (key, val) = pair?;
                    let key_str = match key {
                        Value::String(s) => s.to_str()?.to_string(),
                        Value::Integer(i) => i.to_string(),
                        _ => continue,
                    };
                    map.insert(key_str, lua_to_json(_lua, &val)?);
                }
                JsonValue::Object(map)
            }
        }
        // Functions, userdata, etc. become null
        _ => JsonValue::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_context_new() {
        let ctx = LoadContext::new(
            "/blog/hello".to_string(),
            Method::GET,
            vec![("slug".to_string(), "hello".to_string())],
        );

        assert_eq!(ctx.url, "/blog/hello");
        assert_eq!(ctx.method, Method::GET);
        assert_eq!(ctx.params.get("slug"), Some(&"hello".to_string()));
    }

    #[test]
    fn test_lua_to_json() {
        let lua = Lua::new();

        // Test basic types
        let nil_json = lua_to_json(&lua, &Value::Nil).unwrap();
        assert_eq!(nil_json, JsonValue::Null);

        let bool_json = lua_to_json(&lua, &Value::Boolean(true)).unwrap();
        assert_eq!(bool_json, JsonValue::Bool(true));

        let int_json = lua_to_json(&lua, &Value::Integer(42)).unwrap();
        assert_eq!(int_json, JsonValue::Number(42.into()));
    }
}
