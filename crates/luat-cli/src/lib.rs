// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// Warn on missing documentation for public items
#![warn(missing_docs)]

//! LUAT CLI library.
//!
//! This crate provides the command-line interface for the LUAT templating engine.
//! It includes commands for development, building, and serving LUAT applications.
//!
//! # Features
//!
//! - **Development server** with hot reload
//! - **SvelteKit-style routing** with file-based routes
//! - **Frontend toolchain** integration (Vite, Bun, npm)
//! - **File watching** for automatic rebuilds
//!
//! # Usage
//!
//! This crate is primarily used through the `luat` binary:
//!
//! ```bash
//! luat dev      # Start development server
//! luat build    # Build for production
//! luat serve    # Serve production build
//! luat init     # Initialize new project
//! ```
//!
//! # Configuration
//!
//! Projects are configured via `luat.toml` at the project root.

/// CLI commands (dev, build, serve, init, watch).
pub mod commands;
/// Project configuration from `luat.toml`.
pub mod config;
/// CLI-specific Lua extensions (http client).
pub mod extensions;
/// Key-Value store with SQLite backend.
pub mod kv;
/// SvelteKit-style file-based routing.
pub mod router;
/// Development server with hot reload.
pub mod server;
/// Frontend toolchain management (Vite, Bun, npm).
pub mod toolchain;
/// File system watching for hot reload.
pub mod watcher;
