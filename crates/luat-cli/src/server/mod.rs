// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Development server components.
//!
//! This module provides the HTTP server and live reload functionality
//! for the LUAT development experience.
//!
//! # Components
//!
//! - `http`: HTTP server using Axum
//! - `livereload`: WebSocket-based hot reload
//! - `loader`: Template loading and caching

/// Request body parsing for form data and JSON.
pub mod body_parser;
/// HTTP server implementation using Axum.
pub mod http;
/// Live reload WebSocket server.
pub mod livereload;
/// Template loading and resolution.
pub mod loader;
