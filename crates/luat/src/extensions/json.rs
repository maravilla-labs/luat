// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! JSON module registration for Lua.
//!
//! Provides `json.encode`, `json.decode`, `json.encode_pretty`, and `json.null`.

use mlua::{Lua, LuaSerdeExt, Result as LuaResult, Table, Value};

/// Register the json module as a global on the given Lua instance.
///
/// This makes `json.encode()`, `json.decode()`, `json.encode_pretty()`,
/// and `json.null` available in Lua code.
///
/// # Example
///
/// ```rust,ignore
/// use mlua::Lua;
/// use luat::extensions::json::register_json_module;
///
/// let lua = Lua::new();
/// register_json_module(&lua)?;
/// ```
pub fn register_json_module(lua: &Lua) -> LuaResult<()> {
    let json_module = lua.create_table()?;

    // 1. JSON Encode function (Lua table -> JSON string)
    let encode = lua.create_function(|lua, value: Value| {
        match serde_json::to_string(&lua.to_value(&value).unwrap()) {
            Ok(json_str) => Ok(json_str),
            Err(err) => Err(mlua::Error::external(format!("JSON encode error: {}", err))),
        }
    })?;
    json_module.set("encode", encode)?;

    // 2. Pretty JSON Encode function (with indentation)
    let encode_pretty = lua.create_function(|lua, value: Value| {
        match serde_json::to_string_pretty(&lua.to_value(&value).unwrap()) {
            Ok(json_str) => Ok(json_str),
            Err(err) => Err(mlua::Error::external(format!("JSON encode error: {}", err))),
        }
    })?;
    json_module.set("encode_pretty", encode_pretty)?;

    // 3. JSON Decode function (JSON string -> Lua table)
    let decode = lua.create_function(|lua, json_str: String| {
        match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(json_value) => lua.to_value(&json_value),
            Err(err) => Err(mlua::Error::external(format!("JSON decode error: {}", err))),
        }
    })?;
    json_module.set("decode", decode)?;

    // 4. Null value (since Lua doesn't have a native null)
    let null = lua.create_table()?;
    null.set("__jsontype", "null")?;
    json_module.set("null", null)?;

    // Register as global 'json'
    let globals = lua.globals();
    globals.set("json", json_module)?;

    // Also register in package.preload for require("json")
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;

    let json_loader = lua.create_function(|lua, _: ()| {
        let module = lua.create_table()?;

        let encode = lua.create_function(|lua, value: Value| {
            match serde_json::to_string(&lua.to_value(&value).unwrap()) {
                Ok(json_str) => Ok(json_str),
                Err(err) => Err(mlua::Error::external(format!("JSON encode error: {}", err))),
            }
        })?;
        module.set("encode", encode)?;

        let encode_pretty = lua.create_function(|lua, value: Value| {
            match serde_json::to_string_pretty(&lua.to_value(&value).unwrap()) {
                Ok(json_str) => Ok(json_str),
                Err(err) => Err(mlua::Error::external(format!("JSON encode error: {}", err))),
            }
        })?;
        module.set("encode_pretty", encode_pretty)?;

        let decode = lua.create_function(|lua, json_str: String| {
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(json_value) => lua.to_value(&json_value),
                Err(err) => Err(mlua::Error::external(format!("JSON decode error: {}", err))),
            }
        })?;
        module.set("decode", decode)?;

        let null = lua.create_table()?;
        null.set("__jsontype", "null")?;
        module.set("null", null)?;

        Ok(module)
    })?;
    preload.set("json", json_loader)?;

    Ok(())
}
