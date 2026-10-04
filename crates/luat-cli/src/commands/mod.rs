// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! CLI command implementations.
//!
//! This module contains the implementations for all LUAT CLI commands:
//!
//! - `build`: Compile templates for production
//! - `dev`: Start development server with hot reload
//! - `init`: Initialize a new LUAT project
//! - `serve`: Serve a production build
//! - `watch`: Watch files and rebuild on changes
//! - `add`, `remove`, `install`, `update`, `search`, `pack`, `publish`,
//!   `login`, `yank`: Luat packages (see `docs/packages.md`)

/// Production build command.
pub mod build;
/// Development server command.
pub mod dev;
/// Project initialization command.
pub mod init;
/// Package commands (add, remove, install, update, search, pack, publish,
/// login, yank).
pub mod packages;
/// Production server command.
pub mod serve;
/// File watch command.
pub mod watch;
