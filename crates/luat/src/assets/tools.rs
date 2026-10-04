// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Locating build tools a project installed itself.

use std::path::{Path, PathBuf};

/// The project's own copy of `tool` (`esbuild`, `tailwindcss`) from
/// `node_modules/.bin`, when it has one.
pub fn find_project_tool(project_dir: &Path, tool: &str) -> Option<PathBuf> {
    let bin = project_dir.join("node_modules").join(".bin");
    let names: &[String] = if cfg!(windows) {
        &[format!("{tool}.cmd"), format!("{tool}.exe")]
    } else {
        &[tool.to_string()]
    };
    names.iter().map(|n| bin.join(n)).find(|p| p.is_file())
}
