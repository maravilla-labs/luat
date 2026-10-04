// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Embedded WASM client assets for hybrid rendering mode.
//!
//! These are conditionally compiled in by `build.rs` when the WASM client
//! artifacts exist at build time. When installed via `cargo install` after
//! building with `make wasm-client-release`, the CLI binary is self-contained.
//!
//! At dev server startup, embedded assets are written to `static/_luat/`
//! so they are served by the existing static file server and benefit from
//! the file watcher hot reload.

include!(concat!(env!("OUT_DIR"), "/wasm_assets.rs"));
