// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! CLI-specific Lua extensions that require async/network capabilities.

pub mod http;

pub use http::register_http_module;
