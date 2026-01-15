// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Key-Value store extension for Luat.
//!
//! This module provides a platform-agnostic KV store with a familiar, industry-standard API.
//! It's designed to work only in server-side contexts (`+server.lua`, `+page.server.lua`).
//!
//! # API
//!
//! ```lua
//! -- Get a namespace
//! local kv = KV.namespace("my-namespace")
//!
//! -- Read
//! local value = kv:get("key")                    -- text by default
//! local json = kv:get("key", "json")             -- parse as JSON
//! local data, meta = kv:getWithMetadata("key")
//!
//! -- Write
//! kv:put("key", "value")
//! kv:put("key", "value", {
//!     expiration = 1735689600,      -- Unix timestamp
//!     expirationTtl = 3600,         -- Seconds from now
//!     metadata = { author = "me" }
//! })
//!
//! -- Delete
//! kv:delete("key")
//!
//! -- List
//! local result = kv:list({ prefix = "blog:", limit = 100 })
//! ```
//!
//! # Implementations
//!
//! - **CLI**: SQLite-backed persistent storage
//! - **WASM**: IndexedDB-backed browser storage

mod memory;
mod register;
mod types;

pub use memory::MemoryKVStore;
pub use register::register_kv_module;
pub use types::{KVEntry, KVError, KVResult, ListKey, ListOptions, ListResult, PutOptions};

use std::sync::Arc;

/// Platform-agnostic KV store trait.
///
/// Implementors provide the actual storage mechanism (SQLite, IndexedDB, etc.).
pub trait KVStore: Send + Sync {
    /// Get a value by key.
    ///
    /// Returns `None` if the key doesn't exist or is expired.
    fn get(&self, key: &str) -> KVResult<Option<Vec<u8>>>;

    /// Get a value with its metadata.
    ///
    /// Returns `None` if the key doesn't exist or is expired.
    fn get_with_metadata(&self, key: &str) -> KVResult<Option<KVEntry>>;

    /// Store a value with optional expiration and metadata.
    fn put(&self, key: &str, value: &[u8], options: PutOptions) -> KVResult<()>;

    /// Delete a key.
    ///
    /// No error is returned if the key doesn't exist.
    fn delete(&self, key: &str) -> KVResult<()>;

    /// List keys with optional prefix filtering and pagination.
    fn list(&self, options: ListOptions) -> KVResult<ListResult>;
}

/// Factory function type for creating namespaced KV stores.
pub type KVStoreFactory = Arc<dyn Fn(&str) -> Arc<dyn KVStore> + Send + Sync>;
