// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Whitespace between two pieces of content is significant; whitespace at
//! the start and end of a block is not.

use std::collections::HashMap;

use luat::memory_resolver::MemoryResourceResolver;
use luat::Engine;

fn render(template: &str) -> String {
    let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 4).unwrap();
    let lua = engine.lua();
    let mut ctx = HashMap::new();
    ctx.insert("a".to_string(), mlua::Value::String(lua.create_string("x").unwrap()));
    ctx.insert("b".to_string(), mlua::Value::String(lua.create_string("y").unwrap()));
    engine.render_source(template, &ctx).unwrap()
}

#[test]
fn space_between_expressions_is_kept() {
    assert_eq!(render("<p>{props.a} {props.b}</p>"), "<p>x y</p>");
}

#[test]
fn whitespace_between_elements_collapses_to_one_space() {
    assert_eq!(render("<ul>\n  <li>1</li>\n  <li>2</li>\n</ul>"), "<ul><li>1</li> <li>2</li></ul>");
}

#[test]
fn leading_and_trailing_whitespace_is_dropped() {
    assert_eq!(render("\n  <p>{props.a}</p>\n"), "<p>x</p>");
}
