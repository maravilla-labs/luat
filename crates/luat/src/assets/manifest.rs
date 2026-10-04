// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! The asset manifest: entry source paths → built URLs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::codegen::escape_lua_string;

/// The files one entry produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    /// URL of the entry's own output.
    pub file: String,
    /// Stylesheets the entry pulls in (CSS imported from JavaScript).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css: Vec<String>,
    /// Chunks the entry imports statically, worth preloading.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
}

/// Entry source paths (`src/client/app.js`) mapped to what they produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetManifest {
    entries: BTreeMap<String, AssetEntry>,
}

impl AssetManifest {
    /// Adds or replaces an entry.
    pub fn insert(&mut self, source: String, entry: AssetEntry) {
        self.entries.insert(source, entry);
    }

    /// The entry built from `source`.
    pub fn get(&self, source: &str) -> Option<&AssetEntry> {
        self.entries.get(source.trim_start_matches("./"))
    }

    /// All entries, by source path.
    pub fn entries(&self) -> &BTreeMap<String, AssetEntry> {
        &self.entries
    }

    /// True when nothing was built.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Markup for `%luat.head%`: stylesheets, preloads for imported chunks
    /// and module scripts for every entry.
    pub fn head_tags(&self) -> String {
        let mut styles = Vec::new();
        let mut preloads = Vec::new();
        let mut scripts = Vec::new();
        for entry in self.entries.values() {
            if entry.file.ends_with(".css") {
                styles.push(entry.file.as_str());
            } else {
                scripts.push(entry.file.as_str());
            }
            styles.extend(entry.css.iter().map(String::as_str));
            preloads.extend(entry.imports.iter().map(String::as_str));
        }
        let mut seen = std::collections::HashSet::new();
        let mut out = String::new();
        for href in styles.into_iter().filter(|h| seen.insert(*h)) {
            out.push_str(&format!("<link rel=\"stylesheet\" href=\"{}\">\n", attr(href)));
        }
        for href in preloads.into_iter().filter(|h| seen.insert(*h)) {
            out.push_str(&format!("<link rel=\"modulepreload\" href=\"{}\">\n", attr(href)));
        }
        for src in scripts.into_iter().filter(|h| seen.insert(*h)) {
            out.push_str(&format!("<script type=\"module\" src=\"{}\"></script>\n", attr(src)));
        }
        out
    }

    /// Lua for the bundle: the `__assets` table, the `__assets_head` markup
    /// and the global `asset(source)`, which returns an entry's URL and
    /// fails for unknown entries.
    pub fn to_lua(&self) -> String {
        let q = |s: &str| format!("\"{}\"", escape_lua_string(s));
        let list = |items: &[String]| items.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ");
        let mut lua = String::from("-- Client assets (hashed files)\n__assets = {\n");
        for (source, entry) in &self.entries {
            lua.push_str(&format!(
                "  [{}] = {{ file = {}, css = {{ {} }}, imports = {{ {} }} }},\n",
                q(source),
                q(&entry.file),
                list(&entry.css),
                list(&entry.imports)
            ));
        }
        lua.push_str("}\n");
        lua.push_str(&format!("__assets_head = {}\n", q(&self.head_tags())));
        lua.push_str(ASSET_FUNCTION);
        lua
    }
}

/// `asset(source)`: the URL built from `source`.
pub(crate) const ASSET_FUNCTION: &str = r#"function asset(source)
  local entry = __assets and __assets[(tostring(source):gsub("^%./", ""))]
  if not entry then
    error("unknown asset '" .. tostring(source) .. "' (not an entry in [frontend] entries)", 2)
  end
  return entry.file
end
"#;

fn attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AssetManifest {
        let mut m = AssetManifest::default();
        m.insert(
            "src/client/app.js".into(),
            AssetEntry {
                file: "/_luat/immutable/app-AAAA.js".into(),
                css: vec!["/_luat/immutable/app-BBBB.css".into()],
                imports: vec!["/_luat/immutable/chunks/x-CCCC.js".into()],
            },
        );
        m.insert(
            "src/styles/site.css".into(),
            AssetEntry { file: "/_luat/immutable/site-DDDD.css".into(), ..Default::default() },
        );
        m
    }

    #[test]
    fn head_tags_list_styles_preloads_and_scripts() {
        assert_eq!(
            sample().head_tags(),
            "<link rel=\"stylesheet\" href=\"/_luat/immutable/app-BBBB.css\">\n\
             <link rel=\"stylesheet\" href=\"/_luat/immutable/site-DDDD.css\">\n\
             <link rel=\"modulepreload\" href=\"/_luat/immutable/chunks/x-CCCC.js\">\n\
             <script type=\"module\" src=\"/_luat/immutable/app-AAAA.js\"></script>\n"
        );
    }

    #[test]
    fn lua_asset_function_resolves_entries() {
        let lua = mlua::Lua::new();
        lua.load(sample().to_lua()).exec().unwrap();
        let url: String = lua.load("return asset('./src/client/app.js')").eval().unwrap();
        assert_eq!(url, "/_luat/immutable/app-AAAA.js");
        let err = lua.load("return asset('nope.js')").eval::<String>().unwrap_err();
        assert!(err.to_string().contains("unknown asset 'nope.js'"));
        let head: String = lua.load("return __assets_head").eval().unwrap();
        assert!(head.contains("modulepreload"));
    }
}
