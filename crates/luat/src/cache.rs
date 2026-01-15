// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Compiled module caching for LUAT templates.
//!
//! This module provides caching infrastructure for compiled templates,
//! avoiding repeated parsing and compilation of unchanged templates.
//!
//! # Cache Implementations
//!
//! - [`MemoryCache`]: In-memory LRU cache (recommended for most uses)
//! - [`FileSystemCache`]: Persistent disk cache (production deployments)
//!
//! # Platform Support
//!
//! The cache implementations are platform-aware:
//! - **Native builds**: Use `Arc<Mutex<...>>` for thread safety
//! - **WASM builds**: Use `Rc<RefCell<...>>` (single-threaded)
//!
//! # Custom Caches
//!
//! Implement the [`Cache`] trait to create custom caching strategies
//! (e.g., Redis-backed, distributed, etc.).

use crate::error::Result;
#[cfg(not(target_arch = "wasm32"))]
use crate::error::LuatError;
use lru::LruCache;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// Conditional imports for thread primitives
// Native builds use Arc/Mutex for thread safety
// WASM builds use Rc/RefCell (single-threaded)
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Mutex};

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

/// Platform-agnostic shared pointer type.
///
/// - Native: `Arc<T>` for thread-safe reference counting
/// - WASM: `Rc<T>` for single-threaded reference counting
#[cfg(not(target_arch = "wasm32"))]
pub type SharedPtr<T> = Arc<T>;
/// Platform-agnostic shared pointer type.
#[cfg(target_arch = "wasm32")]
pub type SharedPtr<T> = Rc<T>;

#[cfg(not(target_arch = "wasm32"))]
type SharedMut<T> = Arc<Mutex<T>>;
#[cfg(target_arch = "wasm32")]
type SharedMut<T> = Rc<RefCell<T>>;

/// A compiled LUAT template module.
///
/// Contains the generated Lua code and metadata about the template,
/// including its dependencies and a content hash for cache invalidation.
#[derive(Debug, Clone)]
pub struct Module {
    /// The module name (typically the template filename).
    pub name: String,
    /// The generated Lua source code.
    pub lua_code: String,
    /// Paths to component dependencies.
    pub dependencies: Vec<String>,
    /// Hash of the Lua code for cache invalidation.
    pub hash: u64,
    /// The original file path (for debug.getinfo support in relative imports).
    pub path: Option<String>,
    /// Source map for mapping Lua line numbers to .luat source lines.
    pub source_map: Option<crate::codegen::LuaSourceMap>,
}

impl Module {
    /// Creates a new compiled module.
    ///
    /// Automatically computes a hash of the Lua code for cache invalidation.
    pub fn new(name: String, lua_code: String, dependencies: Vec<String>) -> Self {
        let mut hasher = DefaultHasher::new();
        lua_code.hash(&mut hasher);
        let hash = hasher.finish();

        Self {
            name,
            lua_code,
            dependencies,
            hash,
            path: None,
            source_map: None,
        }
    }

    /// Creates a new compiled module with a path.
    pub fn with_path(name: String, lua_code: String, dependencies: Vec<String>, path: String) -> Self {
        let mut module = Self::new(name, lua_code, dependencies);
        module.path = Some(path);
        module
    }

    /// Creates a new compiled module with a source map.
    pub fn with_source_map(
        name: String,
        lua_code: String,
        dependencies: Vec<String>,
        path: Option<String>,
        source_map: crate::codegen::LuaSourceMap,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        lua_code.hash(&mut hasher);
        let hash = hasher.finish();

        Self {
            name,
            lua_code,
            dependencies,
            hash,
            path,
            source_map: Some(source_map),
        }
    }
}

/// Trait for compiled module caches.
///
/// Implement this trait to create custom caching strategies.
/// On native builds, implementations must be thread-safe (`Send + Sync`).
#[cfg(not(target_arch = "wasm32"))]
pub trait Cache: Send + Sync + std::fmt::Debug {
    /// Retrieves a module from the cache.
    fn get(&self, key: &str) -> Result<Option<SharedPtr<Module>>>;
    /// Stores a module in the cache.
    fn set(&self, key: &str, module: SharedPtr<Module>) -> Result<()>;
    /// Removes a module from the cache.
    fn remove(&self, key: &str) -> Result<()>;
    /// Clears all cached modules.
    fn clear(&self) -> Result<()>;
    /// Checks if a key exists in the cache.
    fn contains_key(&self, key: &str) -> bool;
    /// Creates a boxed clone (for use in closures).
    fn clone_box(&self) -> Box<dyn Cache>;
}

/// Trait for compiled module caches (WASM variant).
#[cfg(target_arch = "wasm32")]
pub trait Cache: std::fmt::Debug {
    /// Retrieves a module from the cache.
    fn get(&self, key: &str) -> Result<Option<SharedPtr<Module>>>;
    /// Stores a module in the cache.
    fn set(&self, key: &str, module: SharedPtr<Module>) -> Result<()>;
    /// Removes a module from the cache.
    fn remove(&self, key: &str) -> Result<()>;
    /// Clears all cached modules.
    fn clear(&self) -> Result<()>;
    /// Checks if a key exists in the cache.
    fn contains_key(&self, key: &str) -> bool;
    /// Creates a boxed clone (for use in closures).
    fn clone_box(&self) -> Box<dyn Cache>;
}

impl Clone for Box<dyn Cache> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// In-memory LRU (Least Recently Used) cache.
///
/// Stores compiled modules in memory with automatic eviction of
/// least recently used entries when the capacity is reached.
///
/// # Examples
///
/// ```rust,ignore
/// use luat::MemoryCache;
///
/// // Create a cache with capacity for 100 modules
/// let cache = MemoryCache::new(100);
/// ```
#[derive(Debug, Clone)]
pub struct MemoryCache {
    cache: SharedMut<LruCache<String, SharedPtr<Module>>>,
}

impl MemoryCache {
    /// Creates a new memory cache with the given capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of modules to cache
    pub fn new(capacity: usize) -> Self {
        let lru_cache = LruCache::new(std::num::NonZeroUsize::new(capacity).unwrap());

        #[cfg(not(target_arch = "wasm32"))]
        let cache = Arc::new(Mutex::new(lru_cache));

        #[cfg(target_arch = "wasm32")]
        let cache = Rc::new(RefCell::new(lru_cache));

        Self { cache }
    }
}

impl Cache for MemoryCache {
    fn get(&self, key: &str) -> Result<Option<SharedPtr<Module>>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut cache = self.cache.lock().map_err(|_| {
                LuatError::CacheError("Failed to acquire cache lock".to_string())
            })?;
            Ok(cache.get(key).cloned())
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut cache = self.cache.borrow_mut();
            Ok(cache.get(key).cloned())
        }
    }

    fn set(&self, key: &str, module: SharedPtr<Module>) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut cache = self.cache.lock().map_err(|_| {
                LuatError::CacheError("Failed to acquire cache lock".to_string())
            })?;
            cache.put(key.to_string(), module);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut cache = self.cache.borrow_mut();
            cache.put(key.to_string(), module);
        }

        Ok(())
    }

    fn remove(&self, key: &str) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut cache = self.cache.lock().map_err(|_| {
                LuatError::CacheError("Failed to acquire cache lock".to_string())
            })?;
            cache.pop(key);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut cache = self.cache.borrow_mut();
            cache.pop(key);
        }

        Ok(())
    }

    fn clear(&self) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut cache = self.cache.lock().map_err(|_| {
                LuatError::CacheError("Failed to acquire cache lock".to_string())
            })?;
            cache.clear();
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut cache = self.cache.borrow_mut();
            cache.clear();
        }

        Ok(())
    }

    fn contains_key(&self, key: &str) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Ok(cache) = self.cache.lock() {
                cache.contains(key)
            } else {
                false
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.cache.borrow().contains(key)
        }
    }

    fn clone_box(&self) -> Box<dyn Cache> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Box::new(Self {
                cache: Arc::clone(&self.cache),
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            Box::new(Self {
                cache: Rc::clone(&self.cache),
            })
        }
    }
}

/// No-op cache that never stores or retrieves anything.
///
/// Useful for development mode where we want to always compile fresh.
#[derive(Debug, Clone, Default)]
pub struct NoOpCache;

impl NoOpCache {
    /// Creates a new no-op cache.
    pub fn new() -> Self {
        Self
    }
}

impl Cache for NoOpCache {
    fn get(&self, _key: &str) -> Result<Option<SharedPtr<Module>>> {
        Ok(None)
    }

    fn set(&self, _key: &str, _module: SharedPtr<Module>) -> Result<()> {
        Ok(())
    }

    fn remove(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        Ok(())
    }

    fn contains_key(&self, _key: &str) -> bool {
        false
    }

    fn clone_box(&self) -> Box<dyn Cache> {
        Box::new(NoOpCache)
    }
}

/// Persistent filesystem-backed cache with memory layer.
///
/// Stores compiled modules on disk for persistence across restarts,
/// with an in-memory LRU layer for fast access.
///
/// Only available on native builds with the `filesystem` feature.
#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
#[derive(Debug)]
pub struct FileSystemCache {
    cache_dir: std::path::PathBuf,
    memory_cache: MemoryCache,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
impl FileSystemCache {
    /// Creates a new filesystem cache.
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Directory for storing cache files
    /// * `memory_capacity` - Size of in-memory LRU layer
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created.
    pub fn new<P: AsRef<std::path::Path>>(cache_dir: P, memory_capacity: usize) -> Result<Self> {
        let cache_dir = cache_dir.as_ref().to_path_buf();

        // Create cache directory if it doesn't exist
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            LuatError::CacheError(format!("Failed to create cache directory: {}", e))
        })?;

        Ok(Self {
            cache_dir,
            memory_cache: MemoryCache::new(memory_capacity),
        })
    }

    fn cache_file_path(&self, key: &str) -> std::path::PathBuf {
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.cache_dir.join(format!("{}.lua", safe_key))
    }

    fn metadata_file_path(&self, key: &str) -> std::path::PathBuf {
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.cache_dir.join(format!("{}.meta.json", safe_key))
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
impl Cache for FileSystemCache {
    fn get(&self, key: &str) -> Result<Option<SharedPtr<Module>>> {
        // Try memory cache first
        if let Some(module) = self.memory_cache.get(key)? {
            return Ok(Some(module));
        }

        // Try file system cache
        let cache_file = self.cache_file_path(key);
        let metadata_file = self.metadata_file_path(key);

        if !cache_file.exists() || !metadata_file.exists() {
            return Ok(None);
        }

        let lua_code = std::fs::read_to_string(&cache_file).map_err(|e| {
            LuatError::CacheError(format!("Failed to read cache file: {}", e))
        })?;

        let metadata_str = std::fs::read_to_string(&metadata_file).map_err(|e| {
            LuatError::CacheError(format!("Failed to read metadata file: {}", e))
        })?;

        let metadata: serde_json::Value = serde_json::from_str(&metadata_str).map_err(|e| {
            LuatError::CacheError(format!("Failed to parse metadata: {}", e))
        })?;

        let name = metadata["name"].as_str().unwrap_or(key).to_string();
        let dependencies: Vec<String> = metadata["dependencies"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let module = Arc::new(Module::new(name, lua_code, dependencies));

        // Store in memory cache for faster access
        self.memory_cache.set(key, module.clone())?;

        Ok(Some(module))
    }

    fn set(&self, key: &str, module: SharedPtr<Module>) -> Result<()> {
        // Store in memory cache
        self.memory_cache.set(key, module.clone())?;

        // Store in file system cache
        let cache_file = self.cache_file_path(key);
        let metadata_file = self.metadata_file_path(key);

        std::fs::write(&cache_file, &module.lua_code).map_err(|e| {
            LuatError::CacheError(format!("Failed to write cache file: {}", e))
        })?;

        let metadata = serde_json::json!({
            "name": module.name,
            "dependencies": module.dependencies,
            "hash": module.hash,
            "created_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        std::fs::write(&metadata_file, metadata.to_string()).map_err(|e| {
            LuatError::CacheError(format!("Failed to write metadata file: {}", e))
        })?;

        Ok(())
    }

    fn remove(&self, key: &str) -> Result<()> {
        // Remove from memory cache
        self.memory_cache.remove(key)?;

        // Remove from file system cache
        let cache_file = self.cache_file_path(key);
        let metadata_file = self.metadata_file_path(key);

        if cache_file.exists() {
            std::fs::remove_file(&cache_file).map_err(|e| {
                LuatError::CacheError(format!("Failed to remove cache file: {}", e))
            })?;
        }

        if metadata_file.exists() {
            std::fs::remove_file(&metadata_file).map_err(|e| {
                LuatError::CacheError(format!("Failed to remove metadata file: {}", e))
            })?;
        }

        Ok(())
    }

    fn clear(&self) -> Result<()> {
        // Clear memory cache
        self.memory_cache.clear()?;

        // Clear file system cache
        for entry in std::fs::read_dir(&self.cache_dir).map_err(|e| {
            LuatError::CacheError(format!("Failed to read cache directory: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                LuatError::CacheError(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "lua" || ext == "json" {
                        std::fs::remove_file(&path).map_err(|e| {
                            LuatError::CacheError(format!("Failed to remove file: {}", e))
                        })?;
                    }
                }
            }
        }

        Ok(())
    }

    fn contains_key(&self, key: &str) -> bool {
        // Check memory cache first
        if self.memory_cache.contains_key(key) {
            return true;
        }

        // Check file system cache
        let cache_file = self.cache_file_path(key);
        let metadata_file = self.metadata_file_path(key);
        cache_file.exists() && metadata_file.exists()
    }

    fn clone_box(&self) -> Box<dyn Cache> {
        Box::new(Self {
            cache_dir: self.cache_dir.clone(),
            memory_cache: self.memory_cache.clone(),
        })
    }
}

/// Generate cache key from source content
pub fn generate_cache_key(source: &str, dependencies: &[String]) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    dependencies.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    use tempfile::TempDir;

    #[test]
    fn test_memory_cache() {
        let cache = MemoryCache::new(10);

        #[cfg(not(target_arch = "wasm32"))]
        let module = Arc::new(Module::new(
            "test".to_string(),
            "return {}".to_string(),
            vec![],
        ));

        #[cfg(target_arch = "wasm32")]
        let module = Rc::new(Module::new(
            "test".to_string(),
            "return {}".to_string(),
            vec![],
        ));

        // Test set and get
        cache.set("test", module.clone()).unwrap();
        let retrieved = cache.get("test").unwrap().unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.lua_code, "return {}");

        // Test contains_key
        assert!(cache.contains_key("test"));
        assert!(!cache.contains_key("nonexistent"));

        // Test remove
        cache.remove("test").unwrap();
        assert!(!cache.contains_key("test"));
        assert!(cache.get("test").unwrap().is_none());
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
    #[test]
    fn test_filesystem_cache() {
        let temp_dir = TempDir::new().unwrap();
        let cache = FileSystemCache::new(temp_dir.path(), 10).unwrap();

        let module = Arc::new(Module::new(
            "test".to_string(),
            "return { render = function() end }".to_string(),
            vec!["Component".to_string()],
        ));

        // Test set and get
        cache.set("test", module.clone()).unwrap();
        let retrieved = cache.get("test").unwrap().unwrap();
        assert_eq!(retrieved.name, "test");
        assert_eq!(retrieved.dependencies, vec!["Component"]);

        // Test contains_key
        assert!(cache.contains_key("test"));

        // Test persistence (create new cache instance)
        let cache2 = FileSystemCache::new(temp_dir.path(), 10).unwrap();
        let retrieved2 = cache2.get("test").unwrap().unwrap();
        assert_eq!(retrieved2.name, "test");
    }

    #[test]
    fn test_cache_key_generation() {
        let key1 = generate_cache_key("hello", &[]);
        let key2 = generate_cache_key("hello", &[]);
        let key3 = generate_cache_key("world", &[]);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);

        let key4 = generate_cache_key("hello", &["dep".to_string()]);
        assert_ne!(key1, key4);
    }
}
