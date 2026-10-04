// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Lua expression evaluator for the client runtime.
//!
//! Uses mlua to evaluate reactive expressions and event handlers.
//! The Lua VM holds the current state and evaluates expressions
//! when state changes or events fire.
//!
//! Extended for full-bundle support: loads compiled Lua modules,
//! provides client runtime with reactive state/derived/get/set,
//! and re-renders templates on state changes.

use crate::reactive::ReactiveValue;
use mlua::{Lua, Result as LuaResult, Value, Function, Table};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared state store accessible from Lua callbacks.
/// Uses Arc<Mutex<>> for Send compatibility (required by mlua's MaybeSend).
type SharedStore = Arc<Mutex<HashMap<String, ReactiveValue>>>;

/// The Lua evaluator for reactive expressions.
pub struct LuaEval {
    lua: Lua,
    /// Shared state store for the client runtime callbacks.
    store: SharedStore,
}

impl LuaEval {
    /// Creates a new Lua evaluator with a sandboxed environment.
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        // Sandbox: remove dangerous functions
        lua.globals().set("io", Value::Nil)?;
        lua.globals().set("debug", Value::Nil)?;
        lua.globals().set("loadstring", Value::Nil)?;
        lua.globals().set("loadfile", Value::Nil)?;
        lua.globals().set("dofile", Value::Nil)?;

        // Set up helper functions needed by generated Lua code
        lua.load(r#"
            function html_escape(s)
                if s == nil then return "" end
                s = tostring(s)
                s = string.gsub(s, "&", "&amp;")
                s = string.gsub(s, "<", "&lt;")
                s = string.gsub(s, ">", "&gt;")
                s = string.gsub(s, '"', "&quot;")
                s = string.gsub(s, "'", "&#39;")
                return s
            end

            function smart_tostring(v)
                if v == nil then return "" end
                return tostring(v)
            end
        "#).exec()?;

        let store = Arc::new(Mutex::new(HashMap::new()));

        Ok(Self { lua, store })
    }

    /// Sets a reactive variable in the Lua environment.
    pub fn set_var(&self, name: &str, value: &ReactiveValue) -> LuaResult<()> {
        let lua_val = reactive_to_lua(&self.lua, value)?;
        self.lua.globals().set(name, lua_val)?;
        // Also update the shared store
        self.store.lock().unwrap().insert(name.to_string(), value.clone());
        Ok(())
    }

    /// Gets a variable's current value from the Lua environment.
    pub fn get_var(&self, name: &str) -> LuaResult<ReactiveValue> {
        let val: Value = self.lua.globals().get(name)?;
        Ok(lua_to_reactive(&val))
    }

    /// Evaluates a Lua expression and returns the result.
    pub fn eval_expr(&self, expr: &str) -> LuaResult<ReactiveValue> {
        let chunk = format!("return {}", expr);
        let val: Value = self.lua.load(&chunk).eval()?;
        Ok(lua_to_reactive(&val))
    }

    /// Executes a Lua statement (event handler code).
    /// Returns the names of variables that were modified.
    pub fn exec_handler(
        &self,
        handler: &str,
        tracked_vars: &[String],
    ) -> LuaResult<Vec<(String, ReactiveValue)>> {
        let mut before: HashMap<String, ReactiveValue> = HashMap::new();
        for name in tracked_vars {
            before.insert(name.clone(), self.get_var(name)?);
        }

        self.lua.load(handler).exec()?;

        let mut changed = Vec::new();
        for name in tracked_vars {
            let after = self.get_var(name)?;
            if let Some(prev) = before.get(name) {
                if !values_equal(prev, &after) {
                    changed.push((name.clone(), after));
                }
            }
        }

        Ok(changed)
    }

    // ========================================================================
    // Full-bundle support (new architecture)
    // ========================================================================

    /// Loads a Lua module source code into the VM.
    /// The module is stored in a global modules table keyed by name.
    pub fn load_module(&self, name: &str, source: &str) -> LuaResult<()> {
        // Create __luat_modules table if it doesn't exist
        let globals = self.lua.globals();
        let modules: Table = match globals.get::<Table>("__luat_modules") {
            Ok(t) => t,
            Err(_) => {
                let t = self.lua.create_table()?;
                globals.set("__luat_modules", t.clone())?;
                t
            }
        };

        // Load and execute the module source to get the exports table
        let module_table: Table = self.lua.load(source).eval()?;
        modules.set(name, module_table)?;

        Ok(())
    }

    /// Creates the client runtime table and stores it globally.
    /// The runtime provides reactive state()/derived()/get()/set() methods.
    pub fn setup_client_runtime(&self) -> LuaResult<()> {
        let store = self.store.clone();

        // runtime.state(name, initial) → returns current value from store
        let store_for_state = store.clone();
        let state_fn = self.lua.create_function(move |lua, (name, initial): (String, Value)| {
            let mut store = store_for_state.lock().unwrap();
            if let Some(val) = store.get(&name).cloned() {
                reactive_to_lua(lua, &val)
            } else {
                let rv = lua_to_reactive(&initial);
                store.insert(name, rv);
                Ok(initial)
            }
        })?;

        // runtime.derived(name, fn, deps) → compute value
        let derived_fn = self.lua.create_function(|_, (_name, func, _deps): (String, Function, Value)| {
            func.call::<Value>(())
        })?;

        let runtime = self.lua.create_table()?;
        runtime.set("state", state_fn)?;
        runtime.set("derived", derived_fn)?;

        // Also provide context_stack for compatibility
        let stack = self.lua.create_table()?;
        runtime.set("context_stack", stack)?;

        self.lua.globals().set("__client_runtime", runtime)?;
        Ok(())
    }

    /// Creates the handler runtime table (rt) with get/set methods.
    /// This is passed to event handler functions.
    pub fn create_handler_runtime(&self) -> LuaResult<Table> {
        let store = self.store.clone();

        // rt.get(name) → returns current value
        let store_for_get = store.clone();
        let get_fn = self.lua.create_function(move |lua, name: String| {
            let store = store_for_get.lock().unwrap();
            match store.get(&name).cloned() {
                Some(val) => reactive_to_lua(lua, &val),
                None => Ok(Value::Nil),
            }
        })?;

        // rt.set(name, value) → updates store
        let store_for_set = store.clone();
        let set_fn = self.lua.create_function(move |_, (name, value): (String, Value)| {
            let rv = lua_to_reactive(&value);
            store_for_set.lock().unwrap().insert(name, rv);
            Ok(())
        })?;

        let rt = self.lua.create_table()?;
        rt.set("get", get_fn)?;
        rt.set("set", set_fn)?;
        Ok(rt)
    }

    /// Executes a named handler from a module's exports.__handlers table.
    /// Returns true if execution succeeded.
    pub fn exec_named_handler(&self, module_name: &str, handler_name: &str) -> LuaResult<bool> {
        let globals = self.lua.globals();
        let modules: Table = globals.get("__luat_modules")?;
        let module: Table = modules.get(module_name)?;
        let handlers: Table = module.get("__handlers")?;
        let handler_fn: Function = handlers.get(handler_name)?;

        // Create the rt table for the handler
        let rt = self.create_handler_runtime()?;
        handler_fn.call::<()>(rt)?;

        // Sync store values back to Lua globals
        let store = self.store.lock().unwrap();
        for (name, value) in store.iter() {
            let lua_val = reactive_to_lua(&self.lua, value)?;
            self.lua.globals().set(name.as_str(), lua_val)?;
        }

        Ok(true)
    }

    /// Re-renders a module using the client runtime and returns the HTML.
    pub fn render_module(&self, module_name: &str) -> LuaResult<String> {
        let globals = self.lua.globals();
        let modules: Table = globals.get("__luat_modules")?;
        let module: Table = modules.get(module_name)?;
        let render_fn: Function = module.get("render")?;

        // Get the client runtime
        let runtime: Table = globals.get("__client_runtime")?;

        // Create empty props table
        let props = self.lua.create_table()?;

        // Call render(props, runtime)
        let result: String = render_fn.call((props, runtime))?;
        Ok(result)
    }

    /// Gets the current store state (for syncing with ReactiveStore).
    pub fn get_store_snapshot(&self) -> HashMap<String, ReactiveValue> {
        self.store.lock().unwrap().clone()
    }

    /// Updates the store with external values (e.g., from initial state setup).
    pub fn update_store(&self, name: &str, value: &ReactiveValue) {
        self.store.lock().unwrap().insert(name.to_string(), value.clone());
    }
}

impl Default for LuaEval {
    fn default() -> Self {
        Self::new().expect("Failed to create Lua evaluator")
    }
}

/// Convert a ReactiveValue to a Lua Value.
pub fn reactive_to_lua(lua: &Lua, value: &ReactiveValue) -> LuaResult<Value> {
    match value {
        ReactiveValue::Str(s) => Ok(Value::String(lua.create_string(s)?)),
        ReactiveValue::Num(n) => Ok(Value::Number(*n)),
        ReactiveValue::Bool(b) => Ok(Value::Boolean(*b)),
        ReactiveValue::Nil => Ok(Value::Nil),
    }
}

/// Convert a Lua Value to a ReactiveValue.
pub fn lua_to_reactive(val: &Value) -> ReactiveValue {
    match val {
        Value::String(s) => {
            ReactiveValue::Str(s.to_string_lossy().to_string())
        }
        Value::Integer(n) => ReactiveValue::Num(*n as f64),
        Value::Number(n) => ReactiveValue::Num(*n),
        Value::Boolean(b) => ReactiveValue::Bool(*b),
        Value::Nil => ReactiveValue::Nil,
        _ => ReactiveValue::Nil,
    }
}

/// Compare two ReactiveValues for equality.
fn values_equal(a: &ReactiveValue, b: &ReactiveValue) -> bool {
    match (a, b) {
        (ReactiveValue::Str(a), ReactiveValue::Str(b)) => a == b,
        (ReactiveValue::Num(a), ReactiveValue::Num(b)) => (a - b).abs() < f64::EPSILON,
        (ReactiveValue::Bool(a), ReactiveValue::Bool(b)) => a == b,
        (ReactiveValue::Nil, ReactiveValue::Nil) => true,
        _ => false,
    }
}
