// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Lua registration for KV store.

use super::{KVStore, KVStoreFactory, ListOptions, PutOptions};
use mlua::{Lua, MultiValue, Result as LuaResult, Table, Value};
use serde_json::Value as JsonValue;
use std::sync::Arc;

/// Registers the KV module in Lua globals.
///
/// This makes `KV.namespace("name")` available to Lua code.
///
/// # Example
///
/// ```rust,ignore
/// use luat::kv::{register_kv_module, MemoryKVStore, KVStoreFactory};
///
/// let lua = Lua::new();
/// let factory: KVStoreFactory = Arc::new(|namespace| {
///     Arc::new(MemoryKVStore::new())
/// });
/// register_kv_module(&lua, factory)?;
/// ```
pub fn register_kv_module(lua: &Lua, factory: KVStoreFactory) -> LuaResult<()> {
    let kv_table = lua.create_table()?;

    // KV.namespace("name") -> returns namespace table with methods
    let namespace_fn = lua.create_function(move |lua, name: String| {
        let store = factory(&name);
        create_namespace_table(lua, store)
    })?;

    kv_table.set("namespace", namespace_fn)?;
    lua.globals().set("KV", kv_table)?;

    Ok(())
}

/// Creates a Lua table representing a KV namespace with all methods.
fn create_namespace_table(lua: &Lua, store: Arc<dyn KVStore>) -> LuaResult<Table> {
    let ns = lua.create_table()?;

    // get(self, key, type?) -> value or nil
    // Note: Lua's colon syntax passes self as the first argument
    let store_get = store.clone();
    ns.set(
        "get",
        lua.create_function(
            move |lua, (_self, key, type_hint): (Value, String, Option<String>)| {
                match store_get.get(&key) {
                    Ok(Some(bytes)) => match type_hint.as_deref() {
                        Some("json") => {
                            let json: JsonValue = serde_json::from_slice(&bytes)
                                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                            json_to_lua(lua, &json)
                        }
                        Some("text") | None => {
                            let s = String::from_utf8_lossy(&bytes);
                            Ok(Value::String(lua.create_string(s.as_ref())?))
                        }
                        Some("arrayBuffer") => {
                            // Return as Lua string (binary safe)
                            Ok(Value::String(lua.create_string(&bytes)?))
                        }
                        Some(other) => Err(mlua::Error::runtime(format!(
                            "Unknown type hint: {}. Expected 'text', 'json', or 'arrayBuffer'",
                            other
                        ))),
                    },
                    Ok(None) => Ok(Value::Nil),
                    Err(e) => Err(mlua::Error::runtime(e.to_string())),
                }
            },
        )?,
    )?;

    // getWithMetadata(self, key, type?) -> value, metadata (multiple return values)
    let store_get_meta = store.clone();
    ns.set(
        "getWithMetadata",
        lua.create_function(
            move |lua, (_self, key, type_hint): (Value, String, Option<String>)| {
                match store_get_meta.get_with_metadata(&key) {
                    Ok(Some(entry)) => {
                        let value = match type_hint.as_deref() {
                            Some("json") => {
                                let json: JsonValue = serde_json::from_slice(&entry.value)
                                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                                json_to_lua(lua, &json)?
                            }
                            Some("text") | None => {
                                let s = String::from_utf8_lossy(&entry.value);
                                Value::String(lua.create_string(s.as_ref())?)
                            }
                            Some("arrayBuffer") => {
                                Value::String(lua.create_string(&entry.value)?)
                            }
                            Some(other) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Unknown type hint: {}",
                                    other
                                )))
                            }
                        };

                        // Create metadata table
                        let meta_table = lua.create_table()?;
                        if let Some(meta) = entry.metadata {
                            meta_table.set("metadata", json_to_lua(lua, &meta)?)?;
                        }
                        if let Some(exp) = entry.expiration {
                            meta_table.set("expiration", exp)?;
                        }

                        Ok(MultiValue::from_vec(vec![value, Value::Table(meta_table)]))
                    }
                    Ok(None) => Ok(MultiValue::from_vec(vec![Value::Nil, Value::Nil])),
                    Err(e) => Err(mlua::Error::runtime(e.to_string())),
                }
            },
        )?,
    )?;

    // put(self, key, value, options?)
    let store_put = store.clone();
    ns.set(
        "put",
        lua.create_function(
            move |lua, (_self, key, value, options): (Value, String, Value, Option<Table>)| {
                // Convert value to bytes
                let bytes = lua_value_to_bytes(lua, &value)?;

                // Parse options
                let put_options = if let Some(opts) = options {
                    parse_put_options(lua, &opts)?
                } else {
                    PutOptions::default()
                };

                store_put
                    .put(&key, &bytes, put_options)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;

                Ok(())
            },
        )?,
    )?;

    // delete(self, key)
    let store_delete = store.clone();
    ns.set(
        "delete",
        lua.create_function(move |_lua, (_self, key): (Value, String)| {
            store_delete
                .delete(&key)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            Ok(())
        })?,
    )?;

    // list(self, options?) -> { keys = [...], list_complete = bool, cursor = string? }
    let store_list = store;
    ns.set(
        "list",
        lua.create_function(move |lua, (_self, options): (Value, Option<Table>)| {
            let list_options = if let Some(opts) = options {
                parse_list_options(&opts)?
            } else {
                ListOptions::default()
            };

            let result = store_list
                .list(list_options)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;

            // Build result table
            let result_table = lua.create_table()?;

            // keys array
            let keys_table = lua.create_table()?;
            for (i, key) in result.keys.iter().enumerate() {
                let key_table = lua.create_table()?;
                key_table.set("name", key.name.as_str())?;
                if let Some(exp) = key.expiration {
                    key_table.set("expiration", exp)?;
                }
                if let Some(ref meta) = key.metadata {
                    key_table.set("metadata", json_to_lua(lua, meta)?)?;
                }
                keys_table.set(i + 1, key_table)?;
            }
            result_table.set("keys", keys_table)?;

            result_table.set("list_complete", result.list_complete)?;

            if let Some(cursor) = result.cursor {
                result_table.set("cursor", cursor)?;
            }

            Ok(result_table)
        })?,
    )?;

    Ok(ns)
}

/// Converts a Lua value to bytes for storage.
fn lua_value_to_bytes(lua: &Lua, value: &Value) -> LuaResult<Vec<u8>> {
    match value {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Integer(i) => Ok(i.to_string().into_bytes()),
        Value::Number(n) => Ok(n.to_string().into_bytes()),
        Value::Boolean(b) => Ok(b.to_string().into_bytes()),
        Value::Table(_) => {
            // Serialize table as JSON
            let json = lua_to_json(lua, value)?;
            Ok(serde_json::to_vec(&json).map_err(|e| mlua::Error::runtime(e.to_string()))?)
        }
        Value::Nil => Ok(Vec::new()),
        _ => Err(mlua::Error::runtime(
            "Cannot store function, thread, or userdata values",
        )),
    }
}

/// Parses a Lua table into PutOptions.
fn parse_put_options(lua: &Lua, table: &Table) -> LuaResult<PutOptions> {
    let mut options = PutOptions::default();

    // expiration (Unix timestamp)
    if let Ok(exp) = table.get::<u64>("expiration") {
        options.expiration = Some(exp);
    }

    // expirationTtl (seconds from now)
    if let Ok(ttl) = table.get::<u64>("expirationTtl") {
        options.expiration_ttl = Some(ttl);
    }

    // metadata (arbitrary JSON)
    if let Ok(meta) = table.get::<Table>("metadata") {
        options.metadata = Some(lua_to_json(lua, &Value::Table(meta))?);
    }

    Ok(options)
}

/// Parses a Lua table into ListOptions.
fn parse_list_options(table: &Table) -> LuaResult<ListOptions> {
    let mut options = ListOptions::default();

    if let Ok(prefix) = table.get::<String>("prefix") {
        options.prefix = Some(prefix);
    }

    if let Ok(limit) = table.get::<usize>("limit") {
        options.limit = Some(limit);
    }

    if let Ok(cursor) = table.get::<String>("cursor") {
        options.cursor = Some(cursor);
    }

    Ok(options)
}

/// Converts a JSON value to a Lua value.
fn json_to_lua(lua: &Lua, json: &JsonValue) -> LuaResult<Value> {
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
        JsonValue::String(s) => Value::String(lua.create_string(s)?),
        JsonValue::Array(arr) => {
            let table = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                table.set(i + 1, json_to_lua(lua, v)?)?;
            }
            Value::Table(table)
        }
        JsonValue::Object(obj) => {
            let table = lua.create_table()?;
            for (k, v) in obj {
                table.set(k.as_str(), json_to_lua(lua, v)?)?;
            }
            Value::Table(table)
        }
    })
}

/// Converts a Lua value to JSON.
#[allow(clippy::only_used_in_recursion)]
fn lua_to_json(lua: &Lua, value: &Value) -> LuaResult<JsonValue> {
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
                    arr.push(lua_to_json(lua, &v)?);
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
                    map.insert(key_str, lua_to_json(lua, &val)?);
                }
                JsonValue::Object(map)
            }
        }
        _ => JsonValue::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::MemoryKVStore;

    fn create_test_lua() -> Lua {
        let lua = Lua::new();
        let factory: KVStoreFactory = Arc::new(|_namespace| Arc::new(MemoryKVStore::new()));
        register_kv_module(&lua, factory).unwrap();
        lua
    }

    #[test]
    fn test_basic_get_put() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            kv:put("key1", "value1")
            result = kv:get("key1")
        "#,
        )
        .exec()
        .unwrap();

        let result: String = lua.globals().get("result").unwrap();
        assert_eq!(result, "value1");
    }

    #[test]
    fn test_json_type() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            kv:put("data", { name = "Alice", age = 30 })
            result = kv:get("data", "json")
        "#,
        )
        .exec()
        .unwrap();

        let result: Table = lua.globals().get("result").unwrap();
        let name: String = result.get("name").unwrap();
        let age: i64 = result.get("age").unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(age, 30);
    }

    #[test]
    fn test_delete() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            kv:put("key1", "value1")
            kv:delete("key1")
            result = kv:get("key1")
        "#,
        )
        .exec()
        .unwrap();

        let result: Value = lua.globals().get("result").unwrap();
        assert!(matches!(result, Value::Nil));
    }

    #[test]
    fn test_get_with_metadata() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            kv:put("key1", "value1", { metadata = { author = "test" } })
            value, meta = kv:getWithMetadata("key1")
        "#,
        )
        .exec()
        .unwrap();

        let value: String = lua.globals().get("value").unwrap();
        assert_eq!(value, "value1");

        let meta: Table = lua.globals().get("meta").unwrap();
        let metadata: Table = meta.get("metadata").unwrap();
        let author: String = metadata.get("author").unwrap();
        assert_eq!(author, "test");
    }

    #[test]
    fn test_list() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            kv:put("blog:post1", "content1")
            kv:put("blog:post2", "content2")
            kv:put("user:alice", "data")
            result = kv:list({ prefix = "blog:" })
        "#,
        )
        .exec()
        .unwrap();

        let result: Table = lua.globals().get("result").unwrap();
        let keys: Table = result.get("keys").unwrap();
        let list_complete: bool = result.get("list_complete").unwrap();

        assert!(list_complete);

        // Count keys
        let mut count = 0;
        for pair in keys.pairs::<i64, Table>() {
            let (_, key_entry) = pair.unwrap();
            let name: String = key_entry.get("name").unwrap();
            assert!(name.starts_with("blog:"));
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn test_list_pagination() {
        let lua = create_test_lua();

        lua.load(
            r#"
            local kv = KV.namespace("test")
            for i = 1, 5 do
                kv:put("key" .. i, "value" .. i)
            end
            result = kv:list({ limit = 2 })
        "#,
        )
        .exec()
        .unwrap();

        let result: Table = lua.globals().get("result").unwrap();
        let keys: Table = result.get("keys").unwrap();
        let list_complete: bool = result.get("list_complete").unwrap();

        assert!(!list_complete);

        // Count keys in first page
        let mut count = 0;
        for _ in keys.pairs::<i64, Table>() {
            count += 1;
        }
        assert_eq!(count, 2);
    }
}
