// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Form actions system for Luat.
//!
//! This module provides a platform-agnostic actions system inspired by SvelteKit's
//! form actions. It supports:
//!
//! - Default actions (POST/PUT/DELETE without `?/actionName`)
//! - Named actions (e.g., `?/login`, `?/publish`)
//! - Both function and method-table action definitions
//! - HTMX-compatible responses with custom headers
//!
//! # Example
//!
//! ```lua
//! -- +page.server.lua
//! actions = {
//!     default = function(ctx)
//!         -- Handle default POST
//!         return { success = true }
//!     end,
//!
//!     login = function(ctx)
//!         -- Handle POST ?/login
//!         local email = ctx.form.email
//!         if not email then
//!             return fail(400, { error = "Email required" })
//!         end
//!         return { success = true }
//!     end,
//!
//!     -- Method-specific handlers
//!     update = {
//!         post = function(ctx) ... end,
//!         put = function(ctx) ... end,
//!     }
//! }
//! ```

mod context;
mod executor;
mod response;

pub use context::ActionContext;
pub use executor::ActionExecutor;
pub use response::ActionResponse;
