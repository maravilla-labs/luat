// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `$name(...)` in a `<script>` is a magic function only in code; inside a
//! string or a comment it is text and stays as written.

use std::collections::HashMap;

use luat::memory_resolver::MemoryResourceResolver;
use luat::Engine;

fn render(template: &str) -> String {
    let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 4).unwrap();
    engine.render_source(template, &HashMap::new()).unwrap()
}

#[test]
fn dollar_calls_in_strings_stay_as_written() {
    assert_eq!(
        render("<script>local a = \"$nextTick(() => open = true)\"</script><b x-init={a}></b>"),
        "<b x-init=\"$nextTick(() =&gt; open = true)\"></b>"
    );
    assert_eq!(
        render("<script>local a = '$refs.x.focus(); $dispatch(\\'go\\')'</script>{a}"),
        "$refs.x.focus(); $dispatch(&#39;go&#39;)"
    );
    assert_eq!(
        render("<script>local a = [==[$watch(open, () => {})]==]</script>{a}"),
        "$watch(open, () =&gt; {})"
    );
}

#[test]
fn dollar_calls_in_comments_are_left_alone() {
    assert_eq!(
        render("<script>-- $nextTick(a, b)\n--[[ $state(1) ]] local a = 2</script>{a}"),
        "2"
    );
}

#[test]
fn magic_functions_in_code_still_apply() {
    assert_eq!(
        render("<script>local s = \"$state(9)\"; local n = $state(1)</script>{s}{n}"),
        "$state(9)1"
    );
}

#[test]
fn json_null_becomes_nil() {
    let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 4).unwrap();
    let value = engine
        .to_value(serde_json::json!({ "a": null, "b": { "c": null }, "d": 1 }))
        .unwrap();
    let check: mlua::Function = engine
        .lua()
        .load("return function(t) return t.a == nil, t.b.c == nil, (t.a or 'x'), t.d end")
        .eval()
        .unwrap();
    let (a, c, fallback, d): (bool, bool, String, i64) = check.call(value).unwrap();
    assert!(a && c);
    assert_eq!(fallback, "x");
    assert_eq!(d, 1);
}

#[test]
fn json_null_in_an_array_keeps_the_positions() {
    let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 4).unwrap();
    let value = engine.to_value(serde_json::json!([1, null, 3])).unwrap();
    let check: mlua::Function = engine
        .lua()
        .load("return function(t) return t[1], t[2] == nil, t[3] end")
        .eval()
        .unwrap();
    let (a, hole, c): (i64, bool, i64) = check.call(value).unwrap();
    assert_eq!((a, hole, c), (1, true, 3));
}
