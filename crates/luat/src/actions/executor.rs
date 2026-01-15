// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Action executor for running Lua action handlers.

use super::{ActionContext, ActionResponse};
use mlua::{Function, Lua, Result as LuaResult, Table, Value};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Executes form actions from Lua server files.
///
/// The ActionExecutor handles:
/// - Loading and executing `+page.server.lua` files
/// - Finding the appropriate action handler (default or named)
/// - Converting ActionContext to Lua tables
/// - Parsing Lua responses into ActionResponse
///
/// # Example
///
/// ```rust,ignore
/// use luat::actions::{ActionExecutor, ActionContext};
///
/// let lua = mlua::Lua::new();
/// let executor = ActionExecutor::new(&lua);
///
/// let source = r#"
/// actions = {
///     default = function(ctx)
///         return { success = true, message = "Hello " .. (ctx.form.name or "World") }
///     end
/// }
/// "#;
///
/// let ctx = ActionContext::new("POST", "/api/greet")
///     .with_body(serde_json::json!({ "name": "Alice" }));
///
/// let response = executor.execute(source, "test/+page.server.lua", &ctx)?;
/// assert_eq!(response.status, 200);
/// ```
pub struct ActionExecutor<'lua> {
    lua: &'lua Lua,
}

impl<'lua> ActionExecutor<'lua> {
    /// Creates a new ActionExecutor with the given Lua instance.
    pub fn new(lua: &'lua Lua) -> Self {
        Self { lua }
    }

    /// Executes an action from the given Lua source code.
    ///
    /// # Arguments
    ///
    /// * `source` - The Lua source code (typically from `+page.server.lua`)
    /// * `path` - The file path for require() resolution and error reporting
    /// * `ctx` - The action context containing request data
    ///
    /// # Returns
    ///
    /// An `ActionResponse` containing the result of the action.
    pub fn execute(&self, source: &str, path: &str, ctx: &ActionContext) -> LuaResult<ActionResponse> {
        // Set current module path so require() can resolve relative paths
        // This enables the resolver searcher in engine.rs to find modules
        self.lua.set_named_registry_value("__luat_current_module", path)?;
        let globals = self.lua.globals();
        let _ = globals.set("__luat_current_module", path);

        // Register the fail() helper function
        self.register_fail_helper()?;

        // Load and execute the server file with proper chunk name for error reporting
        self.lua.load(source).set_name(path).exec()?;

        // Find the appropriate handler
        let handler = self.find_handler(ctx)?;

        // Create context table for Lua
        let ctx_table = self.context_to_lua(ctx)?;

        // Call the handler
        let result: Value = handler.call(ctx_table)?;

        // Parse the response
        self.parse_response(result)
    }

    /// Registers the `fail()` helper function in Lua globals.
    ///
    /// The fail function creates an error response with status and data:
    /// ```lua
    /// return fail(400, { error = "Validation failed" })
    /// ```
    fn register_fail_helper(&self) -> LuaResult<()> {
        let fail_fn = self.lua.create_function(|lua, (status, data): (u16, Value)| {
            let result = lua.create_table()?;
            result.set("__fail", true)?;
            result.set("status", status)?;

            // Merge data into result if it's a table
            if let Value::Table(data_table) = data {
                for pair in data_table.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    result.set(key, value)?;
                }
            }

            Ok(result)
        })?;

        self.lua.globals().set("fail", fail_fn)?;
        Ok(())
    }

    /// Finds the appropriate handler function based on the action context.
    ///
    /// Handler resolution order:
    /// 1. Named action with method: `actions.{name}.{method}` (e.g., `actions.update.post`)
    /// 2. Named action function: `actions.{name}` (e.g., `actions.login`)
    /// 3. Default action with method: `actions.default.{method}`
    /// 4. Default action function: `actions.default`
    fn find_handler(&self, ctx: &ActionContext) -> LuaResult<Function> {
        let globals = self.lua.globals();
        let method = ctx.method.to_lowercase();
        let action_name = ctx.effective_action_name();

        // Get the actions table
        let actions_table: Table = globals.get("actions").map_err(|_| {
            mlua::Error::runtime("No 'actions' table found in server file")
        })?;

        // Try to get the action entry
        let action_entry: Value = actions_table.get(action_name).map_err(|_| {
            mlua::Error::runtime(format!("Action '{}' not found", action_name))
        })?;

        match action_entry {
            // Action is a function - use it directly
            Value::Function(f) => Ok(f),

            // Action is a table - look for method-specific handler
            Value::Table(method_table) => {
                let handler: Function = method_table.get(method.as_str()).map_err(|_| {
                    mlua::Error::runtime(format!(
                        "No '{}' handler found for action '{}'",
                        method, action_name
                    ))
                })?;
                Ok(handler)
            }

            _ => Err(mlua::Error::runtime(format!(
                "Action '{}' is not a function or table",
                action_name
            ))),
        }
    }

    /// Converts ActionContext to a Lua table.
    fn context_to_lua(&self, ctx: &ActionContext) -> LuaResult<Table> {
        let table = self.lua.create_table()?;

        // Add params
        let params_table = self.lua.create_table()?;
        for (k, v) in &ctx.params {
            params_table.set(k.as_str(), v.as_str())?;
        }
        table.set("params", params_table)?;

        // Add query
        let query_table = self.lua.create_table()?;
        for (k, v) in &ctx.query {
            query_table.set(k.as_str(), v.as_str())?;
        }
        table.set("query", query_table)?;

        // Add headers
        let headers_table = self.lua.create_table()?;
        for (k, v) in &ctx.headers {
            headers_table.set(k.as_str(), v.as_str())?;
        }
        table.set("headers", headers_table)?;

        // Add body/form/json
        // If body is null, use an empty table so ctx.form.key doesn't error
        let body_value = if ctx.body.is_null() {
            Value::Table(self.lua.create_table()?)
        } else {
            self.json_to_lua(&ctx.body)?
        };
        table.set("body", body_value.clone())?;
        table.set("form", body_value.clone())?; // Alias for form data
        table.set("json", body_value)?; // Alias for JSON body

        // Add cookies
        let cookies_table = self.lua.create_table()?;
        for (k, v) in &ctx.cookies {
            cookies_table.set(k.as_str(), v.as_str())?;
        }
        table.set("cookies", cookies_table)?;

        // Add metadata
        table.set("url", ctx.url.as_str())?;
        table.set("method", ctx.method.as_str())?;

        Ok(table)
    }

    /// Parses a Lua value into an ActionResponse.
    fn parse_response(&self, value: Value) -> LuaResult<ActionResponse> {
        match value {
            Value::Table(table) => {
                let mut response = ActionResponse {
                    status: 200,
                    headers: HashMap::new(),
                    data: JsonValue::Null,
                };

                // Check if this is a fail() result
                let is_fail: bool = table.get("__fail").unwrap_or(false);

                // Extract status
                if let Ok(status) = table.get::<u16>("status") {
                    response.status = status;
                } else if is_fail {
                    response.status = 400; // Default fail status
                }

                // Extract headers
                if let Ok(headers) = table.get::<Table>("headers") {
                    for (k, v) in headers.pairs::<String, String>().flatten() {
                        response.headers.insert(k, v);
                    }
                }

                // Check for shorthand redirect (server-side HTTP 302)
                if let Ok(redirect) = table.get::<String>("redirect") {
                    response.headers.insert("Location".to_string(), redirect);
                    if response.status == 200 {
                        response.status = 302;
                    }
                }

                // Extract body or convert entire table to data
                // Note: In Lua, accessing a non-existent key returns nil, so we need to check
                // if the value is not nil to determine if "body" key was explicitly set.
                let body_value: Value = table.get("body").unwrap_or(Value::Nil);
                if !matches!(body_value, Value::Nil) {
                    response.data = self.lua_to_json(&body_value)?;
                } else {
                    // No explicit "body" key - convert entire table (excluding special keys) to data
                    response.data = self.table_to_json_excluding(
                        &table,
                        &["__fail", "status", "headers", "body", "redirect"],
                    )?;
                }

                Ok(response)
            }

            Value::Nil => Ok(ActionResponse {
                status: 204, // No Content
                headers: HashMap::new(),
                data: JsonValue::Null,
            }),

            _ => Ok(ActionResponse {
                status: 200,
                headers: HashMap::new(),
                data: self.lua_to_json(&value)?,
            }),
        }
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

    #[test]
    fn test_execute_default_action() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    return { success = true, message = "Hello" }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/test");
        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.data["success"], true);
        assert_eq!(response.data["message"], "Hello");
    }

    #[test]
    fn test_execute_named_action() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                login = function(ctx)
                    return { logged_in = true, user = ctx.form.email }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/auth?/login")
            .with_action(Some("login".to_string()))
            .with_body(serde_json::json!({ "email": "test@example.com" }));

        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.data["logged_in"], true);
        assert_eq!(response.data["user"], "test@example.com");
    }

    #[test]
    fn test_execute_method_specific_action() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                update = {
                    post = function(ctx)
                        return { method = "post" }
                    end,
                    put = function(ctx)
                        return { method = "put" }
                    end
                }
            }
        "#;

        // Test POST
        let ctx = ActionContext::new("POST", "/test?/update")
            .with_action(Some("update".to_string()));
        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();
        assert_eq!(response.data["method"], "post");

        // Test PUT (need fresh Lua instance due to global state)
        let lua2 = Lua::new();
        let executor2 = ActionExecutor::new(&lua2);
        let ctx2 = ActionContext::new("PUT", "/test?/update")
            .with_action(Some("update".to_string()));
        let response2 = executor2.execute(source, "test/+page.server.lua", &ctx2).unwrap();
        assert_eq!(response2.data["method"], "put");
    }

    #[test]
    fn test_fail_helper() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    if not ctx.form.email then
                        return fail(400, { error = "Email required" })
                    end
                    return { success = true }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/test");
        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(response.status, 400);
        assert_eq!(response.data["error"], "Email required");
    }

    #[test]
    fn test_server_redirect() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    return {
                        redirect = "/dashboard"
                    }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/test");
        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(response.status, 302);
        assert_eq!(
            response.headers.get("Location"),
            Some(&"/dashboard".to_string())
        );
    }

    #[test]
    fn test_custom_headers() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    return {
                        headers = {
                            ["HX-Trigger"] = "showMessage",
                            ["X-Custom"] = "value"
                        },
                        success = true
                    }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/test");
        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(
            response.headers.get("HX-Trigger"),
            Some(&"showMessage".to_string())
        );
        assert_eq!(
            response.headers.get("X-Custom"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_action_not_found() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    return { success = true }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/test?/nonexistent")
            .with_action(Some("nonexistent".to_string()));

        let result = executor.execute(source, "test/+page.server.lua", &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_context_params_available() {
        let lua = Lua::new();
        let executor = ActionExecutor::new(&lua);

        let source = r#"
            actions = {
                default = function(ctx)
                    return {
                        slug = ctx.params.slug,
                        method = ctx.method,
                        url = ctx.url
                    }
                end
            }
        "#;

        let ctx = ActionContext::new("POST", "/blog/hello/edit")
            .with_params([("slug".to_string(), "hello".to_string())].into());

        let response = executor.execute(source, "test/+page.server.lua", &ctx).unwrap();

        assert_eq!(response.data["slug"], "hello");
        assert_eq!(response.data["method"], "POST");
        assert_eq!(response.data["url"], "/blog/hello/edit");
    }
}
