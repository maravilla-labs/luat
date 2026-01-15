// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! SQLite-backed KV store implementation.

use luat::kv::{KVEntry, KVError, KVResult, KVStore, ListKey, ListOptions, ListResult, PutOptions};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Row data returned from KV queries: (value, metadata, expiration).
type KVRowData = (Vec<u8>, Option<String>, Option<u64>);

/// Maximum number of items that can be returned in a single list query.
/// This prevents memory exhaustion from unbounded queries.
const MAX_LIST_LIMIT: usize = 10000;

/// SQLite-backed KV store.
///
/// Each namespace shares the same SQLite database but uses a namespace
/// column to separate data.
pub struct SqliteKVStore {
    conn: Mutex<Connection>,
    namespace: String,
}

impl SqliteKVStore {
    /// Creates a new SQLite-backed KV store.
    ///
    /// The database file is stored at `data_dir/kv.db`.
    pub fn new(data_dir: &Path, namespace: &str) -> KVResult<Self> {
        let db_path = data_dir.join("kv.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| KVError::Storage(format!("Failed to open database: {}", e)))?;

        // Create table if it doesn't exist
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS kv (
                namespace TEXT NOT NULL,
                key TEXT NOT NULL,
                value BLOB NOT NULL,
                metadata TEXT,
                expiration INTEGER,
                PRIMARY KEY (namespace, key)
            )
            "#,
            [],
        )
        .map_err(|e| KVError::Storage(format!("Failed to create table: {}", e)))?;

        // Create index for prefix queries
        conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_kv_namespace_key ON kv (namespace, key)
            "#,
            [],
        )
        .map_err(|e| KVError::Storage(format!("Failed to create index: {}", e)))?;

        Ok(Self {
            conn: Mutex::new(conn),
            namespace: namespace.to_string(),
        })
    }

    /// Get current Unix timestamp.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Check if an expiration timestamp is expired.
    fn is_expired(expiration: Option<u64>) -> bool {
        if let Some(exp) = expiration {
            Self::now() >= exp
        } else {
            false
        }
    }
}

impl KVStore for SqliteKVStore {
    fn get(&self, key: &str) -> KVResult<Option<Vec<u8>>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let result: Result<(Vec<u8>, Option<u64>), rusqlite::Error> = conn.query_row(
            "SELECT value, expiration FROM kv WHERE namespace = ?1 AND key = ?2",
            params![&self.namespace, key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((value, expiration)) => {
                if Self::is_expired(expiration) {
                    // Entry is expired, delete it and return None
                    let _ = conn.execute(
                        "DELETE FROM kv WHERE namespace = ?1 AND key = ?2",
                        params![&self.namespace, key],
                    );
                    Ok(None)
                } else {
                    Ok(Some(value))
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KVError::Storage(e.to_string())),
        }
    }

    fn get_with_metadata(&self, key: &str) -> KVResult<Option<KVEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let result: Result<KVRowData, rusqlite::Error> = conn.query_row(
                "SELECT value, metadata, expiration FROM kv WHERE namespace = ?1 AND key = ?2",
                params![&self.namespace, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            );

        match result {
            Ok((value, metadata_str, expiration)) => {
                if Self::is_expired(expiration) {
                    // Entry is expired, delete it and return None
                    let _ = conn.execute(
                        "DELETE FROM kv WHERE namespace = ?1 AND key = ?2",
                        params![&self.namespace, key],
                    );
                    Ok(None)
                } else {
                    let metadata = metadata_str
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| KVError::Serialization(e.to_string()))?;

                    Ok(Some(KVEntry {
                        value,
                        metadata,
                        expiration,
                    }))
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KVError::Storage(e.to_string())),
        }
    }

    fn put(&self, key: &str, value: &[u8], options: PutOptions) -> KVResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let expiration = options.calculate_expiration();
        let metadata_str = options
            .metadata
            .map(|m| serde_json::to_string(&m))
            .transpose()
            .map_err(|e| KVError::Serialization(e.to_string()))?;

        conn.execute(
            r#"
            INSERT OR REPLACE INTO kv (namespace, key, value, metadata, expiration)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![&self.namespace, key, value, metadata_str, expiration],
        )
        .map_err(|e| KVError::Storage(e.to_string()))?;

        Ok(())
    }

    fn delete(&self, key: &str) -> KVResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        conn.execute(
            "DELETE FROM kv WHERE namespace = ?1 AND key = ?2",
            params![&self.namespace, key],
        )
        .map_err(|e| KVError::Storage(e.to_string()))?;

        Ok(())
    }

    fn list(&self, options: ListOptions) -> KVResult<ListResult> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let now = Self::now();
        let limit = std::cmp::min(options.limit.unwrap_or(1000), MAX_LIST_LIMIT);

        // Build query based on options
        let mut sql = String::from(
            "SELECT key, metadata, expiration FROM kv WHERE namespace = ?1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(self.namespace.clone())];

        // Add prefix filter if specified
        if let Some(ref prefix) = options.prefix {
            sql.push_str(" AND key LIKE ?2");
            params_vec.push(Box::new(format!("{}%", prefix)));
        }

        // Add cursor filter if specified (pagination)
        if let Some(ref cursor) = options.cursor {
            let param_num = params_vec.len() + 1;
            sql.push_str(&format!(" AND key > ?{}", param_num));
            params_vec.push(Box::new(cursor.clone()));
        }

        // Order and limit
        sql.push_str(" ORDER BY key");
        sql.push_str(&format!(" LIMIT {}", limit + 1)); // Fetch one extra to check if more

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let key: String = row.get(0)?;
                let metadata_str: Option<String> = row.get(1)?;
                let expiration: Option<u64> = row.get(2)?;
                Ok((key, metadata_str, expiration))
            })
            .map_err(|e| KVError::Storage(e.to_string()))?;

        let mut keys = Vec::new();
        for row in rows {
            let (key, metadata_str, expiration) =
                row.map_err(|e| KVError::Storage(e.to_string()))?;

            // Skip expired entries
            if let Some(exp) = expiration {
                if now >= exp {
                    continue;
                }
            }

            // Check if we've reached the limit
            if keys.len() >= limit {
                // There are more keys
                let cursor = keys.last().map(|k: &ListKey| k.name.clone());
                return Ok(ListResult {
                    keys,
                    list_complete: false,
                    cursor,
                });
            }

            let metadata = metadata_str
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|e| KVError::Serialization(e.to_string()))?;

            keys.push(ListKey {
                name: key,
                expiration,
                metadata,
            });
        }

        Ok(ListResult {
            keys,
            list_complete: true,
            cursor: None,
        })
    }
}

// SQLite connections are not Send by default, but our Mutex wrapper makes it safe
unsafe impl Send for SqliteKVStore {}
unsafe impl Sync for SqliteKVStore {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (TempDir, SqliteKVStore) {
        let temp_dir = TempDir::new().unwrap();
        let store = SqliteKVStore::new(temp_dir.path(), "test").unwrap();
        (temp_dir, store)
    }

    #[test]
    fn test_basic_operations() {
        let (_temp_dir, store) = create_test_store();

        // Put and get
        store.put("key1", b"value1", PutOptions::default()).unwrap();
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
        let (_temp_dir, store) = create_test_store();

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
        let (_temp_dir, store) = create_test_store();

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
        let (_temp_dir, store) = create_test_store();

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
        let (_temp_dir, store) = create_test_store();

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
