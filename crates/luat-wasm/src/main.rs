// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! WASM Library and Test Runner for Luat
//!
//! This binary provides:
//! 1. A JavaScript-callable API via extern "C" functions for browser/Node.js usage
//! 2. Integration tests for the WASM build (runs on startup in test mode)
//!
//! Compiled with Emscripten and can be used in browsers or Node.js.

use luat::cache::MemoryCache;
use luat::engine::Engine;
use luat::memory_resolver::MemoryResourceResolver;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ============================================================================
// Global Engine State (thread-local for WASM single-threaded environment)
// ============================================================================

thread_local! {
    static ENGINE: RefCell<Option<Engine<MemoryResourceResolver>>> = const { RefCell::new(None) };
    static RESOLVER: RefCell<Option<MemoryResourceResolver>> = const { RefCell::new(None) };
}

// ============================================================================
// Extern "C" API for JavaScript
// ============================================================================

/// Initialize the Luat engine. Must be called before any other functions.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn luat_init() -> i32 {
    let resolver = MemoryResourceResolver::new();
    let cache = Box::new(MemoryCache::new(100));

    match Engine::new(resolver.clone(), cache) {
        Ok(engine) => {
            ENGINE.with(|e| {
                *e.borrow_mut() = Some(engine);
            });
            RESOLVER.with(|r| {
                *r.borrow_mut() = Some(resolver);
            });
            0
        }
        Err(_) => -1
    }
}

/// Add a template to the engine's memory resolver.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// - `path` must be a valid pointer to a null-terminated UTF-8 string, or null.
/// - `source` must be a valid pointer to a null-terminated UTF-8 string, or null.
/// - The memory pointed to by `path` and `source` must be valid for reads
///   until this function returns.
#[no_mangle]
pub unsafe extern "C" fn luat_add_template(path: *const c_char, source: *const c_char) -> i32 {
    if path.is_null() || source.is_null() {
        return -1;
    }

    let path_str = unsafe {
        match CStr::from_ptr(path).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let source_str = unsafe {
        match CStr::from_ptr(source).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    RESOLVER.with(|r| {
        if let Some(resolver) = r.borrow().as_ref() {
            resolver.add_template(path_str, source_str.to_string());
            0
        } else {
            -1
        }
    })
}

/// Remove a template from the engine's memory resolver.
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// - `path` must be a valid pointer to a null-terminated UTF-8 string, or null.
/// - The memory pointed to by `path` must be valid for reads until this function returns.
#[no_mangle]
pub unsafe extern "C" fn luat_remove_template(path: *const c_char) -> i32 {
    if path.is_null() {
        return -1;
    }

    let path_str = unsafe {
        match CStr::from_ptr(path).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    RESOLVER.with(|r| {
        if let Some(resolver) = r.borrow().as_ref() {
            resolver.remove_template(path_str);
            0
        } else {
            -1
        }
    })
}

/// Clear all templates from the engine's memory resolver.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn luat_clear_templates() -> i32 {
    RESOLVER.with(|r| {
        if let Some(resolver) = r.borrow().as_ref() {
            resolver.clear();
            0
        } else {
            -1
        }
    });

    // Also clear the engine cache and Lua module cache
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow().as_ref() {
            let _ = engine.clear_cache();
            let _ = engine.clear_lua_module_cache();
        }
    });

    0
}

/// Render a template with the given context (JSON string).
/// Returns a pointer to the rendered HTML string, or null on error.
/// The caller must free the returned string with luat_free_string.
///
/// # Safety
///
/// - `entry` must be a valid pointer to a null-terminated UTF-8 string, or null.
/// - `context_json` must be a valid pointer to a null-terminated UTF-8 JSON string, or null.
/// - The memory pointed to by `entry` and `context_json` must be valid for reads
///   until this function returns.
/// - The returned pointer must be freed by calling `luat_free_string`.
#[no_mangle]
pub unsafe extern "C" fn luat_render(entry: *const c_char, context_json: *const c_char) -> *mut c_char {
    if entry.is_null() || context_json.is_null() {
        return std::ptr::null_mut();
    }

    let entry_str = unsafe {
        match CStr::from_ptr(entry).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let context_str = unsafe {
        match CStr::from_ptr(context_json).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        }
    };

    // Parse JSON context
    let context_value: serde_json::Value = match serde_json::from_str(context_str) {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };

    ENGINE.with(|e| {
        let engine_ref = e.borrow();
        let engine = match engine_ref.as_ref() {
            Some(eng) => eng,
            None => return std::ptr::null_mut(),
        };

        // Compile the entry template
        let module = match engine.compile_entry(entry_str) {
            Ok(m) => m,
            Err(_) => return std::ptr::null_mut(),
        };

        // Convert JSON to Lua value
        let context = match engine.to_value(context_value) {
            Ok(c) => c,
            Err(_) => return std::ptr::null_mut(),
        };

        // Render
        match engine.render(&module, &context) {
            Ok(result) => {
                match CString::new(result) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                }
            }
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Render a template and return detailed result as JSON.
/// Returns JSON with { "success": bool, "html": string|null, "error": string|null }
/// The caller must free the returned string with luat_free_string.
///
/// # Safety
///
/// - `entry` must be a valid pointer to a null-terminated UTF-8 string, or null.
/// - `context_json` must be a valid pointer to a null-terminated UTF-8 JSON string, or null.
/// - The memory pointed to by `entry` and `context_json` must be valid for reads
///   until this function returns.
/// - The returned pointer must be freed by calling `luat_free_string`.
#[no_mangle]
pub unsafe extern "C" fn luat_render_with_error(entry: *const c_char, context_json: *const c_char) -> *mut c_char {
    if entry.is_null() || context_json.is_null() {
        let error_json = serde_json::json!({
            "success": false,
            "html": null,
            "error": "Invalid arguments: entry or context is null"
        });
        return match CString::new(error_json.to_string()) {
            Ok(c_str) => c_str.into_raw(),
            Err(_) => std::ptr::null_mut(),
        };
    }

    let entry_str = unsafe {
        match CStr::from_ptr(entry).to_str() {
            Ok(s) => s,
            Err(e) => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": format!("Invalid entry path encoding: {}", e)
                });
                return match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                };
            }
        }
    };

    let context_str = unsafe {
        match CStr::from_ptr(context_json).to_str() {
            Ok(s) => s,
            Err(e) => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": format!("Invalid context encoding: {}", e)
                });
                return match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                };
            }
        }
    };

    // Parse JSON context
    let context_value: serde_json::Value = match serde_json::from_str(context_str) {
        Ok(v) => v,
        Err(e) => {
            let error_json = serde_json::json!({
                "success": false,
                "html": null,
                "error": format!("Invalid JSON context: {}", e)
            });
            return match CString::new(error_json.to_string()) {
                Ok(c_str) => c_str.into_raw(),
                Err(_) => std::ptr::null_mut(),
            };
        }
    };

    ENGINE.with(|e| {
        let engine_ref = e.borrow();
        let engine = match engine_ref.as_ref() {
            Some(eng) => eng,
            None => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": "Engine not initialized. Call luat_init() first."
                });
                return match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                };
            }
        };

        // Compile the entry template
        let module = match engine.compile_entry(entry_str) {
            Ok(m) => m,
            Err(e) => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": format!("Compilation error: {}", e)
                });
                return match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                };
            }
        };

        // Convert JSON to Lua value
        let context = match engine.to_value(context_value) {
            Ok(c) => c,
            Err(e) => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": format!("Context conversion error: {}", e)
                });
                return match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                };
            }
        };

        // Render
        match engine.render(&module, &context) {
            Ok(result) => {
                let success_json = serde_json::json!({
                    "success": true,
                    "html": result,
                    "error": null
                });
                match CString::new(success_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                }
            }
            Err(e) => {
                let error_json = serde_json::json!({
                    "success": false,
                    "html": null,
                    "error": format!("Render error: {}", e)
                });
                match CString::new(error_json.to_string()) {
                    Ok(c_str) => c_str.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                }
            }
        }
    })
}

/// Free a string allocated by luat_render or luat_render_with_error.
///
/// # Safety
///
/// - `ptr` must be either null or a valid pointer previously returned by
///   `luat_render`, `luat_render_with_error`, or `luat_run_tests`.
/// - `ptr` must not have been freed before.
/// - After calling this function, `ptr` is invalid and must not be used.
#[no_mangle]
pub unsafe extern "C" fn luat_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// Get the version of the Luat library.
/// Returns a pointer to the version string. The caller must NOT free this string.
#[no_mangle]
pub extern "C" fn luat_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

/// Test result structure
struct TestResult {
    name: &'static str,
    passed: bool,
    error: Option<String>,
}

/// Create engine with templates
fn create_engine_with_templates(templates: &[(&str, &str)]) -> Engine<MemoryResourceResolver> {
    let resolver = MemoryResourceResolver::new();
    for (path, content) in templates {
        resolver.add_template(path, content.to_string());
    }
    let cache = Box::new(MemoryCache::new(100));
    Engine::new(resolver, cache).expect("Failed to create engine")
}

// ============================================================================
// Test Functions
// ============================================================================

fn test_compile_simple_template() -> TestResult {
    let name = "compile_simple_template";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<div>Hello World</div>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            if module.lua_code.is_empty() {
                TestResult { name, passed: false, error: Some("Lua code is empty".to_string()) }
            } else {
                TestResult { name, passed: true, error: None }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_render_simple_html() -> TestResult {
    let name = "render_simple_html";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<div>Hello World</div>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({})).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Hello World") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Unexpected output: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_render_with_props() -> TestResult {
    let name = "render_with_props";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<h1>Hello, {props.name}!</h1>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({ "name": "World" })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Hello, World!") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Unexpected output: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_if_block() -> TestResult {
    let name = "if_block";
    let engine = create_engine_with_templates(&[
        ("test.luat", "{#if props.show}<p>Visible</p>{/if}"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({ "show": true })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Visible") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Expected 'Visible', got: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_each_block() -> TestResult {
    let name = "each_block";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<ul>{#each props.items as item}<li>{item}</li>{/each}</ul>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({
                "items": ["Apple", "Banana", "Cherry"]
            })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Apple") && result.contains("Banana") && result.contains("Cherry") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Missing items in output: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_component_import() -> TestResult {
    let name = "component_import";
    let engine = create_engine_with_templates(&[
        ("Button.luat", r#"<button class="btn">{@render props.children?.()}</button>"#),
        ("main.luat", r#"<script>
local Button = require("Button")
</script>
<Button>Click me</Button>"#),
    ]);

    match engine.compile_entry("main.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({})).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("btn") && result.contains("Click me") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Missing button content: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_script_block() -> TestResult {
    let name = "script_block";
    let engine = create_engine_with_templates(&[
        ("test.luat", r#"<script>
local greeting = "Hello from Lua"
</script>
<p>{greeting}</p>"#),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({})).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Hello from Lua") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Missing script output: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_html_escaping() -> TestResult {
    let name = "html_escaping";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<p>{props.text}</p>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({
                "text": "<script>alert('xss')</script>"
            })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if !result.contains("<script>") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some("Script tag not escaped".to_string()) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_nested_props() -> TestResult {
    let name = "nested_props";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<p>{props.user.name} - {props.user.email}</p>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({
                "user": {
                    "name": "Alice",
                    "email": "alice@example.com"
                }
            })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("Alice") && result.contains("alice@example.com") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Missing nested props: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

fn test_arithmetic_expression() -> TestResult {
    let name = "arithmetic_expression";
    let engine = create_engine_with_templates(&[
        ("test.luat", "<p>{props.a + props.b}</p>"),
    ]);

    match engine.compile_entry("test.luat") {
        Ok(module) => {
            let context = engine.to_value(serde_json::json!({ "a": 5, "b": 3 })).unwrap();
            match engine.render(&module, &context) {
                Ok(result) => {
                    if result.contains("8") {
                        TestResult { name, passed: true, error: None }
                    } else {
                        TestResult { name, passed: false, error: Some(format!("Expected 8, got: {}", result)) }
                    }
                }
                Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
            }
        }
        Err(e) => TestResult { name, passed: false, error: Some(format!("{}", e)) }
    }
}

// ============================================================================
// Main - Empty for library use (tests run via luat_run_tests)
// ============================================================================

fn main() {
    // Empty - library mode. Call luat_run_tests() to run tests.
}

/// Run all tests and return JSON results.
/// Returns a C string with JSON results (caller must free with luat_free_string).
#[no_mangle]
pub extern "C" fn luat_run_tests() -> *mut c_char {
    let tests: Vec<fn() -> TestResult> = vec![
        test_compile_simple_template,
        test_render_simple_html,
        test_render_with_props,
        test_if_block,
        test_each_block,
        test_component_import,
        test_script_block,
        test_html_escaping,
        test_nested_props,
        test_arithmetic_expression,
    ];

    let mut passed = 0;
    let mut failed = 0;
    let mut results = Vec::new();

    for test_fn in tests {
        let result = test_fn();
        if result.passed {
            passed += 1;
            println!("✓ {}", result.name);
        } else {
            failed += 1;
            println!("✗ {} - {}", result.name, result.error.as_ref().unwrap_or(&"Unknown error".to_string()));
        }
        results.push(result);
    }

    println!("\n{} passed, {} failed", passed, failed);

    // Output JSON for programmatic parsing
    println!("\n--- JSON RESULTS ---");
    let json_results: Vec<_> = results.iter().map(|r| {
        serde_json::json!({
            "name": r.name,
            "passed": r.passed,
            "error": r.error
        })
    }).collect();
    let json_output = serde_json::to_string_pretty(&serde_json::json!({
        "passed": passed,
        "failed": failed,
        "tests": json_results
    })).unwrap();
    println!("{}", json_output);

    // Return JSON as C string
    match CString::new(json_output) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
