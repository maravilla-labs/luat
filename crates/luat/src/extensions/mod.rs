// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

/// JSON module for Lua.
pub mod json;
/// Lua extensions.
pub mod lua;

pub use json::register_json_module;