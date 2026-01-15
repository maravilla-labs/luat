// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! WebAssembly bindings for Luat
//!
//! This module provides JavaScript-friendly bindings for using Luat in the browser
//! and other WASM environments like edge runtimes and serverless platforms.
//!
//! # Example (JavaScript)
//!
//! ```javascript
//! import { WasmEngine } from 'luat';
//!
//! const engine = new WasmEngine();
//! engine.addTemplate('hello.luat', '<h1>Hello, {props.name}!</h1>');
//!
//! const html = engine.render('hello.luat', { name: 'World' });
//! console.log(html); // <h1>Hello, World!</h1>
//! ```

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use crate::actions::{ActionContext, ActionExecutor};
use crate::cache::MemoryCache;
use crate::engine::Engine;
use crate::kv::{register_kv_module, KVStoreFactory, MemoryKVStore};
use crate::memory_resolver::MemoryResourceResolver;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A WASM-compatible Luat template engine
///
/// This is the main entry point for using Luat in WebAssembly environments.
/// It uses in-memory template storage and caching.
#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine<MemoryResourceResolver>,
    resolver: MemoryResourceResolver,
    /// In-memory KV stores by namespace (persists across Lua executions)
    kv_stores: Arc<RwLock<HashMap<String, Arc<MemoryKVStore>>>>,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Create a new WASM engine with the specified cache size
    #[wasm_bindgen(constructor)]
    pub fn new(cache_size: Option<usize>) -> Result<WasmEngine, JsValue> {
        let cache_size = cache_size.unwrap_or(100);
        let resolver = MemoryResourceResolver::new();
        let cache = Box::new(MemoryCache::new(cache_size));

        let engine = Engine::new(resolver.clone(), cache)
            .map_err(|e| JsValue::from_str(&format!("Failed to create engine: {}", e)))?;

        Ok(WasmEngine {
            engine,
            resolver,
            kv_stores: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a KV store factory for use with Lua instances
    fn create_kv_factory(&self) -> KVStoreFactory {
        let stores = self.kv_stores.clone();
        Arc::new(move |namespace: &str| -> Arc<dyn crate::kv::KVStore> {
            let mut stores_guard = stores.write().unwrap();
            if let Some(store) = stores_guard.get(namespace) {
                store.clone()
            } else {
                let store = Arc::new(MemoryKVStore::new());
                stores_guard.insert(namespace.to_string(), store.clone());
                store
            }
        })
    }

    /// Register KV module in a Lua instance
    fn setup_kv(&self, lua: &mlua::Lua) -> Result<(), JsValue> {
        let factory = self.create_kv_factory();
        register_kv_module(lua, factory)
            .map_err(|e| JsValue::from_str(&format!("Failed to register KV module: {}", e)))
    }

    /// Add a template to the engine
    ///
    /// # Arguments
    /// * `path` - The template path/name (e.g., "components/Button.luat")
    /// * `source` - The template source code
    #[wasm_bindgen(js_name = addTemplate)]
    pub fn add_template(&self, path: &str, source: &str) {
        self.resolver.add_template(path, source.to_string());
    }

    /// Remove a template from the engine
    #[wasm_bindgen(js_name = removeTemplate)]
    pub fn remove_template(&self, path: &str) {
        self.resolver.remove_template(path);
    }

    /// Clear all templates
    #[wasm_bindgen(js_name = clearTemplates)]
    pub fn clear_templates(&self) {
        self.resolver.clear();
    }

    /// Compile a template and return the compiled module as a string
    ///
    /// This is useful for pre-compilation or debugging.
    #[wasm_bindgen]
    pub fn compile(&self, entry: &str) -> Result<String, JsValue> {
        let module = self.engine.compile_entry(entry)
            .map_err(|e| JsValue::from_str(&format!("Compilation error: {}", e)))?;

        Ok(module.lua_code.clone())
    }

    /// Render a template with the given context
    ///
    /// # Arguments
    /// * `entry` - The entry template path
    /// * `context` - A JavaScript object to use as the template context
    ///
    /// # Returns
    /// The rendered HTML string
    #[wasm_bindgen]
    pub fn render(&self, entry: &str, context: JsValue) -> Result<String, JsValue> {
        // Compile the template
        let module = self.engine.compile_entry(entry)
            .map_err(|e| JsValue::from_str(&format!("Compilation error: {}", e)))?;

        // Convert JS value to Lua value
        let lua_context = self.js_to_lua_value(&context)?;

        // Render
        let html = self.engine.render(&module, &lua_context)
            .map_err(|e| JsValue::from_str(&format!("Render error: {}", e)))?;

        Ok(html)
    }

    /// Render a template synchronously (alias for render)
    #[wasm_bindgen(js_name = renderSync)]
    pub fn render_sync(&self, entry: &str, context: JsValue) -> Result<String, JsValue> {
        self.render(entry, context)
    }

    /// Parse a template and return the AST as JSON (for debugging)
    #[wasm_bindgen(js_name = parseToJson)]
    pub fn parse_to_json(&self, source: &str) -> Result<String, JsValue> {
        use crate::parser::parse_template;

        let ast = parse_template(source)
            .map_err(|e| JsValue::from_str(&format!("Parse error: {}", e)))?;

        serde_json::to_string_pretty(&ast)
            .map_err(|e| JsValue::from_str(&format!("JSON serialization error: {}", e)))
    }

    /// Execute a form action from a +page.server.lua file
    ///
    /// # Arguments
    /// * `source` - The Lua source code (from +page.server.lua)
    /// * `context` - A JavaScript object with action context:
    ///   - `method`: HTTP method (POST, PUT, DELETE)
    ///   - `url`: Request URL
    ///   - `params`: Route parameters
    ///   - `query`: Query parameters
    ///   - `headers`: Request headers
    ///   - `body`: Request body (form data or JSON)
    ///   - `action_name`: Optional named action (e.g., "login", "publish")
    ///   - `cookies`: Request cookies
    ///
    /// # Returns
    /// A JavaScript object with:
    ///   - `status`: HTTP status code
    ///   - `headers`: Response headers
    ///   - `data`: Response data
    ///
    /// # Example (JavaScript)
    /// ```javascript
    /// const response = engine.executeAction(serverSource, {
    ///     method: "POST",
    ///     url: "/blog/myid/edit?/login",
    ///     params: { slug: "myid" },
    ///     body: { email: "user@example.com" },
    ///     action_name: "login"
    /// });
    /// // Returns: { status: 200, headers: {...}, data: { success: true } }
    /// ```
    #[wasm_bindgen(js_name = executeAction)]
    pub fn execute_action(&self, source: &str, context: JsValue) -> Result<JsValue, JsValue> {
        // Parse context from JS
        let ctx = self.parse_action_context(&context)?;

        // Create a new Lua state for each action execution
        let lua = mlua::Lua::new();

        // Register KV module
        self.setup_kv(&lua)?;

        let executor = ActionExecutor::new(&lua);

        // Execute the action
        let response = executor
            .execute(source, ctx.url.as_str(), &ctx)
            .map_err(|e| JsValue::from_str(&format!("Action error: {}", e)))?;

        // Convert response to JS object
        self.action_response_to_js(&response)
    }

    /// Execute an API handler from a +server.lua file
    ///
    /// # Arguments
    /// * `source` - The Lua source code (from +server.lua)
    /// * `context` - A JavaScript object with request context:
    ///   - `method`: HTTP method (GET, POST, PUT, DELETE)
    ///   - `url`: Request URL
    ///   - `params`: Route parameters
    ///   - `query`: Query parameters
    ///   - `headers`: Request headers
    ///   - `body`: Request body (form data or JSON)
    ///   - `cookies`: Request cookies
    ///
    /// # Returns
    /// A JavaScript object with:
    ///   - `status`: HTTP status code
    ///   - `headers`: Response headers
    ///   - `body`: Response body
    ///
    /// # Example (JavaScript)
    /// ```javascript
    /// const response = engine.executeApi(serverSource, {
    ///     method: "GET",
    ///     url: "/api/blog/my-post",
    ///     params: { slug: "my-post" }
    /// });
    /// // Returns: { status: 200, headers: {...}, body: { title: "..." } }
    /// ```
    #[wasm_bindgen(js_name = executeApi)]
    pub fn execute_api(&self, source: &str, context: JsValue) -> Result<JsValue, JsValue> {
        // Parse context from JS
        let ctx = self.parse_action_context(&context)?;

        // Create a new Lua state for each API execution
        let lua = mlua::Lua::new();

        // Register KV module
        self.setup_kv(&lua)?;

        // Register fail() helper
        self.register_fail_helper(&lua)
            .map_err(|e| JsValue::from_str(&format!("Failed to register helpers: {}", e)))?;

        // Load and execute the server file
        lua.load(source)
            .exec()
            .map_err(|e| JsValue::from_str(&format!("Lua error: {}", e)))?;

        // Find and call the appropriate method handler
        let method = ctx.method.to_uppercase();
        let handler: mlua::Function = lua
            .globals()
            .get(method.as_str())
            .map_err(|_| JsValue::from_str(&format!("No {} handler found", method)))?;

        // Create context table for Lua
        let ctx_table = self.context_to_lua(&lua, &ctx)
            .map_err(|e| JsValue::from_str(&format!("Failed to create context: {}", e)))?;

        // Call the handler
        let result: mlua::Value = handler
            .call(ctx_table)
            .map_err(|e| JsValue::from_str(&format!("Handler error: {}", e)))?;

        // Parse and convert response
        let response = self.parse_api_response(&lua, result)
            .map_err(|e| JsValue::from_str(&format!("Response error: {}", e)))?;

        self.api_response_to_js(&response)
    }

    /// Get the version of the Luat library
    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

/// API response structure (for +server.lua handlers)
struct ApiResponse {
    status: u16,
    headers: std::collections::HashMap<String, String>,
    body: serde_json::Value,
}

impl WasmEngine {
    /// Convert a JavaScript value to a Lua-compatible mlua::Value
    fn js_to_lua_value(&self, js_value: &JsValue) -> Result<mlua::Value, JsValue> {
        // Convert JsValue to serde_json::Value first
        let json_value: serde_json::Value = serde_wasm_bindgen::from_value(js_value.clone())
            .map_err(|e| JsValue::from_str(&format!("Failed to convert JS value: {}", e)))?;

        // Then convert to Lua value
        self.engine.to_value(json_value)
            .map_err(|e| JsValue::from_str(&format!("Failed to convert to Lua value: {}", e)))
    }

    /// Parse ActionContext from a JavaScript object
    fn parse_action_context(&self, js_value: &JsValue) -> Result<ActionContext, JsValue> {
        let json: serde_json::Value = serde_wasm_bindgen::from_value(js_value.clone())
            .map_err(|e| JsValue::from_str(&format!("Failed to parse context: {}", e)))?;

        let obj = json.as_object()
            .ok_or_else(|| JsValue::from_str("Context must be an object"))?;

        let method = obj.get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("POST")
            .to_string();

        let url = obj.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("/")
            .to_string();

        let mut ctx = ActionContext::new(&method, &url);

        // Parse params
        if let Some(params) = obj.get("params").and_then(|v| v.as_object()) {
            let params_map: std::collections::HashMap<String, String> = params
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            ctx = ctx.with_params(params_map);
        }

        // Parse query
        if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
            let query_map: std::collections::HashMap<String, String> = query
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            ctx = ctx.with_query(query_map);
        }

        // Parse headers
        if let Some(headers) = obj.get("headers").and_then(|v| v.as_object()) {
            let headers_map: std::collections::HashMap<String, String> = headers
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            ctx = ctx.with_headers(headers_map);
        }

        // Parse body
        if let Some(body) = obj.get("body") {
            ctx = ctx.with_body(body.clone());
        }

        // Parse action_name
        if let Some(action_name) = obj.get("action_name").and_then(|v| v.as_str()) {
            ctx = ctx.with_action(Some(action_name.to_string()));
        }

        // Parse cookies
        if let Some(cookies) = obj.get("cookies").and_then(|v| v.as_object()) {
            let cookies_map: std::collections::HashMap<String, String> = cookies
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            ctx = ctx.with_cookies(cookies_map);
        }

        Ok(ctx)
    }

    /// Convert ActionResponse to a JavaScript object
    fn action_response_to_js(&self, response: &crate::actions::ActionResponse) -> Result<JsValue, JsValue> {
        let obj = serde_json::json!({
            "status": response.status,
            "headers": response.headers,
            "data": response.data
        });

        serde_wasm_bindgen::to_value(&obj)
            .map_err(|e| JsValue::from_str(&format!("Failed to convert response: {}", e)))
    }

    /// Convert ApiResponse to a JavaScript object
    fn api_response_to_js(&self, response: &ApiResponse) -> Result<JsValue, JsValue> {
        let obj = serde_json::json!({
            "status": response.status,
            "headers": response.headers,
            "body": response.body
        });

        serde_wasm_bindgen::to_value(&obj)
            .map_err(|e| JsValue::from_str(&format!("Failed to convert response: {}", e)))
    }

    /// Register the fail() helper in a Lua instance
    fn register_fail_helper(&self, lua: &mlua::Lua) -> mlua::Result<()> {
        let fail_fn = lua.create_function(|lua, (status, data): (u16, mlua::Value)| {
            let result = lua.create_table()?;
            result.set("__fail", true)?;
            result.set("status", status)?;

            if let mlua::Value::Table(data_table) = data {
                for pair in data_table.pairs::<mlua::Value, mlua::Value>() {
                    let (key, value) = pair?;
                    result.set(key, value)?;
                }
            }

            Ok(result)
        })?;

        lua.globals().set("fail", fail_fn)?;
        Ok(())
    }

    /// Convert ActionContext to a Lua table
    fn context_to_lua(&self, lua: &mlua::Lua, ctx: &ActionContext) -> mlua::Result<mlua::Table> {
        let table = lua.create_table()?;

        // Add params
        let params_table = lua.create_table()?;
        for (k, v) in &ctx.params {
            params_table.set(k.as_str(), v.as_str())?;
        }
        table.set("params", params_table)?;

        // Add query
        let query_table = lua.create_table()?;
        for (k, v) in &ctx.query {
            query_table.set(k.as_str(), v.as_str())?;
        }
        table.set("query", query_table)?;

        // Add headers
        let headers_table = lua.create_table()?;
        for (k, v) in &ctx.headers {
            headers_table.set(k.as_str(), v.as_str())?;
        }
        table.set("headers", headers_table)?;

        // Add body/form/json
        let body_value = if ctx.body.is_null() {
            mlua::Value::Table(lua.create_table()?)
        } else {
            self.json_to_lua_value(lua, &ctx.body)?
        };
        table.set("body", body_value.clone())?;
        table.set("form", body_value.clone())?;
        table.set("json", body_value)?;

        // Add cookies
        let cookies_table = lua.create_table()?;
        for (k, v) in &ctx.cookies {
            cookies_table.set(k.as_str(), v.as_str())?;
        }
        table.set("cookies", cookies_table)?;

        // Add metadata
        table.set("url", ctx.url.as_str())?;
        table.set("method", ctx.method.as_str())?;

        Ok(table)
    }

    /// Convert JSON to Lua value
    fn json_to_lua_value(&self, lua: &mlua::Lua, json: &serde_json::Value) -> mlua::Result<mlua::Value> {
        Ok(match json {
            serde_json::Value::Null => mlua::Value::Nil,
            serde_json::Value::Bool(b) => mlua::Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    mlua::Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    mlua::Value::Number(f)
                } else {
                    mlua::Value::Nil
                }
            }
            serde_json::Value::String(s) => mlua::Value::String(lua.create_string(s)?),
            serde_json::Value::Array(arr) => {
                let table = lua.create_table()?;
                for (i, v) in arr.iter().enumerate() {
                    table.set(i + 1, self.json_to_lua_value(lua, v)?)?;
                }
                mlua::Value::Table(table)
            }
            serde_json::Value::Object(obj) => {
                let table = lua.create_table()?;
                for (k, v) in obj {
                    table.set(k.as_str(), self.json_to_lua_value(lua, v)?)?;
                }
                mlua::Value::Table(table)
            }
        })
    }

    /// Convert Lua value to JSON
    fn lua_to_json_value(&self, value: &mlua::Value) -> mlua::Result<serde_json::Value> {
        Ok(match value {
            mlua::Value::Nil => serde_json::Value::Null,
            mlua::Value::Boolean(b) => serde_json::Value::Bool(*b),
            mlua::Value::Integer(i) => serde_json::Value::Number((*i).into()),
            mlua::Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            mlua::Value::String(s) => serde_json::Value::String(s.to_str()?.to_string()),
            mlua::Value::Table(t) => {
                let mut is_array = true;
                let mut max_index = 0i64;

                for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                    let (key, _) = pair?;
                    match key {
                        mlua::Value::Integer(i) if i > 0 => {
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
                        let v: mlua::Value = t.get(i)?;
                        arr.push(self.lua_to_json_value(&v)?);
                    }
                    serde_json::Value::Array(arr)
                } else {
                    let mut map = serde_json::Map::new();
                    for pair in t.clone().pairs::<mlua::Value, mlua::Value>() {
                        let (key, val) = pair?;
                        let key_str = match key {
                            mlua::Value::String(s) => s.to_str()?.to_string(),
                            mlua::Value::Integer(i) => i.to_string(),
                            _ => continue,
                        };
                        map.insert(key_str, self.lua_to_json_value(&val)?);
                    }
                    serde_json::Value::Object(map)
                }
            }
            _ => serde_json::Value::Null,
        })
    }

    /// Parse API response from Lua value
    fn parse_api_response(&self, _lua: &mlua::Lua, value: mlua::Value) -> mlua::Result<ApiResponse> {
        match value {
            mlua::Value::Table(table) => {
                let mut response = ApiResponse {
                    status: 200,
                    headers: std::collections::HashMap::new(),
                    body: serde_json::Value::Null,
                };

                // Check for fail marker
                let is_fail: bool = table.get("__fail").unwrap_or(false);

                // Extract status
                if let Ok(status) = table.get::<u16>("status") {
                    response.status = status;
                } else if is_fail {
                    response.status = 400;
                }

                // Extract headers
                if let Ok(headers) = table.get::<mlua::Table>("headers") {
                    for pair in headers.pairs::<String, String>() {
                        if let Ok((k, v)) = pair {
                            response.headers.insert(k, v);
                        }
                    }
                }

                // Check for redirect shorthand
                if let Ok(redirect) = table.get::<String>("redirect") {
                    response.headers.insert("Location".to_string(), redirect);
                    if response.status == 200 {
                        response.status = 302;
                    }
                }

                // Extract body
                let body_value: mlua::Value = table.get("body").unwrap_or(mlua::Value::Nil);
                if !matches!(body_value, mlua::Value::Nil) {
                    response.body = self.lua_to_json_value(&body_value)?;
                } else {
                    // No explicit body - convert table excluding special keys
                    let mut map = serde_json::Map::new();
                    let exclude = &["__fail", "status", "headers", "body", "redirect"];

                    for pair in table.pairs::<mlua::Value, mlua::Value>() {
                        let (key, val) = pair?;
                        let key_str = match key {
                            mlua::Value::String(s) => s.to_str()?.to_string(),
                            mlua::Value::Integer(i) => i.to_string(),
                            _ => continue,
                        };
                        if !exclude.contains(&key_str.as_str()) {
                            map.insert(key_str, self.lua_to_json_value(&val)?);
                        }
                    }
                    response.body = serde_json::Value::Object(map);
                }

                Ok(response)
            }

            mlua::Value::Nil => Ok(ApiResponse {
                status: 204,
                headers: std::collections::HashMap::new(),
                body: serde_json::Value::Null,
            }),

            _ => Ok(ApiResponse {
                status: 200,
                headers: std::collections::HashMap::new(),
                body: self.lua_to_json_value(&value)?,
            }),
        }
    }
}

/// Initialize the WASM module (called automatically)
#[wasm_bindgen(start)]
pub fn init() {
    // Set up console error panic hook for better error messages
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
