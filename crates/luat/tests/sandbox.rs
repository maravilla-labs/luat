// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Guest code must not be able to escape the sandbox: no dynamic code
//! loading, no filesystem or process access, no way to reach the original
//! libraries through `require`, and no tampering with state shared by every
//! string.

use luat::memory_resolver::MemoryResourceResolver;
use luat::Engine;

fn engine() -> Engine<MemoryResourceResolver> {
    Engine::with_memory_cache(MemoryResourceResolver::new(), 8).unwrap()
}

/// Evaluates `expr` as guest code and returns its result.
fn guest_eval<T: mlua::FromLua>(engine: &Engine<MemoryResourceResolver>, expr: &str) -> T {
    engine
        .lua()
        .load(format!("return {expr}"))
        .set_name("=guest")
        .eval()
        .unwrap_or_else(|e| panic!("evaluating `{expr}` failed: {e}"))
}

fn guest_errors(engine: &Engine<MemoryResourceResolver>, code: &str) -> String {
    match engine.lua().load(code).set_name("=guest").exec() {
        Ok(()) => panic!("`{code}` was expected to fail"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn dynamic_code_loading_is_unavailable() {
    let engine = engine();
    for name in ["load", "loadstring", "loadfile", "dofile", "__luat_internal_load"] {
        let present: bool = guest_eval(&engine, &format!("rawget(_G, {name:?}) ~= nil"));
        assert!(!present, "{name} must not be reachable from guest code");
    }
}

#[test]
fn io_debug_and_process_access_are_unavailable() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "io == nil"));
    assert!(guest_eval::<bool>(&engine, "debug == nil"));
    assert!(guest_eval::<bool>(&engine, "os.execute == nil"));
    assert!(guest_eval::<bool>(&engine, "os.getenv == nil"));
    assert!(guest_eval::<bool>(&engine, "os.remove == nil"));
    assert!(guest_eval::<bool>(&engine, "type(os.time()) == 'number'"));
}

#[test]
fn require_cannot_return_original_libraries() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "require('os').execute == nil"));
    assert!(guest_eval::<bool>(&engine, "package.loaded.io == nil"));
    assert!(guest_eval::<bool>(&engine, "package.loaded.debug == nil"));
    let err = guest_errors(&engine, "require('io')");
    assert!(err.contains("io"), "unexpected error: {err}");
}

#[test]
fn require_cannot_search_the_filesystem_or_load_c_modules() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "package.path == ''"));
    assert!(guest_eval::<bool>(&engine, "package.cpath == ''"));
    assert!(guest_eval::<bool>(&engine, "package.loadlib == nil"));
    assert!(guest_eval::<bool>(&engine, "package.searchpath == nil"));
    // Only the preload searcher plus the engine's two resolver searchers.
    assert_eq!(guest_eval::<i64>(&engine, "#package.searchers"), 3);
}

#[test]
fn preloaded_modules_still_resolve() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "type(require('json').encode) == 'function'"));
}

#[test]
fn string_metatable_is_locked() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "getmetatable('') == false"));
    let err = guest_errors(&engine, "setmetatable('', {})");
    assert!(err.contains("table expected"), "unexpected error: {err}");
}

#[test]
fn garbage_collector_cannot_be_stopped() {
    let engine = engine();
    assert!(guest_eval::<bool>(&engine, "type(collectgarbage('count')) == 'number'"));
    let err = guest_errors(&engine, "collectgarbage('stop')");
    assert!(err.contains("only \"count\""), "unexpected error: {err}");
}

#[test]
fn bundles_can_still_load_server_sources() {
    let engine = engine();
    let bundle = r#"
        local __load = ...
        __server_sources = { ["lib/util.lua"] = "return { answer = 42 }" }
        local fn = __load(__server_sources["lib/util.lua"], "@lib/util.lua")
        bundled_answer = fn().answer
    "#;
    engine.preload_bundle_code(bundle).unwrap();
    assert_eq!(guest_eval::<i64>(&engine, "bundled_answer"), 42);
}
