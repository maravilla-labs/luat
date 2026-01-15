// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! In-memory KV store implementation for testing.

use super::{KVEntry, KVError, KVResult, KVStore, ListKey, ListOptions, ListResult, PutOptions};
use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// In-memory KV store entry.
#[derive(Clone)]
struct MemoryEntry {
    value: Vec<u8>,
    metadata: Option<serde_json::Value>,
    expiration: Option<u64>,
}

/// In-memory KV store implementation.
///
/// Useful for testing and development. Data is lost when the process exits.
pub struct MemoryKVStore {
    data: RwLock<BTreeMap<String, MemoryEntry>>,
}

impl MemoryKVStore {
    /// Create a new in-memory KV store.
    pub fn new() -> Self {
        Self {
            data: RwLock::new(BTreeMap::new()),
        }
    }

    /// Get the current Unix timestamp.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Check if an entry is expired.
    fn is_expired(entry: &MemoryEntry) -> bool {
        if let Some(exp) = entry.expiration {
            Self::now() >= exp
        } else {
            false
        }
    }
}

impl Default for MemoryKVStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KVStore for MemoryKVStore {
    fn get(&self, key: &str) -> KVResult<Option<Vec<u8>>> {
        let data = self.data.read().map_err(|e| KVError::Storage(e.to_string()))?;

        match data.get(key) {
            Some(entry) if !Self::is_expired(entry) => Ok(Some(entry.value.clone())),
            _ => Ok(None),
        }
    }

    fn get_with_metadata(&self, key: &str) -> KVResult<Option<KVEntry>> {
        let data = self.data.read().map_err(|e| KVError::Storage(e.to_string()))?;

        match data.get(key) {
            Some(entry) if !Self::is_expired(entry) => Ok(Some(KVEntry {
                value: entry.value.clone(),
                metadata: entry.metadata.clone(),
                expiration: entry.expiration,
            })),
            _ => Ok(None),
        }
    }

    fn put(&self, key: &str, value: &[u8], options: PutOptions) -> KVResult<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let entry = MemoryEntry {
            value: value.to_vec(),
            metadata: options.metadata.clone(),
            expiration: options.calculate_expiration(),
        };

        data.insert(key.to_string(), entry);
        Ok(())
    }

    fn delete(&self, key: &str) -> KVResult<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        data.remove(key);
        Ok(())
    }

    fn list(&self, options: ListOptions) -> KVResult<ListResult> {
        let data = self.data.read().map_err(|e| KVError::Storage(e.to_string()))?;

        let now = Self::now();
        let limit = options.limit.unwrap_or(1000);

        // Parse cursor as the last key seen (simple pagination)
        let start_after = options.cursor.as_deref();

        let mut keys = Vec::new();
        let mut seen_start = start_after.is_none();

        for (key, entry) in data.iter() {
            // Skip until we pass the cursor
            if !seen_start {
                if Some(key.as_str()) == start_after {
                    seen_start = true;
                }
                continue;
            }

            // Skip expired entries
            if let Some(exp) = entry.expiration {
                if now >= exp {
                    continue;
                }
            }

            // Apply prefix filter
            if let Some(ref prefix) = options.prefix {
                if !key.starts_with(prefix) {
                    continue;
                }
            }

            // Check limit (fetch one extra to determine if list is complete)
            if keys.len() >= limit {
                // There are more keys
                let cursor = keys.last().map(|k: &ListKey| k.name.clone());
                return Ok(ListResult {
                    keys,
                    list_complete: false,
                    cursor,
                });
            }

            keys.push(ListKey {
                name: key.clone(),
                expiration: entry.expiration,
                metadata: entry.metadata.clone(),
            });
        }

        Ok(ListResult {
            keys,
            list_complete: true,
            cursor: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let store = MemoryKVStore::new();

        // Put and get
        store
            .put("key1", b"value1", PutOptions::default())
            .unwrap();
        let value = store.get("key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Get non-existent key
        let value = store.get("nonexistent").unwrap();
        assert_eq!(value, None);

        // Delete
        store.delete("key1").unwrap();
        let value = store.get("key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_metadata() {
        let store = MemoryKVStore::new();

        let options = PutOptions {
            metadata: Some(serde_json::json!({ "author": "test" })),
            ..Default::default()
        };
        store.put("key1", b"value1", options).unwrap();

        let entry = store.get_with_metadata("key1").unwrap().unwrap();
        assert_eq!(entry.value, b"value1".to_vec());
        assert_eq!(
            entry.metadata,
            Some(serde_json::json!({ "author": "test" }))
        );
    }

    #[test]
    fn test_expiration_ttl() {
        let store = MemoryKVStore::new();

        // Set with 0 TTL (expired immediately)
        let options = PutOptions {
            expiration_ttl: Some(0),
            ..Default::default()
        };
        store.put("key1", b"value1", options).unwrap();

        // Should be expired
        let value = store.get("key1").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_list_with_prefix() {
        let store = MemoryKVStore::new();

        store
            .put("blog:post1", b"content1", PutOptions::default())
            .unwrap();
        store
            .put("blog:post2", b"content2", PutOptions::default())
            .unwrap();
        store
            .put("user:alice", b"data", PutOptions::default())
            .unwrap();

        let result = store
            .list(ListOptions {
                prefix: Some("blog:".to_string()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(result.keys.len(), 2);
        assert!(result.list_complete);
        assert!(result.keys.iter().all(|k| k.name.starts_with("blog:")));
    }

    #[test]
    fn test_list_pagination() {
        let store = MemoryKVStore::new();

        for i in 0..5 {
            store
                .put(&format!("key{}", i), b"value", PutOptions::default())
                .unwrap();
        }

        // First page
        let result = store
            .list(ListOptions {
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(result.keys.len(), 2);
        assert!(!result.list_complete);
        assert!(result.cursor.is_some());

        // Second page
        let result = store
            .list(ListOptions {
                limit: Some(2),
                cursor: result.cursor,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(result.keys.len(), 2);
        assert!(!result.list_complete);
    }
}
