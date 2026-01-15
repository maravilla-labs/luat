// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Type definitions for the KV store.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Result type for KV operations.
pub type KVResult<T> = Result<T, KVError>;

/// Error type for KV operations.
#[derive(Debug)]
pub enum KVError {
    /// Key not found (only used internally, get returns None instead).
    NotFound,
    /// Storage backend error.
    Storage(String),
    /// Serialization/deserialization error.
    Serialization(String),
    /// Invalid operation (e.g., invalid key format).
    InvalidOperation(String),
}

impl fmt::Display for KVError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KVError::NotFound => write!(f, "Key not found"),
            KVError::Storage(msg) => write!(f, "Storage error: {}", msg),
            KVError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            KVError::InvalidOperation(msg) => write!(f, "Invalid operation: {}", msg),
        }
    }
}

impl std::error::Error for KVError {}

impl From<serde_json::Error> for KVError {
    fn from(err: serde_json::Error) -> Self {
        KVError::Serialization(err.to_string())
    }
}

/// A stored KV entry with value and metadata.
#[derive(Debug, Clone)]
pub struct KVEntry {
    /// The stored value as raw bytes.
    pub value: Vec<u8>,
    /// Optional JSON metadata associated with the entry.
    pub metadata: Option<serde_json::Value>,
    /// Optional Unix timestamp when the entry expires.
    pub expiration: Option<u64>,
}

/// Options for the `put` operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PutOptions {
    /// Unix timestamp when the entry should expire.
    pub expiration: Option<u64>,
    /// Time-to-live in seconds from now.
    #[serde(rename = "expirationTtl")]
    pub expiration_ttl: Option<u64>,
    /// Arbitrary JSON metadata to store with the entry.
    pub metadata: Option<serde_json::Value>,
}

impl PutOptions {
    /// Calculate the actual expiration timestamp.
    ///
    /// If `expiration` is set, use it directly.
    /// If `expiration_ttl` is set, calculate from current time.
    /// If neither is set, return `None` (no expiration).
    pub fn calculate_expiration(&self) -> Option<u64> {
        if let Some(exp) = self.expiration {
            Some(exp)
        } else if let Some(ttl) = self.expiration_ttl {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(now + ttl)
        } else {
            None
        }
    }
}

/// Options for the `list` operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListOptions {
    /// Only return keys starting with this prefix.
    pub prefix: Option<String>,
    /// Maximum number of keys to return.
    pub limit: Option<usize>,
    /// Cursor for pagination (opaque string from previous list result).
    pub cursor: Option<String>,
}

/// Result of a `list` operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult {
    /// The keys matching the query.
    pub keys: Vec<ListKey>,
    /// Whether all matching keys have been returned.
    pub list_complete: bool,
    /// Cursor for fetching the next page (if `list_complete` is false).
    pub cursor: Option<String>,
}

/// Information about a key in a list result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKey {
    /// The key name.
    pub name: String,
    /// Optional expiration timestamp.
    pub expiration: Option<u64>,
    /// Optional metadata associated with the key.
    pub metadata: Option<serde_json::Value>,
}
