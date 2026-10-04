// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Luat Client Runtime — WASM module for reactive DOM updates.
//!
//! Compiled with Emscripten, provides a C FFI API for JavaScript.
//! JavaScript handles DOM operations (walking, patching, events).
//! WASM handles computation (Lua VM, reactive store, expression eval).
//!
//! # Architecture (Option C — batched updates)
//!
//! 1. JS finds markers in DOM via TreeWalker
//! 2. JS calls `luat_client_decode_marker(base64)` → gets marker data as JSON
//! 3. JS calls `luat_client_register_state(name, value)` for each reactive var
//! 4. JS calls `luat_client_register_subscriber(id, expr, deps_json)` for each binding
//! 5. On event: JS calls `luat_client_exec_handler(expr)` → gets full update plan as JSON
//! 6. JS patches DOM nodes using the returned `[{sub_id, value}]` array

mod eval;
mod marker;
mod reactive;

use eval::LuaEval;
use reactive::{ReactiveStore, ReactiveValue, Subscriber, SubscriberId, SubscriberKind};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ============================================================================
// Global State (thread-local for WASM single-threaded environment)
// ============================================================================

struct ClientState {
    /// Lua expression evaluator.
    eval: LuaEval,
    /// Reactive dependency graph and state store.
    store: ReactiveStore,
    /// All tracked reactive variable names.
    tracked_vars: Vec<String>,
}

thread_local! {
    static STATE: RefCell<Option<ClientState>> = const { RefCell::new(None) };
}

// ============================================================================
// Extern "C" API for JavaScript
// ============================================================================

/// Initialize the client runtime. Must be called before any other functions.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn luat_client_init() -> i32 {
    match LuaEval::new() {
        Ok(eval) => {
            let state = ClientState {
                eval,
                store: ReactiveStore::new(),
                tracked_vars: Vec::new(),
            };
            STATE.with(|s| {
                *s.borrow_mut() = Some(state);
            });
            0
        }
        Err(_) => -1,
    }
}

/// Decode a base64-encoded marker from an HTML comment.
/// Returns a JSON string describing the marker, or null on error.
/// The caller must free the returned string with `luat_client_free_string`.
///
/// Returned JSON format:
/// - Expression: `{"type":"expr","id":N,"expr":"...","deps":["..."],"value":"..."}`
/// - Event: `{"type":"event","id":N,"bindings":[{"event":"click","handler":"...","modifiers":[]}]}`
/// - Attribute: `{"type":"attr","id":N,"bindings":[{"attr":"cx","expr":"x","deps":["x"],"value":"50"}]}`
///
/// # Safety
///
/// `payload` must be a valid pointer to a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn luat_client_decode_marker(payload: *const c_char) -> *mut c_char {
    if payload.is_null() {
        return std::ptr::null_mut();
    }

    let payload_str = match CStr::from_ptr(payload).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let decoded = match marker::try_decode(payload_str) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let json = match &decoded {
        marker::DecodedMarker::Expression(m) => {
            serde_json::json!({
                "type": "expr",
                "id": m.id,
                "expr": m.expr,
                "deps": m.deps,
                "value": marker_value_to_json(&m.value),
            })
        }
        marker::DecodedMarker::Event(em) => {
            let bindings: Vec<_> = em.bindings.iter().map(|b| {
                serde_json::json!({
                    "event": b.event_type,
                    "handler": b.handler,
                    "modifiers": b.modifiers,
                })
            }).collect();
            serde_json::json!({
                "type": "event",
                "id": em.id,
                "bindings": bindings,
            })
        }
        marker::DecodedMarker::Attribute(am) => {
            let bindings: Vec<_> = am.bindings.iter().map(|b| {
                serde_json::json!({
                    "attr": b.attr,
                    "expr": b.expr,
                    "deps": b.deps,
                    "value": marker_value_to_json(&b.value),
                })
            }).collect();
            serde_json::json!({
                "type": "attr",
                "id": am.id,
                "bindings": bindings,
            })
        }
        marker::DecodedMarker::Component(m) => {
            let nesting = match &m.value {
                marker::MarkerValue::Str(s) => {
                    let parts: Vec<&str> = s.split(':').collect();
                    if parts.len() == 3 {
                        serde_json::json!({
                            "parent_id": parts[0].parse::<u16>().unwrap_or(65535),
                            "depth": parts[1].parse::<u16>().unwrap_or(0),
                            "sibling_index": parts[2].parse::<u16>().unwrap_or(0),
                        })
                    } else {
                        serde_json::Value::Null
                    }
                }
                _ => serde_json::Value::Null,
            };
            serde_json::json!({
                "type": "component",
                "id": m.id,
                "expr": m.expr,
                "deps": m.deps,
                "nesting": nesting,
            })
        }
        marker::DecodedMarker::DerivedDef(m) => {
            let var_name = match &m.value {
                marker::MarkerValue::Str(s) => s.clone(),
                _ => String::new(),
            };
            serde_json::json!({
                "type": "derived_def",
                "id": m.id,
                "name": var_name,
                "expr": m.expr,
                "deps": m.deps,
            })
        }
        marker::DecodedMarker::TemplateBoundary(m) => {
            serde_json::json!({
                "type": "template_boundary",
                "id": m.id,
                "template": m.expr,
            })
        }
    };

    json_to_cstring(&json)
}

/// Register a reactive state variable with an initial value.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `name` and `value_json` must be valid pointers to null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn luat_client_register_state(
    name: *const c_char,
    value_json: *const c_char,
) -> i32 {
    if name.is_null() || value_json.is_null() {
        return -1;
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let value_str = match CStr::from_ptr(value_json).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let value = json_to_reactive(value_str);

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Don't overwrite an existing value with Nil.
            // Derived vars are registered first with their computed initial value;
            // expression markers may later call register_state with Nil for the
            // same variable — we must not clobber the correct value.
            let should_set = match &value {
                ReactiveValue::Nil => state.store.get(name_str).is_none(),
                _ => true,
            };

            if should_set {
                // Set in reactive store
                state.store.set_initial(name_str, value.clone());
                // Set in Lua VM
                let _ = state.eval.set_var(name_str, &value);
            }

            // Track this variable
            if !state.tracked_vars.contains(&name_str.to_string()) {
                state.tracked_vars.push(name_str.to_string());
            }

            0
        } else {
            -1
        }
    })
}

/// Register a subscriber (expression or attribute binding).
/// `id` is the subscriber ID (assigned by JS).
/// `kind` is "text" or "attr".
/// `expr` is the Lua expression to evaluate.
/// `deps_json` is a JSON array of dependency variable names.
/// `attr_name` is the attribute name (only for kind="attr", null for "text").
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// All pointer parameters must be valid null-terminated UTF-8 strings (or null for optional ones).
#[no_mangle]
pub unsafe extern "C" fn luat_client_register_subscriber(
    id: u16,
    kind: *const c_char,
    expr: *const c_char,
    deps_json: *const c_char,
    attr_name: *const c_char,
) -> i32 {
    if kind.is_null() || expr.is_null() || deps_json.is_null() {
        return -1;
    }

    let kind_str = match CStr::from_ptr(kind).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let expr_str = match CStr::from_ptr(expr).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let deps_str = match CStr::from_ptr(deps_json).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let deps: Vec<String> = match serde_json::from_str(deps_str) {
        Ok(d) => d,
        Err(_) => return -1,
    };

    let subscriber_kind = match kind_str {
        "text" => SubscriberKind::TextContent {
            expr: expr_str.to_string(),
        },
        "attr" => {
            let attr = if attr_name.is_null() {
                return -1;
            } else {
                match CStr::from_ptr(attr_name).to_str() {
                    Ok(s) => s.to_string(),
                    Err(_) => return -1,
                }
            };
            SubscriberKind::Attribute {
                attr,
                expr: expr_str.to_string(),
            }
        }
        _ => return -1,
    };

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            let sub = Subscriber {
                kind: subscriber_kind,
                deps,
            };
            state.store.subscribe_with_id(SubscriberId(id), sub);
            0
        } else {
            -1
        }
    })
}

/// Register a derived variable (name, expression, dependencies).
/// The expression will be re-evaluated whenever any dependency changes.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// All pointer parameters must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn luat_client_register_derived(
    name: *const c_char,
    expression: *const c_char,
    deps_json: *const c_char,
) -> i32 {
    if name.is_null() || expression.is_null() || deps_json.is_null() {
        return -1;
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let expr_str = match CStr::from_ptr(expression).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let deps_str = match CStr::from_ptr(deps_json).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let deps: Vec<String> = match serde_json::from_str(deps_str) {
        Ok(d) => d,
        Err(_) => return -1,
    };

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Register in reactive store
            state.store.register_derived(
                name_str.to_string(),
                expr_str.to_string(),
                deps.clone(),
            );

            // Track the derived variable name so exec_handler can detect changes
            if !state.tracked_vars.contains(&name_str.to_string()) {
                state.tracked_vars.push(name_str.to_string());
            }

            // Evaluate initial value and set in Lua VM
            if let Ok(val) = state.eval.eval_expr(expr_str) {
                let _ = state.eval.set_var(name_str, &val);
                state.store.set_initial(name_str, val);
            }

            0
        } else {
            -1
        }
    })
}

/// Execute an event handler expression in the Lua VM.
/// Returns a JSON string with the full update plan:
/// `[{"sub_id": N, "value": "..."}, ...]`
///
/// This is the core of Option C — one WASM call returns all DOM updates needed.
/// Returns null on error. Caller must free with `luat_client_free_string`.
///
/// # Safety
///
/// `handler_expr` must be a valid pointer to a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn luat_client_exec_handler(handler_expr: *const c_char) -> *mut c_char {
    if handler_expr.is_null() {
        return std::ptr::null_mut();
    }

    let expr_str = match CStr::from_ptr(handler_expr).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Execute the handler in Lua, tracking variable changes
            let changed = match state.eval.exec_handler(expr_str, &state.tracked_vars) {
                Ok(c) => c,
                Err(_) => return std::ptr::null_mut(),
            };

            if changed.is_empty() {
                // No state changes — return empty array
                return json_to_cstring(&serde_json::json!([]));
            }

            // Cascade through derived variables and collect all dirty subscribers
            let all_dirty = cascade_derived_and_collect_dirty(
                &state.eval,
                &mut state.store,
                &changed,
            );

            // Evaluate all dirty expressions and build update plan
            let updates = build_update_plan(&state.eval, &state.store, &all_dirty);
            json_to_cstring(&updates)
        } else {
            std::ptr::null_mut()
        }
    })
}

/// Set a reactive state variable directly (from JS).
/// Returns a JSON update plan like `exec_handler`.
/// Caller must free with `luat_client_free_string`.
///
/// # Safety
///
/// `name` and `value_json` must be valid pointers to null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn luat_client_set_state(
    name: *const c_char,
    value_json: *const c_char,
) -> *mut c_char {
    if name.is_null() || value_json.is_null() {
        return std::ptr::null_mut();
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let value_str = match CStr::from_ptr(value_json).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let value = json_to_reactive(value_str);

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Sync to Lua VM
            let _ = state.eval.set_var(name_str, &value);

            // Cascade through derived variables and collect all dirty subscribers
            let changed = vec![(name_str.to_string(), value)];
            let all_dirty = cascade_derived_and_collect_dirty(
                &state.eval,
                &mut state.store,
                &changed,
            );

            if all_dirty.is_empty() {
                return json_to_cstring(&serde_json::json!([]));
            }

            // Build update plan
            let updates = build_update_plan(&state.eval, &state.store, &all_dirty);
            json_to_cstring(&updates)
        } else {
            std::ptr::null_mut()
        }
    })
}

/// Evaluate a single Lua expression and return the result as a string.
/// Caller must free with `luat_client_free_string`.
///
/// # Safety
///
/// `expr` must be a valid pointer to a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn luat_client_eval_expr(expr: *const c_char) -> *mut c_char {
    if expr.is_null() {
        return std::ptr::null_mut();
    }

    let expr_str = match CStr::from_ptr(expr).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    STATE.with(|s| {
        let state_ref = s.borrow();
        if let Some(ref state) = *state_ref {
            match state.eval.eval_expr(expr_str) {
                Ok(val) => {
                    let display = val.to_display_string();
                    match CString::new(display) {
                        Ok(cs) => cs.into_raw(),
                        Err(_) => std::ptr::null_mut(),
                    }
                }
                Err(_) => std::ptr::null_mut(),
            }
        } else {
            std::ptr::null_mut()
        }
    })
}

// ============================================================================
// Full-Bundle API (new architecture)
// ============================================================================

/// Load the full Lua bundle (JSON map of module_name → source).
/// Must be called after `luat_client_init()`.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `bundle_json` must be a valid pointer to a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn luat_client_load_bundle(bundle_json: *const c_char) -> i32 {
    if bundle_json.is_null() {
        return -1;
    }

    let json_str = match CStr::from_ptr(bundle_json).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Parse as JSON object: { "module_name": "lua_source", ... }
    let bundle: std::collections::HashMap<String, String> = match serde_json::from_str(json_str) {
        Ok(b) => b,
        Err(_) => return -1,
    };

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Set up the client runtime
            if state.eval.setup_client_runtime().is_err() {
                return -1;
            }

            // Load each module
            for (name, source) in &bundle {
                if state.eval.load_module(name, source).is_err() {
                    return -1;
                }
            }

            0
        } else {
            -1
        }
    })
}

/// Execute a named event handler and return re-rendered HTML.
/// The handler is looked up in the module's exports.__handlers table.
/// After execution, the module is re-rendered with updated state.
/// Returns the new HTML string, or null on error.
/// Caller must free with `luat_client_free_string`.
///
/// # Safety
///
/// `module_name` and `handler_name` must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn luat_client_exec_named_handler(
    module_name: *const c_char,
    handler_name: *const c_char,
) -> *mut c_char {
    if module_name.is_null() || handler_name.is_null() {
        return std::ptr::null_mut();
    }

    let module_str = match CStr::from_ptr(module_name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let handler_str = match CStr::from_ptr(handler_name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    STATE.with(|s| {
        let mut state_ref = s.borrow_mut();
        if let Some(ref mut state) = *state_ref {
            // Execute the named handler
            match state.eval.exec_named_handler(module_str, handler_str) {
                Ok(_) => {}
                Err(_) => return std::ptr::null_mut(),
            }

            // Re-render the module with updated state
            match state.eval.render_module(module_str) {
                Ok(html) => {
                    match CString::new(html) {
                        Ok(cs) => cs.into_raw(),
                        Err(_) => std::ptr::null_mut(),
                    }
                }
                Err(_) => std::ptr::null_mut(),
            }
        } else {
            std::ptr::null_mut()
        }
    })
}

/// Free a string allocated by any luat_client_* function.
///
/// # Safety
///
/// `ptr` must be either null or a valid pointer previously returned by a luat_client_* function.
#[no_mangle]
pub unsafe extern "C" fn luat_client_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// Get the version of the client runtime.
/// Returns a static string (caller must NOT free).
#[no_mangle]
pub extern "C" fn luat_client_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Cascade through derived variables after state changes.
///
/// Given a set of changed variable names, finds derived vars that depend on them,
/// re-evaluates them, and if their value changed, adds them to the changed set.
/// Repeats until no more derived vars are affected (topological cascade).
///
/// Returns a list of all dirty subscriber IDs (from both direct and derived changes).
fn cascade_derived_and_collect_dirty(
    eval: &eval::LuaEval,
    store: &mut ReactiveStore,
    initially_changed: &[(String, ReactiveValue)],
) -> Vec<SubscriberId> {
    let mut all_dirty: Vec<SubscriberId> = Vec::new();

    // First, apply direct state changes
    for (name, new_value) in initially_changed {
        let dirty = store.set(name, new_value.clone());
        for id in dirty {
            if !all_dirty.contains(&id) {
                all_dirty.push(id);
            }
        }
    }

    // Cascade through derived variables
    let mut changed_names: Vec<String> = initially_changed.iter().map(|(n, _)| n.clone()).collect();
    let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        let mut newly_changed: Vec<(String, ReactiveValue)> = Vec::new();

        // Collect derived vars that depend on any of the changed names
        let derived_to_eval: Vec<(String, String)> = {
            let mut to_eval = Vec::new();
            for changed_name in &changed_names {
                let dependents = store.get_derived_dependents(changed_name);
                for dv in dependents {
                    if !processed.contains(&dv.name) {
                        to_eval.push((dv.name.clone(), dv.expression.clone()));
                    }
                }
            }
            to_eval
        };

        if derived_to_eval.is_empty() {
            break;
        }

        for (name, expression) in &derived_to_eval {
            processed.insert(name.clone());

            // Evaluate the derived expression
            if let Ok(new_val) = eval.eval_expr(expression) {
                // Check if value actually changed
                let changed = match store.get(name) {
                    Some(old) => old.to_display_string() != new_val.to_display_string(),
                    None => true,
                };

                if changed {
                    // Update Lua VM and store
                    let _ = eval.set_var(name, &new_val);
                    let dirty = store.set(name, new_val.clone());
                    for id in dirty {
                        if !all_dirty.contains(&id) {
                            all_dirty.push(id);
                        }
                    }
                    newly_changed.push((name.clone(), new_val));
                }
            }
        }

        if newly_changed.is_empty() {
            break;
        }

        changed_names = newly_changed.iter().map(|(n, _)| n.clone()).collect();
    }

    all_dirty
}

/// Build the update plan: evaluate all dirty subscriber expressions.
/// Returns a JSON array of `[{sub_id, value, kind, attr?}]`.
fn build_update_plan(
    eval: &LuaEval,
    store: &ReactiveStore,
    dirty: &[SubscriberId],
) -> serde_json::Value {
    let mut updates = Vec::new();

    for id in dirty {
        if let Some(sub) = store.get_subscriber(*id) {
            let expr = match &sub.kind {
                SubscriberKind::TextContent { expr } => expr,
                SubscriberKind::Attribute { expr, .. } => expr,
            };

            // Evaluate expression in Lua
            let value = match eval.eval_expr(expr) {
                Ok(v) => v.to_display_string(),
                Err(_) => continue,
            };

            let update = match &sub.kind {
                SubscriberKind::TextContent { .. } => {
                    serde_json::json!({
                        "sub_id": id.0,
                        "kind": "text",
                        "value": value,
                    })
                }
                SubscriberKind::Attribute { attr, .. } => {
                    serde_json::json!({
                        "sub_id": id.0,
                        "kind": "attr",
                        "attr": attr,
                        "value": value,
                    })
                }
            };

            updates.push(update);
        }
    }

    serde_json::Value::Array(updates)
}

/// Convert a MarkerValue to a JSON value for the decode API.
fn marker_value_to_json(value: &marker::MarkerValue) -> serde_json::Value {
    match value {
        marker::MarkerValue::Nil => serde_json::Value::Null,
        marker::MarkerValue::Bool(b) => serde_json::Value::Bool(*b),
        marker::MarkerValue::Int(n) => serde_json::json!(*n),
        marker::MarkerValue::Float(f) => serde_json::json!(*f),
        marker::MarkerValue::Str(s) => serde_json::Value::String(s.clone()),
    }
}

/// Convert a JSON string value to a ReactiveValue.
fn json_to_reactive(json_str: &str) -> ReactiveValue {
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(serde_json::Value::String(s)) => ReactiveValue::Str(s),
        Ok(serde_json::Value::Number(n)) => {
            ReactiveValue::Num(n.as_f64().unwrap_or(0.0))
        }
        Ok(serde_json::Value::Bool(b)) => ReactiveValue::Bool(b),
        Ok(serde_json::Value::Null) => ReactiveValue::Nil,
        _ => {
            // Treat as raw string if not valid JSON
            ReactiveValue::Str(json_str.to_string())
        }
    }
}

/// Convert a serde_json::Value to a CString pointer.
fn json_to_cstring(value: &serde_json::Value) -> *mut c_char {
    let json_str = value.to_string();
    match CString::new(json_str) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Main (empty for library use)
// ============================================================================

fn main() {
    // Empty — library mode via extern "C" exports.
}
