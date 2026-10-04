// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Build script for conditional WASM client artifact embedding.
//!
//! If the WASM client has been built (via `make wasm-client-release`), this
//! embeds the artifacts directly into the CLI binary so that `cargo install`
//! produces a self-contained binary with hybrid mode support.
//!
//! If the WASM artifacts don't exist, stubs returning `None` are generated
//! and the CLI falls back to filesystem lookup at runtime.

use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let wasm_dir = workspace_root.join("target/wasm32-unknown-emscripten/release");
    let wasm_file = wasm_dir.join("luat_client.wasm");
    let emscripten_js = wasm_dir.join("luat-client.js");
    let client_js = Path::new(&manifest_dir).join("../luat-client/luat-client.js");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("wasm_assets.rs");

    if wasm_file.exists() && emscripten_js.exists() && client_js.exists() {
        // Generate code that embeds the WASM artifacts
        let code = format!(
            r##"
/// Returns the embedded WASM binary, if compiled with WASM support.
pub fn wasm_binary() -> Option<&'static [u8]> {{
    Some(include_bytes!({wasm_path:?}))
}}

/// Returns the embedded Emscripten JS factory, if compiled with WASM support.
pub fn emscripten_js() -> Option<&'static str> {{
    Some(include_str!({emscripten_path:?}))
}}

/// Returns the embedded luat-client.js runtime, if compiled with WASM support.
pub fn client_js() -> Option<&'static str> {{
    Some(include_str!({client_path:?}))
}}
"##,
            wasm_path = wasm_file.to_string_lossy(),
            emscripten_path = emscripten_js.to_string_lossy(),
            client_path = client_js.to_string_lossy(),
        );

        fs::write(&dest, code).unwrap();

        // Rerun if any artifact changes
        println!(
            "cargo:rerun-if-changed={}",
            wasm_file.to_string_lossy()
        );
        println!(
            "cargo:rerun-if-changed={}",
            emscripten_js.to_string_lossy()
        );
        println!(
            "cargo:rerun-if-changed={}",
            client_js.to_string_lossy()
        );
    } else {
        // Generate stubs that return None
        let code = r#"
/// Returns None — WASM artifacts were not available at compile time.
pub fn wasm_binary() -> Option<&'static [u8]> {
    None
}

/// Returns None — WASM artifacts were not available at compile time.
pub fn emscripten_js() -> Option<&'static str> {
    None
}

/// Returns None — WASM artifacts were not available at compile time.
pub fn client_js() -> Option<&'static str> {
    None
}
"#;
        fs::write(&dest, code).unwrap();
    }

    println!("cargo:rerun-if-changed=build.rs");
}
