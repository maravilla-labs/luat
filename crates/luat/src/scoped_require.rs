// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Per-module `require` so relative imports resolve against the right file.
//!
//! Module searchers resolve relative names against "the module currently
//! being loaded", tracked in `__luat_current_module`. That slot is shared by
//! the whole Lua state. Once server code runs as coroutines, a `require`
//! that executes after an `await` would see whichever request touched the
//! slot last. Giving each loaded chunk its own `require`, which sets the slot
//! to that chunk's path only for the duration of the (synchronous) call,
//! makes resolution independent of interleaving.

use mlua::{Function, Lua, MultiValue, Result as LuaResult, Table, Value};

use crate::resolver::ResourceResolver;

/// Registry and global key naming the module whose code is running.
pub(crate) const CURRENT_MODULE_KEY: &str = "__luat_current_module";

/// Registry key of the module cache keyed by canonical path.
const MODULE_CACHE_KEY: &str = "__luat_module_cache";

/// Registry key set while modules must be loaded fresh on every require
/// (development mode).
pub(crate) const NO_MODULE_CACHE_KEY: &str = "__luat_no_module_cache";

/// Lua app data that maps `(importer, name)` to a module's canonical path.
///
/// Lua's own `require` caches results in `package.loaded` by the literal
/// name, so `require("./helper")` from two directories would return the
/// same module. Caching by canonical path keeps them apart.
pub(crate) struct ModuleKeys(pub(crate) Box<dyn ResourceResolver>);

/// Drops every module cached by canonical path.
pub(crate) fn clear_module_cache(lua: &Lua) -> LuaResult<()> {
    lua.unset_named_registry_value(MODULE_CACHE_KEY)
}

/// Returns a `require` that resolves names relative to `importer`.
///
/// It looks up the global `require` at call time, so replacements installed
/// later (bundles, dev mode) are honoured.
pub(crate) fn bound_require(lua: &Lua, importer: &str) -> LuaResult<Function> {
    let importer = importer.to_string();
    lua.create_function(move |lua, name: Value| {
        let canonical = match &name {
            Value::String(s) => canonical_key(lua, &importer, &s.to_str()?),
            _ => None,
        };
        let Some(key) = canonical else {
            return require_as(lua, &importer, name);
        };

        let use_cache = !lua.named_registry_value::<bool>(NO_MODULE_CACHE_KEY).unwrap_or(false);
        let cache = module_cache(lua)?;
        if use_cache {
            let cached: Value = cache.raw_get(key.as_str())?;
            if !cached.is_nil() {
                return Ok(MultiValue::from_iter([cached]));
            }
        }

        // Hide any module another importer cached under the same literal
        // name, so the searchers resolve this name relative to `importer`.
        let literal = name.clone();
        let loaded: Table = lua.globals().get::<Table>("package")?.get("loaded")?;
        let shadowed: Value = loaded.raw_get(literal.clone())?;
        loaded.raw_set(literal.clone(), Value::Nil)?;
        let result = require_as(lua, &importer, name);
        loaded.raw_set(literal, shadowed)?;

        let values = result?;
        if use_cache {
            if let Some(module) = values.iter().next() {
                cache.raw_set(key.as_str(), module.clone())?;
            }
        }
        Ok(values)
    })
}

fn canonical_key(lua: &Lua, importer: &str, name: &str) -> Option<String> {
    let keys = lua.app_data_ref::<ModuleKeys>()?;
    keys.0.get_resolved_path(importer, name).ok()
}

fn module_cache(lua: &Lua) -> LuaResult<Table> {
    if let Some(cache) = lua.named_registry_value::<Option<Table>>(MODULE_CACHE_KEY)? {
        return Ok(cache);
    }
    let cache = lua.create_table()?;
    lua.set_named_registry_value(MODULE_CACHE_KEY, cache.clone())?;
    Ok(cache)
}

/// Calls the global `require` with the current-module slot set to
/// `importer` for the duration of the call.
fn require_as(lua: &Lua, importer: &str, name: Value) -> LuaResult<MultiValue> {
    let globals = lua.globals();
    let require: Function = globals.get("require")?;

    let prev_registry: Option<String> = lua.named_registry_value(CURRENT_MODULE_KEY)?;
    let prev_global: Value = globals.raw_get(CURRENT_MODULE_KEY)?;
    lua.set_named_registry_value(CURRENT_MODULE_KEY, importer)?;
    globals.raw_set(CURRENT_MODULE_KEY, importer)?;

    let result = require.call::<MultiValue>(name);

    match prev_registry {
        Some(prev) => lua.set_named_registry_value(CURRENT_MODULE_KEY, prev)?,
        None => lua.unset_named_registry_value(CURRENT_MODULE_KEY)?,
    }
    globals.raw_set(CURRENT_MODULE_KEY, prev_global)?;
    result
}

/// Environment for template modules: reads and writes pass through to
/// globals exactly as before, but `require` is bound to `importer`.
pub(crate) fn module_env(lua: &Lua, importer: &str) -> LuaResult<Table> {
    let env = lua.create_table()?;
    env.raw_set("require", bound_require(lua, importer)?)?;
    let globals = lua.globals();
    let mt = lua.create_table()?;
    mt.set("__index", globals.clone())?;
    mt.set("__newindex", globals)?;
    env.set_metatable(Some(mt));
    Ok(env)
}

/// Environment for server files (`+page.server.lua`, `+server.lua`): reads
/// fall through to globals, but definitions stay local so one route's
/// handlers never leak into another's. `require` is bound to `importer`.
pub(crate) fn handler_env(lua: &Lua, importer: &str) -> LuaResult<Table> {
    let env = lua.create_table()?;
    env.raw_set("require", bound_require(lua, importer)?)?;
    let mt = lua.create_table()?;
    mt.set("__index", lua.globals())?;
    env.set_metatable(Some(mt));
    Ok(env)
}
