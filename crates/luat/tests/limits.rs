// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Execution limits must stop guest code wherever it runs: on the main
//! thread, inside coroutines created by `respond_async` or by guest code,
//! and even when guest code tries to swallow the error with `pcall`.

use std::time::{Duration, Instant};

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, EngineLimits, LimitExceeded, LuatError, LuatRequest, Router};

fn engine() -> Engine<MemoryResourceResolver> {
    Engine::with_memory_cache(MemoryResourceResolver::new(), 8).unwrap()
}

fn budget(n: u64) -> EngineLimits {
    EngineLimits {
        instruction_budget: Some(n),
        ..Default::default()
    }
}

#[allow(clippy::result_large_err)]
fn run(engine: &Engine<MemoryResourceResolver>, code: &str) -> Result<(), LuatError> {
    engine
        .lua()
        .load(code)
        .set_name("=guest")
        .exec()
        .map_err(LuatError::LuaError)
}

#[test]
fn infinite_loop_stops_at_instruction_budget() {
    let engine = engine();
    engine.set_limits(&budget(100_000)).unwrap();

    let err = run(&engine, "while true do end").unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));
    assert_eq!(engine.limit_exceeded(), Some(LimitExceeded::Instructions));
}

#[test]
fn pcall_cannot_swallow_the_limit() {
    let engine = engine();
    engine.set_limits(&budget(100_000)).unwrap();

    let err = run(
        &engine,
        "while true do pcall(function() while true do end end) end",
    )
    .unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));
}

#[test]
fn guest_coroutines_are_limited() {
    let engine = engine();
    engine.set_limits(&budget(100_000)).unwrap();

    let err = run(
        &engine,
        "local co = coroutine.wrap(function() while true do end end); co()",
    )
    .unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));
}

#[test]
fn deadline_interrupts_running_code() {
    let engine = engine();
    engine
        .set_limits(&EngineLimits {
            deadline: Some(Instant::now() + Duration::from_millis(50)),
            ..Default::default()
        })
        .unwrap();

    let started = Instant::now();
    let err = run(&engine, "while true do end").unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Deadline));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
}

#[test]
fn memory_limit_stops_allocation() {
    let engine = engine();
    engine
        .set_limits(&EngineLimits {
            memory_bytes: Some(8 * 1024 * 1024),
            ..Default::default()
        })
        .unwrap();

    let err = run(
        &engine,
        "local t = {} for i = 1, 1e9 do t[i] = string.rep('x', 1024) .. i end",
    )
    .unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Memory));
}

#[test]
fn unlimited_engine_runs_long_loops() {
    let engine = engine();
    run(&engine, "local n = 0 for i = 1, 2000000 do n = n + i end").unwrap();
    assert_eq!(engine.limit_exceeded(), None);
}

#[test]
fn work_within_budget_completes() {
    let engine = engine();
    engine.set_limits(&budget(10_000_000)).unwrap();
    run(&engine, "local n = 0 for i = 1, 10000 do n = n + i end").unwrap();
    assert_eq!(engine.limit_exceeded(), None);
}

#[test]
fn engines_are_limited_independently() {
    let limited = engine();
    let free = engine();
    limited.set_limits(&budget(50_000)).unwrap();

    assert!(run(&limited, "while true do end").is_err());
    run(&free, "local n = 0 for i = 1, 1000000 do n = n + i end").unwrap();
    assert_eq!(free.limit_exceeded(), None);
}

#[test]
fn tripped_engine_refuses_new_limits() {
    let engine = engine();
    engine.set_limits(&budget(50_000)).unwrap();
    assert!(run(&engine, "while true do end").is_err());
    assert!(engine.set_limits(&budget(50_000)).is_err());
}

#[cfg(feature = "async-lua")]
#[tokio::test]
async fn async_load_is_limited() {
    let resolver = MemoryResourceResolver::new();
    resolver.add_template("+page.luat", "<p>never</p>".to_string());
    resolver.add_template(
        "+page.server.lua",
        "function load(ctx) while true do end end".to_string(),
    );
    let engine = Engine::with_memory_cache(resolver, 8).unwrap();
    engine.set_limits(&budget(100_000)).unwrap();

    let router = Router::from_paths(["+page.luat", "+page.server.lua"].into_iter());
    let route = router.match_url("/").unwrap();
    let err = engine
        .respond_async(&route, &LuatRequest::new("/", "GET"))
        .await
        .unwrap_err();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));
}
