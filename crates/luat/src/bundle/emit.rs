// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Lua source for the data tables a bundle carries.

use std::collections::BTreeMap;

use crate::codegen::escape_lua_string;

/// A Lua long-string literal holding `text` verbatim.
///
/// The bracket level is chosen so the closing bracket cannot occur inside
/// the text (a fixed `[=[ ... ]=]` breaks on sources containing `]=]`).
/// A leading newline is added because Lua drops the first newline of a long
/// string.
pub(crate) fn long_string(text: &str) -> String {
    let mut level = 0;
    while text.contains(&format!("]{}]", "=".repeat(level))) {
        level += 1;
    }
    let eq = "=".repeat(level);
    format!("[{eq}[\n{text}]{eq}]")
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", escape_lua_string(s))
}

/// `__server_sources = { [name] = source, ... }`: raw Lua sources run at
/// request time.
pub(crate) fn server_sources(sources: &[(String, String)]) -> String {
    let mut lua = String::from("-- Server and library sources (run at request time)\n__server_sources = {\n");
    for (name, source) in sources {
        lua.push_str(&format!("  [{}] = {},\n", quoted(name), long_string(source)));
    }
    lua.push_str("}\n");
    lua
}

/// `__require_map = { [importer] = { [name] = module_key } }`: requires
/// resolved at build time.
pub(crate) fn require_map(map: &BTreeMap<String, BTreeMap<String, String>>) -> String {
    let mut lua = String::from("-- Requires resolved at build time\n__require_map = {\n");
    for (module, deps) in map {
        lua.push_str(&format!("  [{}] = {{\n", quoted(module)));
        for (name, resolved) in deps {
            lua.push_str(&format!("    [{}] = {},\n", quoted(name), quoted(resolved)));
        }
        lua.push_str("  },\n");
    }
    lua.push_str("}\n");
    lua
}

/// `__route_files = { ... }`: route file paths, relative to the routes
/// directory, from which hosts rebuild the router.
pub(crate) fn route_files(files: &[String]) -> String {
    let mut lua = String::from("-- Route files (the router is rebuilt from these)\n__route_files = {\n");
    for file in files {
        lua.push_str(&format!("  {},\n", quoted(file)));
    }
    lua.push_str("}\n");
    lua
}

/// `__app_html = [[...]]`: the app shell, when the project has one.
pub(crate) fn app_html(html: Option<&str>) -> String {
    match html {
        Some(html) => format!("__app_html = {}\n", long_string(html)),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_string(lua_expr: &str) -> String {
        mlua::Lua::new().load(format!("return {lua_expr}")).eval().unwrap()
    }

    #[test]
    fn long_strings_survive_closing_brackets() {
        for text in ["plain", "a ]] b", "a ]=] b ]==] c", "ends with ]=", "\nleading newline"] {
            assert_eq!(eval_string(&long_string(text)), text);
        }
    }

    #[test]
    fn server_sources_round_trip() {
        let lua = mlua::Lua::new();
        let code = server_sources(&[("a/\"x\".lua".to_string(), "return ']=]'".to_string())]);
        lua.load(&code).exec().unwrap();
        let src: String = lua.load("return __server_sources['a/\"x\".lua']").eval().unwrap();
        assert_eq!(src, "return ']=]'");
    }
}
