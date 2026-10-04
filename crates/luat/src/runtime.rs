// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Runtime execution for server-side Lua code.
//!
//! This module provides the execution layer for:
//! - Load functions (+page.server.lua, +layout.server.lua)
//! - API handlers (+server.lua)
//!
//! All execution uses the Engine's Lua instance, ensuring all modules
//! (json, KV, etc.) are available.

use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use crate::body::parse_structured_body;
use crate::request::LuatRequest;

/// Result of running a load function.
#[derive(Debug, Clone)]
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

/// Result of running an API handler.
#[derive(Debug, Clone)]
pub struct ApiResult {
    /// HTTP status code
    pub status: u16,

    /// Response body (sent as JSON unless `raw` is set)
    pub body: JsonValue,

    /// Raw body, when the handler returned a string `body`. Sent as-is with
    /// the handler's `content-type` (text/plain if it set none).
    pub raw: Option<Vec<u8>>,

    /// Response headers
    pub headers: crate::response::Headers,
}

impl Default for ApiResult {
    fn default() -> Self {
        Self {
            status: 200,
            body: JsonValue::Null,
            raw: None,
            headers: crate::response::Headers::new(),
        }
    }
}

impl ApiResult {
    /// Creates a method not allowed response.
    pub fn method_not_allowed(method: &str) -> Self {
        Self {
            status: 405,
            body: JsonValue::Object({
                let mut map = serde_json::Map::new();
                map.insert(
                    "error".to_string(),
                    JsonValue::String(format!("Method {} not allowed", method)),
                );
                map
            }),
            raw: None,
            headers: crate::response::Headers::new(),
        }
    }
}

/// Runtime executor for Lua code.
///
/// This struct provides methods to execute server-side Lua code
/// using a shared Lua instance.
pub struct Runtime<'lua> {
    lua: &'lua Lua,
    /// Per-request state (`context_stack`, `page_context`) that the `ctx`
    /// helpers read and write. Owned by one request, never shared through
    /// globals or the registry, so interleaved requests stay separate.
    request_runtime: Option<Table>,
    /// Collects cookies set with `ctx.setCookie` / `ctx.deleteCookie`.
    cookies: crate::ctx_helpers::CookieJar,
}

impl<'lua> Runtime<'lua> {
    /// Creates a new runtime with the given Lua instance.
    ///
    /// Without a request runtime, `ctx.setContext` and friends are no-ops.
    pub fn new(lua: &'lua Lua) -> Self {
        Self {
            lua,
            request_runtime: None,
            cookies: Default::default(),
        }
    }

    /// Creates a runtime whose `ctx` helpers operate on `request_runtime`.
    pub fn with_request_runtime(lua: &'lua Lua, request_runtime: Table) -> Self {
        Self {
            lua,
            request_runtime: Some(request_runtime),
            cookies: Default::default(),
        }
    }

    /// Collects cookies set by handlers into `jar`.
    pub fn with_cookies(mut self, jar: crate::ctx_helpers::CookieJar) -> Self {
        self.cookies = jar;
        self
    }

    /// Runs a load function from Lua source code.
    ///
    /// # Arguments
    ///
    /// * `source` - The Lua source code
    /// * `name` - Name for error reporting
    /// * `request` - The request context
    /// * `params` - URL parameters extracted from route matching
    ///
    /// # Returns
    ///
    /// A `LoadResult` containing props, optional redirect, and status.
    pub fn run_load(
        &self,
        source: &str,
        name: &str,
        request: &LuatRequest,
        params: &HashMap<String, String>,
    ) -> LuaResult<LoadResult> {
        let Some(load_fn) = self.prepare_handler(source, name, "load")? else {
            return Ok(LoadResult::default());
        };
        let ctx_table = self.create_context_table(request, params)?;
        let result: Value = load_fn.call(ctx_table)?;
        self.parse_load_result(result)
    }

    /// Async variant of [`run_load`](Self::run_load).
    ///
    /// The load function runs as a coroutine, so it may call async host
    /// functions registered with `Lua::create_async_function`.
    #[cfg(feature = "async-lua")]
    pub async fn run_load_async(
        &self,
        source: &str,
        name: &str,
        request: &LuatRequest,
        params: &HashMap<String, String>,
    ) -> LuaResult<LoadResult> {
        let Some(load_fn) = self.prepare_handler(source, name, "load")? else {
            return Ok(LoadResult::default());
        };
        let ctx_table = self.create_context_table(request, params)?;
        let result: Value = load_fn.call_async(ctx_table).await?;
        self.parse_load_result(result)
    }

    /// Runs an API handler (GET, POST, etc.) from Lua source code.
    ///
    /// # Arguments
    ///
    /// * `source` - The Lua source code
    /// * `name` - Name for error reporting
    /// * `request` - The request context
    /// * `params` - URL parameters extracted from route matching
    ///
    /// # Returns
    ///
    /// An `ApiResult` containing status, body, and headers.
    pub fn run_api(
        &self,
        source: &str,
        name: &str,
        request: &LuatRequest,
        params: &HashMap<String, String>,
    ) -> LuaResult<ApiResult> {
        let Some(handler_fn) = self.prepare_handler(source, name, &request.method)? else {
            return Ok(ApiResult::method_not_allowed(&request.method));
        };
        let ctx_table = self.create_context_table(request, params)?;
        let result: Value = handler_fn.call(ctx_table)?;
        self.parse_api_result(result)
    }

    /// Async variant of [`run_api`](Self::run_api).
    #[cfg(feature = "async-lua")]
    pub async fn run_api_async(
        &self,
        source: &str,
        name: &str,
        request: &LuatRequest,
        params: &HashMap<String, String>,
    ) -> LuaResult<ApiResult> {
        let Some(handler_fn) = self.prepare_handler(source, name, &request.method)? else {
            return Ok(ApiResult::method_not_allowed(&request.method));
        };
        let ctx_table = self.create_context_table(request, params)?;
        let result: Value = handler_fn.call_async(ctx_table).await?;
        self.parse_api_result(result)
    }

    /// Executes `source` in a fresh environment that inherits from globals and
    /// returns the function it defined under `fn_name`, if any.
    ///
    /// Using a per-call environment keeps user definitions (`load`, `GET`, …)
    /// out of `_G`, so one route's handlers never leak into another's.
    fn prepare_handler(&self, source: &str, name: &str, fn_name: &str) -> LuaResult<Option<Function>> {
        let env = crate::scoped_require::handler_env(self.lua, name)?;
        self.lua
            .load(source)
            .set_name(name)
            .set_environment(env.clone())
            .exec()?;

        env.raw_get::<Option<Function>>(fn_name)
    }

    /// Creates a Lua context table from a request.
    fn create_context_table(
        &self,
        request: &LuatRequest,
        params: &HashMap<String, String>,
    ) -> LuaResult<Table> {
        let ctx = self.lua.create_table()?;

        // Add params
        let params_table = self.lua.create_table()?;
        for (key, value) in params {
            params_table.set(key.as_str(), value.as_str())?;
        }
        ctx.set("params", params_table)?;

        // Add URL and method
        ctx.set("url", request.path.as_str())?;
        ctx.set("method", request.method.as_str())?;

        // Add query params
        let query_table = self.lua.create_table()?;
        for (key, value) in &request.query {
            query_table.set(key.as_str(), value.as_str())?;
        }
        ctx.set("query", query_table)?;

        // Add headers
        let headers_table = self.lua.create_table()?;
        for (key, value) in &request.headers {
            headers_table.set(key.as_str(), value.as_str())?;
        }
        ctx.set("headers", headers_table)?;

        // Add cookies (explicit, or parsed from the Cookie header)
        let cookies_table = self.lua.create_table()?;
        for (key, value) in request.cookie_map() {
            cookies_table.set(key, value)?;
        }
        ctx.set("cookies", cookies_table)?;
        crate::ctx_helpers::install(self.lua, &ctx, &self.cookies)?;

        // Add body/form/json
        if let Some(body) = &request.body {
            if let Some(json) = parse_structured_body(body, request.content_type()) {
                let body_value = if json.is_null() {
                    Value::Table(self.lua.create_table()?)
                } else {
                    self.json_to_lua(&json)?
                };
                ctx.set("body", body_value.clone())?;
                ctx.set("form", body_value.clone())?;
                ctx.set("json", body_value)?;
            } else if let Ok(body_str) = std::str::from_utf8(body) {
                ctx.set("body", body_str)?;
            }
        } else {
            let empty = self.lua.create_table()?;
            ctx.set("body", empty.clone())?;
            ctx.set("form", empty.clone())?;
            ctx.set("json", empty)?;
        }

        // setContext/getContext and setPageContext/getPageContext operate on
        // this request's own runtime table.
        let rt = self.request_runtime.clone();
        let set_context = self.lua.create_function(move |lua, (key, value): (String, Value)| {
            let Some(stack) = rt.as_ref().and_then(|r| r.get::<Table>("context_stack").ok()) else {
                return Ok(());
            };
            let len = stack.len().unwrap_or(0);
            let scope = if len == 0 {
                let new_scope = lua.create_table()?;
                stack.push(new_scope.clone())?;
                new_scope
            } else {
                stack.get::<Table>(len)?
            };
            scope.set(key, value)
        })?;

        let rt = self.request_runtime.clone();
        let get_context = self.lua.create_function(move |_, key: String| {
            let Some(stack) = rt.as_ref().and_then(|r| r.get::<Table>("context_stack").ok()) else {
                return Ok(Value::Nil);
            };
            for i in (1..=stack.len().unwrap_or(0)).rev() {
                if let Ok(scope) = stack.get::<Table>(i) {
                    let val: Value = scope.get(key.as_str())?;
                    if !val.is_nil() {
                        return Ok(val);
                    }
                }
            }
            Ok(Value::Nil)
        })?;

        ctx.set("setContext", set_context)?;
        ctx.set("getContext", get_context)?;

        let rt = self.request_runtime.clone();
        let set_page_context = self.lua.create_function(move |_, (key, value): (String, Value)| {
            match rt.as_ref().and_then(|r| r.get::<Table>("page_context").ok()) {
                Some(page_ctx) => page_ctx.set(key, value),
                None => Ok(()),
            }
        })?;

        let rt = self.request_runtime.clone();
        let get_page_context = self.lua.create_function(move |_, key: String| {
            match rt.as_ref().and_then(|r| r.get::<Table>("page_context").ok()) {
                Some(page_ctx) => page_ctx.get::<Value>(key),
                None => Ok(Value::Nil),
            }
        })?;

        ctx.set("setPageContext", set_page_context)?;
        ctx.set("getPageContext", get_page_context)?;

        Ok(ctx)
    }

    /// Parses a Lua value into LoadResult.
    fn parse_load_result(&self, value: Value) -> LuaResult<LoadResult> {
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

                // Convert to JSON props (excluding special keys)
                result.props = self.table_to_json_excluding(&table, &["redirect", "status"])?;
            }
            Value::Nil => {
                // Return empty props
            }
            _ => {
                // Wrap non-table values
                result.props = self.lua_to_json(&value)?;
            }
        }

        Ok(result)
    }

    /// Parses a Lua value into ApiResult.
    fn parse_api_result(&self, value: Value) -> LuaResult<ApiResult> {
        let mut result = ApiResult::default();

        match value {
            Value::Table(table) => {
                // Check for status
                if let Ok(status) = table.get::<u16>("status") {
                    result.status = status;
                }

                // Check for body: a string is sent raw, anything else as JSON
                match table.get::<Value>("body")? {
                    Value::String(s) => result.raw = Some(s.as_bytes().to_vec()),
                    Value::Nil => {
                        // If no body key, the whole table is the body
                        result.body = self.table_to_json_excluding(
                            &table,
                            &["status", "headers", "redirect"],
                        )?;
                    }
                    body => result.body = self.lua_to_json(&body)?,
                }

                // Check for headers; a list value sends the header repeatedly
                if let Ok(headers) = table.get::<Table>("headers") {
                    for (k, v) in headers.pairs::<String, Value>().flatten() {
                        match v {
                            Value::Table(values) => {
                                for v in values.sequence_values::<String>().flatten() {
                                    result.headers.append(k.clone(), v);
                                }
                            }
                            other => {
                                if let Some(v) = other.as_string_lossy() {
                                    result.headers.insert(k, v);
                                }
                            }
                        }
                    }
                }

                // Check for redirect shorthand
                if let Ok(redirect) = table.get::<String>("redirect") {
                    result.headers.insert("Location", redirect);
                    if result.status == 200 {
                        result.status = 302;
                    }
                }
            }
            Value::Nil => {
                result.status = 204; // No Content
            }
            _ => {
                result.body = self.lua_to_json(&value)?;
            }
        }

        Ok(result)
    }

    /// Converts a JSON value to a Lua value.
    fn json_to_lua(&self, json: &JsonValue) -> LuaResult<Value> {
        Ok(match json {
            JsonValue::Null => Value::Nil,
            JsonValue::Bool(b) => Value::Boolean(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Number(f)
                } else {
                    Value::Nil
                }
            }
            JsonValue::String(s) => Value::String(self.lua.create_string(s)?),
            JsonValue::Array(arr) => {
                let table = self.lua.create_table()?;
                for (i, v) in arr.iter().enumerate() {
                    table.set(i + 1, self.json_to_lua(v)?)?;
                }
                Value::Table(table)
            }
            JsonValue::Object(obj) => {
                let table = self.lua.create_table()?;
                for (k, v) in obj {
                    table.set(k.as_str(), self.json_to_lua(v)?)?;
                }
                Value::Table(table)
            }
        })
    }

    /// Converts a Lua value to JSON.
    #[allow(clippy::only_used_in_recursion)]
    fn lua_to_json(&self, value: &Value) -> LuaResult<JsonValue> {
        Ok(match value {
            Value::Nil => JsonValue::Null,
            Value::Boolean(b) => JsonValue::Bool(*b),
            Value::Integer(i) => JsonValue::Number((*i).into()),
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
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
                        arr.push(self.lua_to_json(&v)?);
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
                        map.insert(key_str, self.lua_to_json(&val)?);
                    }
                    JsonValue::Object(map)
                }
            }
            // Functions, userdata, etc. become null
            _ => JsonValue::Null,
        })
    }

    /// Converts a Lua table to JSON, excluding specified keys.
    fn table_to_json_excluding(&self, table: &Table, exclude: &[&str]) -> LuaResult<JsonValue> {
        let mut map = serde_json::Map::new();

        for pair in table.clone().pairs::<Value, Value>() {
            let (key, value) = pair?;
            let key_str = match key {
                Value::String(s) => s.to_str()?.to_string(),
                Value::Integer(i) => i.to_string(),
                _ => continue,
            };

            // Skip excluded keys
            if exclude.contains(&key_str.as_str()) {
                continue;
            }

            map.insert(key_str, self.lua_to_json(&value)?);
        }

        Ok(JsonValue::Object(map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn test_run_load_basic() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = r#"
            function load(ctx)
                return { message = "Hello", count = 42 }
            end
        "#;

        let request = LuatRequest::new("/test", "GET");
        let params = HashMap::new();

        let result = runtime.run_load(source, "test", &request, &params).unwrap();

        assert_eq!(result.props["message"], "Hello");
        assert_eq!(result.props["count"], 42);
        assert!(result.redirect.is_none());
    }

    #[test]
    fn test_run_load_with_params() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = r#"
            function load(ctx)
                return { slug = ctx.params.slug }
            end
        "#;

        let request = LuatRequest::new("/blog/hello", "GET");
        let mut params = HashMap::new();
        params.insert("slug".to_string(), "hello".to_string());

        let result = runtime.run_load(source, "test", &request, &params).unwrap();

        assert_eq!(result.props["slug"], "hello");
    }

    #[test]
    fn test_run_load_with_redirect() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = r#"
            function load(ctx)
                return { redirect = "/login" }
            end
        "#;

        let request = LuatRequest::new("/protected", "GET");
        let params = HashMap::new();

        let result = runtime.run_load(source, "test", &request, &params).unwrap();

        assert_eq!(result.redirect, Some("/login".to_string()));
    }

    #[test]
    fn test_run_load_no_function() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = "-- no load function defined";

        let request = LuatRequest::new("/test", "GET");
        let params = HashMap::new();

        let result = runtime.run_load(source, "test", &request, &params).unwrap();

        assert!(result.props.is_object());
        assert!(result.props.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_run_api_get() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = r#"
            function GET(ctx)
                return { status = 200, body = { items = {"a", "b", "c"} } }
            end
        "#;

        let request = LuatRequest::new("/api/items", "GET");
        let params = HashMap::new();

        let result = runtime.run_api(source, "test", &request, &params).unwrap();

        assert_eq!(result.status, 200);
        assert!(result.body["items"].is_array());
    }

    #[test]
    fn test_run_api_method_not_found() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let source = r#"
            function GET(ctx)
                return { message = "ok" }
            end
        "#;

        let request = LuatRequest::new("/api/items", "POST");
        let params = HashMap::new();

        let result = runtime.run_api(source, "test", &request, &params).unwrap();

        assert_eq!(result.status, 405);
        assert!(result.body["error"].as_str().unwrap().contains("POST"));
    }

    #[test]
    fn test_lua_to_json_array() {
        let lua = Lua::new();
        let runtime = Runtime::new(&lua);

        let table = lua.create_table().unwrap();
        table.set(1, "first").unwrap();
        table.set(2, "second").unwrap();
        table.set(3, "third").unwrap();

        let json = runtime.lua_to_json(&Value::Table(table)).unwrap();

        assert!(json.is_array());
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], "first");
    }
}
