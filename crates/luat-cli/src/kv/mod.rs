// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! CLI KV store implementation using SQLite.

mod sqlite;

pub use sqlite::SqliteKVStore;

use luat::kv::{KVStore, KVStoreFactory};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Manager for creating and caching KV store instances.
///
/// Each namespace gets its own KV store, and stores are cached
/// across requests for the lifetime of the server.
pub struct KVManager {
    data_dir: PathBuf,
    stores: RwLock<HashMap<String, Arc<SqliteKVStore>>>,
}

impl KVManager {
    /// Creates a new KV manager with the given data directory.
    ///
    /// The data directory will be created if it doesn't exist.
    pub fn new(data_dir: impl AsRef<Path>) -> std::io::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        // Create data directory if it doesn't exist
        std::fs::create_dir_all(&data_dir)?;

        Ok(Self {
            data_dir,
            stores: RwLock::new(HashMap::new()),
        })
    }

    /// Gets or creates a KV store for the given namespace.
    pub fn get_store(&self, namespace: &str) -> Arc<SqliteKVStore> {
        // Check if we already have a store for this namespace
        {
            let stores = self.stores.read().unwrap();
            if let Some(store) = stores.get(namespace) {
                return store.clone();
            }
        }

        // Create a new store
        let store = Arc::new(
            SqliteKVStore::new(&self.data_dir, namespace)
                .expect("Failed to create KV store"),
        );

        // Cache it
        {
            let mut stores = self.stores.write().unwrap();
            stores.insert(namespace.to_string(), store.clone());
        }

        store
    }

    /// Creates a factory function for use with `register_kv_module`.
    pub fn factory(self: Arc<Self>) -> KVStoreFactory {
        Arc::new(move |namespace: &str| -> Arc<dyn KVStore> {
            self.get_store(namespace)
        })
    }
}
