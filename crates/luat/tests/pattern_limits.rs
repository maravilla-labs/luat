// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Pattern matching runs natively, outside the reach of the instruction
//! hook, so the matcher charges its own work to the engine's limits. A
//! pattern that backtracks exponentially must fail with a normal limit
//! error, quickly, and guest code must not be able to swallow it.

use std::time::{Duration, Instant};

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, EngineLimits, LimitExceeded, LuatError};

/// Backtracks exponentially: about 2^40 steps in reference Lua.
const EVIL: &str = r#"local s, p = string.rep("a", 40), string.rep("a*", 40) .. "b""#;

fn engine() -> Engine<MemoryResourceResolver> {
    Engine::with_memory_cache(MemoryResourceResolver::new(), 8).unwrap()
}

fn budget(n: u64) -> EngineLimits {
    EngineLimits {
        instruction_budget: Some(n),
        ..Default::default()
    }
}

fn deadline_in(ms: u64) -> EngineLimits {
    EngineLimits {
        deadline: Some(Instant::now() + Duration::from_millis(ms)),
        ..Default::default()
    }
}

#[allow(clippy::result_large_err)]
fn run(engine: &Engine<MemoryResourceResolver>, code: &str) -> Result<String, LuatError> {
    engine
        .lua()
        .load(code)
        .set_name("=guest")
        .eval::<Option<String>>()
        .map(Option::unwrap_or_default)
        .map_err(LuatError::LuaError)
}

/// Runs `body` after the EVIL setup and expects `limit` to stop it fast.
fn assert_stopped(limits: EngineLimits, body: &str, limit: LimitExceeded) {
    let engine = engine();
    engine.set_limits(&limits).unwrap();
    let started = Instant::now();
    let result = run(&engine, &format!("{EVIL}\n{body}"));
    let elapsed = started.elapsed();
    let err = result.expect_err("pathological pattern must not complete");
    assert_eq!(LimitExceeded::from_error(&err), Some(limit), "{err}");
    assert_eq!(engine.limit_exceeded(), Some(limit));
    assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
}

#[test]
fn deadline_stops_backtracking_find() {
    assert_stopped(
        deadline_in(100),
        "string.find(s, p)",
        LimitExceeded::Deadline,
    );
}

#[test]
fn budget_stops_backtracking_find() {
    assert_stopped(
        budget(1_000_000),
        "string.find(s, p)",
        LimitExceeded::Instructions,
    );
}

#[test]
fn every_pattern_function_is_limited() {
    for body in [
        "string.match(s, p)",
        "s:find(p)",
        "s:match(p)",
        "string.gsub(s, p, 'x')",
        "s:gsub(p, 'x')",
        "for _ in string.gmatch(s, p) do end",
        "for _ in s:gmatch(p) do end",
    ] {
        assert_stopped(budget(1_000_000), body, LimitExceeded::Instructions);
        assert_stopped(deadline_in(50), body, LimitExceeded::Deadline);
    }
}

#[test]
fn pcall_cannot_swallow_a_pattern_limit() {
    let body = r#"
        local ok, err = pcall(string.find, s, p)
        local n = 0
        for i = 1, 100 do n = n + i end
        return "survived " .. tostring(err)
    "#;
    assert_stopped(budget(1_000_000), body, LimitExceeded::Instructions);
    assert_stopped(deadline_in(50), body, LimitExceeded::Deadline);
}

#[test]
fn retrying_in_a_loop_stays_stopped() {
    let body = "while true do pcall(string.find, s, p) end";
    assert_stopped(budget(1_000_000), body, LimitExceeded::Instructions);
    assert_stopped(deadline_in(50), body, LimitExceeded::Deadline);
}

#[test]
fn coroutines_are_limited_too() {
    let body = "coroutine.wrap(function() pcall(string.find, s, p) return 'survived' end)()";
    assert_stopped(budget(1_000_000), body, LimitExceeded::Instructions);
}

#[test]
fn match_steps_use_the_same_budget_as_instructions() {
    // Each call does ~500k steps; four of them exceed a budget of 1M even
    // though the Lua code itself runs only a handful of instructions.
    let engine = engine();
    engine.set_limits(&budget(1_000_000)).unwrap();
    let code = r#"
        local s = string.rep("a", 1000)
        for i = 1, 4 do string.find(s, ".-b") end
    "#;
    let err = run(&engine, code).unwrap_err();
    assert_eq!(
        LimitExceeded::from_error(&err),
        Some(LimitExceeded::Instructions)
    );
}

#[test]
fn normal_patterns_run_within_limits() {
    let engine = engine();
    engine
        .set_limits(&EngineLimits {
            instruction_budget: Some(5_000_000),
            deadline: Some(Instant::now() + Duration::from_secs(30)),
            memory_bytes: Some(64 * 1024 * 1024),
        })
        .unwrap();
    let code = r#"
        local words = {}
        for i = 1, 2000 do words[i] = "word" .. i end
        local text = table.concat(words, " ")
        local count = select(2, text:gsub("%w+", "<%0>"))
        local n = 0
        for w in text:gmatch("%a+%d+") do n = n + 1 end
        local key, value = ("name = luat"):match("(%w+)%s*=%s*(%w+)")
        return count .. " " .. n .. " " .. key .. "=" .. value .. " " .. text:find("word2000", 1, true)
    "#;
    assert_eq!(run(&engine, code).unwrap(), "2000 2000 name=luat 16885");
    assert_eq!(engine.limit_exceeded(), None);
}

#[test]
fn unlimited_engine_matches_normally() {
    let engine = engine();
    let code = "return (string.gsub('hello world', '(%w+)', '<%1>'))";
    assert_eq!(run(&engine, code).unwrap(), "<hello> <world>");
}

#[test]
fn gsub_result_is_bounded_by_the_memory_limit() {
    // 10^5 matches times a 10^4-byte replacement would be 1 GB.
    let engine = engine();
    engine
        .set_limits(&EngineLimits {
            memory_bytes: Some(16 * 1024 * 1024),
            ..Default::default()
        })
        .unwrap();
    let code = r#"return (string.gsub(string.rep("a", 100000), ".", string.rep("b", 10000)))"#;
    let err = run(&engine, code).unwrap_err();
    assert_eq!(
        LimitExceeded::from_error(&err),
        Some(LimitExceeded::Memory),
        "{err}"
    );
}

#[test]
fn pattern_errors_are_strings_with_the_caller_position() {
    let engine = engine();
    let code = r#"
        local ok, err = pcall(function()
            local i = string.find("abc", "[a")
            return i
        end)
        return type(err) .. " " .. err
    "#;
    assert_eq!(
        run(&engine, code).unwrap(),
        "string guest:3: malformed pattern (missing ']')"
    );
}
