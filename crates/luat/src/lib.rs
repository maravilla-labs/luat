// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// Warn on missing documentation for public items
#![warn(missing_docs)]

// Allow large error types - the LuatError enum contains rich context for debugging
// (source snippets, tracebacks). This is an intentional design choice for better DX.
#![allow(clippy::result_large_err)]

//! # LUAT
//!
//! Svelte-inspired server-side Lua templating engine for Rust.
//!
//! LUAT provides a component-based templating system with Svelte-like syntax,
//! compiled to Lua for high-performance server-side rendering.
//!
//! ## Features
//!
//! - Svelte-like template syntax (`{#if}`, `{#each}`, components)
//! - Server-side rendering only (no client hydration)
//! - Component system with props and children
//! - Template bundling for production
//! - Built-in caching (memory or filesystem)
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use luat::{Engine, FileSystemResolver};
//!
//! let resolver = FileSystemResolver::new("./templates");
//! let engine = Engine::with_memory_cache(resolver, 100)?;
//!
//! let module = engine.compile_entry("hello.luat")?;
//! let context = engine.to_value(serde_json::json!({ "name": "World" }))?;
//! let html = engine.render(&module, &context)?;
//! ```

/// Abstract Syntax Tree types for templates.
pub mod ast;
/// Template parser.
pub mod parser;
/// AST to IR transformation.
pub mod transform;
/// Lua code generation.
pub mod codegen;
/// Dependency graph analysis.
pub mod dependencies;
/// Main template engine.
pub mod engine;
/// Resource resolution (filesystem, memory).
pub mod resolver;
/// Error types and reporting.
pub mod error;
/// Compiled module caching.
pub mod cache;
/// Lua runtime extensions.
pub mod extensions;
/// Script block processing.
pub mod script_processor;
/// In-memory resource resolver for testing/WASM.
pub mod memory_resolver;
/// Source map generation for debugging.
pub mod sourcemap;
/// Enhanced parser with better error recovery.
pub mod enhanced_parser;
/// Engine extension utilities.
pub mod engine_ext;
/// Form actions system for handling form submissions.
pub mod actions;
/// Key-Value store extension with a familiar, industry-standard API.
pub mod kv;
/// HTTP request abstraction for the engine.
pub mod request;
/// HTTP response abstraction for the engine.
pub mod response;
/// Shared request body parsing helpers.
mod body;
/// File-based routing for the engine.
pub mod router;
/// Runtime execution for server-side Lua code.
pub mod runtime;

/// WASM bindings for browser usage.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(test)]
mod test_parsing;

#[cfg(test)]
mod script_string_test;

pub use ast::*;
pub use parser::*;
pub use parser::Rule; // Explicitly export Rule enum
pub use parser::LuatParser; // Explicitly export LuatParser
pub use transform::*;
pub use codegen::*;
pub use dependencies::*;
pub use engine::*;
pub use resolver::*;
pub use error::*;
pub use cache::*;
pub use request::LuatRequest;
pub use response::LuatResponse;
pub use router::{Route, Router};
pub use runtime::{ApiResult, LoadResult, Runtime};
pub use extensions::register_json_module;

// Re-export mlua value
pub use mlua::Value;

// Re-export WASM bindings when targeting WebAssembly
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod simple_test;

#[cfg(test)]
mod debug_test;

#[cfg(test)]
mod wasm_tests;
