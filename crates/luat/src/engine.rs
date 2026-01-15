// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! LUAT template engine for compiling and rendering templates.
//!
//! This module provides the core [`Engine`] type that handles the complete
//! template lifecycle: resolution, parsing, compilation, caching, and rendering.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use luat::{Engine, FileSystemResolver};
//!
//! // Create an engine with filesystem resolver and memory cache
//! let resolver = FileSystemResolver::new("./templates");
//! let engine = Engine::with_memory_cache(resolver, 100)?;
//!
//! // Compile and render a template
//! let module = engine.compile_entry("hello.luat")?;
//! let context = engine.to_value(serde_json::json!({ "name": "World" }))?;
//! let html = engine.render(&module, &context)?;
//! ```
//!
//! # Architecture
//!
//! The engine coordinates several subsystems:
//!
//! - **Resolver**: Locates template files by path (filesystem or memory)
//! - **Parser**: Converts template source into an AST
//! - **Transform**: Converts AST to intermediate representation (IR)
//! - **Codegen**: Generates Lua code from IR
//! - **Cache**: Stores compiled modules for reuse
//! - **Lua Runtime**: Executes the generated code with context data
//!
//! # Caching
//!
//! The engine supports pluggable caching strategies:
//!
//! - [`MemoryCache`]: Fast in-memory LRU cache (recommended for most uses)
//! - [`FileSystemCache`]: Persistent disk-based cache for production
//!
//! # Thread Safety
//!
//! On native builds, the engine uses `Arc<Mutex<...>>` for thread-safe caching.
//! On WASM builds, it uses `Rc<RefCell<...>>` for single-threaded operation.

use crate::cache::*;
use crate::codegen::*;
use crate::error::{LuatError, Result};
use crate::parser::parse_template;
use crate::resolver::*;
use crate::transform::*;
use crate::transform::validate_ir;
use crate::sourcemap::BundleSourceMap;
use mlua::LuaSerdeExt;
use mlua::{Lua, Table, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Helper function to convert absolute path to relative path.
/// Used in closures where self is not available.
fn to_relative_path(absolute_path: &str, root_path: &Option<String>) -> String {
    if let Some(root) = root_path {
        if let Ok(relative) = Path::new(absolute_path).strip_prefix(root) {
            return relative.to_string_lossy().to_string();
        }
    }
    // Fallback: return just the filename
    Path::new(absolute_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| absolute_path.to_string())
}

// Conditional imports for thread primitives
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Mutex};

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

/// Main LUAT template engine.
///
/// The engine is generic over the resource resolver type `R`, allowing
/// different template loading strategies (filesystem, memory, network, etc.).
///
/// # Type Parameters
///
/// * `R` - The resource resolver implementation for loading template sources
///
/// # Examples
///
/// ```rust,ignore
/// use luat::{Engine, MemoryResourceResolver, MemoryCache};
///
/// // Using memory resolver for testing
/// let resolver = MemoryResourceResolver::new();
/// resolver.add_template("hello.luat", "<h1>Hello, {props.name}!</h1>");
///
/// let cache = Box::new(MemoryCache::new(100));
/// let engine = Engine::new(resolver, cache)?;
/// ```
#[derive(Debug)]
pub struct Engine<R: ResourceResolver> {
    resolver: R,
    cache: Box<dyn Cache>,
    lua: Lua,
    /// Root path for computing relative paths in error messages
    root_path: Option<String>,
}

/// Wrapper for a Lua value to be used as template context.
///
/// This type wraps an `mlua::Value` for serialization purposes when
/// passing data to template rendering.
#[derive(serde::Serialize)]
pub struct LuatContextValue(pub mlua::Value);

/// Wrapper for a Lua table to be used as template context.
///
/// Provides a serializable context table that can be passed directly
/// to the Lua runtime during template rendering.
#[derive(serde::Serialize)]
pub struct LuatContext(pub mlua::Table);

impl mlua::IntoLuaMulti for LuatContext {
    fn into_lua_multi(self, lua: &mlua::Lua) -> mlua::Result<mlua::MultiValue> {
        self.0.into_lua_multi(lua)
    }
}

impl<R: ResourceResolver> Engine<R> {
    /// Returns a reference to the resolver used by this engine.
    pub fn resolver(&self) -> &R {
        &self.resolver
    }

    /// Sets the root path for computing relative paths in error messages.
    ///
    /// When set, file paths in error messages will be shown relative to this root,
    /// making them shorter and easier to read.
    pub fn set_root_path<P: AsRef<std::path::Path>>(&mut self, root: P) {
        self.root_path = Some(root.as_ref().to_string_lossy().to_string());
    }

    /// Converts an absolute path to a relative path based on the root.
    ///
    /// If root_path is not set, returns just the filename as a fallback.
    fn make_relative_path(&self, absolute_path: &str) -> String {
        use std::path::Path;

        if let Some(root) = &self.root_path {
            if let Ok(relative) = Path::new(absolute_path).strip_prefix(root) {
                return relative.to_string_lossy().to_string();
            }
        }
        // Fallback: return just the filename if path stripping fails
        Path::new(absolute_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| absolute_path.to_string())
    }

    /// Sandboxes the Lua environment by disabling dangerous functions and libraries.
    ///
    /// This removes access to:
    /// - `io` library (file I/O)
    /// - `debug` library (introspection)
    /// - `load`, `loadstring`, `loadfile`, `dofile` (dynamic code execution)
    /// - Most of `os` library (keeps only `os.date`, `os.time`, `os.clock`, `os.difftime`)
    fn sandbox_lua(lua: &Lua, globals: &Table) -> Result<()> {
        // Save load function for internal use before sandboxing
        // This allows the bundle's module loader to work while preventing user code access
        let load_fn: mlua::Function = globals.get("load")?;
        globals.set("__luat_internal_load", load_fn)?;

        // Save safe os functions before removing the library
        let os_table: Table = globals.get("os")?;
        let os_date: mlua::Function = os_table.get("date")?;
        let os_time: mlua::Function = os_table.get("time")?;
        let os_clock: mlua::Function = os_table.get("clock")?;
        let os_difftime: mlua::Function = os_table.get("difftime")?;

        // Disable dangerous libraries
        globals.set("io", mlua::Value::Nil)?;
        globals.set("debug", mlua::Value::Nil)?;

        // Disable dangerous functions (user code cannot use these)
        globals.set("load", mlua::Value::Nil)?;
        globals.set("loadstring", mlua::Value::Nil)?;
        globals.set("loadfile", mlua::Value::Nil)?;
        globals.set("dofile", mlua::Value::Nil)?;

        // Create restricted os table with only safe functions
        let safe_os = lua.create_table()?;
        safe_os.set("date", os_date)?;
        safe_os.set("time", os_time)?;
        safe_os.set("clock", os_clock)?;
        safe_os.set("difftime", os_difftime)?;

        // Replace os with restricted version
        globals.set("os", safe_os)?;

        Ok(())
    }

    /// Creates a new engine with the given resolver and cache.
    ///
    /// This is the low-level constructor. For convenience, prefer
    /// [`with_memory_cache`](Self::with_memory_cache) or
    /// [`with_filesystem_cache`](Self::with_filesystem_cache).
    ///
    /// # Arguments
    ///
    /// * `resolver` - Resource resolver for loading template sources
    /// * `cache` - Cache implementation for storing compiled modules
    ///
    /// # Errors
    ///
    /// Returns an error if the Lua runtime fails to initialize.
    pub fn new(resolver: R, cache: Box<dyn Cache>) -> Result<Self> {
        let lua = Lua::new();
        let globals = lua.globals();

        // Security: Sandbox the Lua environment
        // Disable dangerous libraries and functions while keeping safe ones
        Self::sandbox_lua(&lua, &globals)?;

        globals.set(
            "createContextHelpers",
            lua.create_function(|lua, runtime: Table| {
                let create_context_runtime = runtime.clone();
                // --- NEW FUNCTION: create_context ---
                let create_context = lua.create_function(move |lua, _: ()| {
                    // Expects no arguments after 'self'
                    // Tracing for debugging
                    tracing::debug!("createContext called.");
                    tracing::debug!(
                        "create_context_runtime in closure: {:?}",
                        create_context_runtime
                    );

                    let stack: Table = create_context_runtime.get("context_stack")?;
                    let new_scope = lua.create_table()?; // Create a new empty table for the scope
                    stack.push(new_scope)?; // Push it onto the stack (mlua Table::push for sequences)
                    tracing::debug!("Context stack after create_context: {:?}", stack);
                    Ok(mlua::Value::Nil)
                })?;

                let set_context_runtime = runtime.clone();
                let set_context =
                    lua.create_function(move |_, (k, v): (String, mlua::Value)| {
                        let stack: Table = set_context_runtime.get("context_stack")?;
                        tracing::debug!("Setting context key: {} with value: {:?}", k, v);
                        // Ensure the stack is not empty
                        let top: Table = stack.get(stack.len()?)?;
                        tracing::debug!("Setting context in top scope: {:?}", top);
                        // Set the value in the top scope
                        // If the value is nil, it will remove the key from the table
                        top.set(k, v)?;
                        tracing::debug!("Context after setting: {:?}", top);

                        //Ok(())
                        Ok(mlua::Value::Nil)
                    })?;

                let get_context_runtime = runtime.clone();
                let get_context = lua.create_function(move |_, k: String| {
                    let stack: Table = get_context_runtime.get("context_stack")?;
                    for i in (1..=stack.len()?).rev() {
                        let scope: Table = stack.get(i)?;
                        let val: mlua::Value = scope.get(k.clone())?;
                        if !val.is_nil() {
                            return Ok(val);
                        }
                    }
                    Ok(mlua::Value::Nil)
                })?;

                let ctx_helpers = lua.create_table()?;
                ctx_helpers.set("setContext", set_context)?;
                ctx_helpers.set("getContext", get_context)?;
                ctx_helpers.set("createContext", create_context)?;
                Ok(ctx_helpers)
            })?,
        )?;

        // lua.load(write_stream_module)
        //     .set_name("raisin:write_stream")?
        //     .eval()?;
        // Create the engine instance
        let mut engine = Self {
            resolver,
            cache,
            lua,
            root_path: None,
        };

        // Setup the custom module searcher to resolve Lua modules through our resolver
        engine.setup_custom_searcher()?;
        // Register the json module using the shared implementation
        crate::extensions::json::register_json_module(&engine.lua)?;

        Ok(engine)
    }
    /// Setup custom Lua module searchers that use our cache and resolver
    /// This integrates with Lua's require system to find modules via our resources
    fn setup_custom_searcher(&mut self) -> Result<()> {
        // Get the package table from Lua globals
        let globals = &self.lua.globals();
        let package: Table = globals.get("package")?;

        // Get the existing searchers table
        let searchers: Table = package.get("searchers")?;
        // Make sure the json module is properly preloaded
        // This explicitly adds it to package.preload

        // Create clones of the engine components for the closures
        // Native: Use Arc<Mutex<...>> for thread safety
        // WASM: Use Rc<RefCell<...>> (single-threaded)
        #[cfg(not(target_arch = "wasm32"))]
        let resolver_clone = Arc::new(Mutex::new(self.resolver.clone_box()));
        #[cfg(not(target_arch = "wasm32"))]
        let cache_clone = Arc::new(Mutex::new(self.cache.clone_box()));
        #[cfg(not(target_arch = "wasm32"))]
        let cache_clone2 = Arc::clone(&cache_clone);

        #[cfg(target_arch = "wasm32")]
        let resolver_clone = Rc::new(RefCell::new(self.resolver.clone_box()));
        #[cfg(target_arch = "wasm32")]
        let cache_clone = Rc::new(RefCell::new(self.cache.clone_box()));
        #[cfg(target_arch = "wasm32")]
        let cache_clone2 = Rc::clone(&cache_clone);

        // Clone root_path for use in closures (for relative path display in errors)
        let root_path_for_searcher = self.root_path.clone();

        // 1. SEARCHER 1: CACHE-BASED SEARCHER
        // This searcher checks if the module is already in the cache
        let cache_searcher = self.lua.create_function(move |lua, module_name: String| {
            //println!("DEBUG: Cache searcher looking for module: {}", module_name);

            // Get normalized module path
            let is_luat = module_name.ends_with(".luat");
            let module_path = if !is_luat && !module_name.ends_with(".lua") {
                format!("{}.luat", module_name)
            } else {
                module_name.clone()
            };

            // Try to get from cache
            let cache_key = format!("module:{}", module_path);

            #[cfg(not(target_arch = "wasm32"))]
            let cache = cache_clone.lock().unwrap();
            #[cfg(target_arch = "wasm32")]
            let cache = cache_clone.borrow();

            // Try cache lookup with exact key
            if let Ok(Some(module)) = cache.get(&cache_key) {
                //println!("DEBUG: Found module in cache with exact key: {}", cache_key);
                // Found in cache, create loader function
                match lua.load(&module.lua_code).into_function() {
                    Ok(loader) => {
                        // Return the loader function and the module path
                        return Ok((Some(loader), Some(format!("cache:{}", module_path))));
                    }
                    Err(_e) => {
                        //println!("DEBUG: Error loading module from cache: {:?}", _e);
                        return Ok((None, None)); // Loading error
                    }
                }
            }

            //println!("DEBUG: Module not found in cache with key: {}", cache_key);

            // Not found in cache with exact key
            Ok((None, None))
        })?;

        // 2. SEARCHER 2: RESOLVER-BASED SEARCHER
        // This searcher uses the resolver to find modules, compile them, and cache them
        let resolver_searcher = self.lua.create_function(move |lua, module_name: String| {
            // We'll keep the original module name exactly as requested in require()
            let original_module_name = module_name.clone();

            // Check if it's a .luat file or not (kept for future use)
            let _is_luat = module_name.ends_with(".luat");
            let _module_path = module_name.clone(); // Keep for future use when extension handling is added

            // Get the importer path from Lua registry
            // The current module path is stored in __luat_current_module before execution
            let importer_path: String = lua
                .named_registry_value("__luat_current_module")
                .unwrap_or_default();

            // Try to resolve the module through our resolver
            #[cfg(not(target_arch = "wasm32"))]
            let resolver = resolver_clone.lock().unwrap();
            #[cfg(not(target_arch = "wasm32"))]
            let cache = cache_clone2.lock().unwrap();

            #[cfg(target_arch = "wasm32")]
            let resolver = resolver_clone.borrow();
            #[cfg(target_arch = "wasm32")]
            let cache = cache_clone2.borrow();

            //println!(
            //    "DEBUG: Resolver searcher trying to resolve '{}' from importer '{}'",
            //    module_path, importer_path
            //);

            // First try the original module name exactly as provided in require()
            match resolver.resolve(&importer_path, &original_module_name) {
                Ok(resolved) => {
                    let (content, source_hash) = if resolved.path.ends_with(".luat") {
                        // For .luat files, compile them to Lua
                        // Parse template
                        match parse_template(&resolved.source) {
                            Ok(mut ast) => {
                                // Store the resolved path in the AST for future reference
                                ast.path = Some(resolved.path.clone());

                                // Transform to IR
                                match transform_ast(ast) {
                                    Ok(ir) => {
                                        // Extract module name for codegen
                                        let module_name = std::path::Path::new(&resolved.path)
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown")
                                            .to_string();

                                        let components: Vec<String> =
                                            ir.components.clone().into_iter().collect();
                                        match generate_lua_code(ir, &module_name) {
                                            Ok(lua_code) => {
                                                // Calculate hash of source code
                                                let mut hasher =
                                                    std::collections::hash_map::DefaultHasher::new(
                                                    );
                                                resolved.source.hash(&mut hasher);
                                                let hash = hasher.finish();

                                                // Create a module and cache it
                                                #[cfg(not(target_arch = "wasm32"))]
                                                let module = Arc::new(Module::new(
                                                    module_name.clone(),
                                                    lua_code.clone(),
                                                    components,
                                                ));
                                                #[cfg(target_arch = "wasm32")]
                                                let module = Rc::new(Module::new(
                                                    module_name.clone(),
                                                    lua_code.clone(),
                                                    components,
                                                ));

                                                // Use the canonical path as cache key
                                                let cache_key = format!("module:{}", resolved.path);
                                                let source_key = format!("source:{}", hash);

                                                //println!(
                                                //    "DEBUG: Caching module at key: {}",
                                                //    cache_key
                                                //);

                                                // Cache under both the original module name from require() and the resolved path
                                                let _ = cache.set(&cache_key, module.clone());
                                                let _ = cache.set(
                                                    &format!("module:{}", original_module_name),
                                                    module.clone(),
                                                );

                                                // Also cache under source hash
                                                let _ = cache.set(&source_key, module.clone());

                                                (lua_code, Some(hash))
                                            }
                                            Err(e) => {
                                                return Err(mlua::Error::RuntimeError(
                                                    format!("Code generation error in {}: {}", resolved.path, e)
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        return Err(mlua::Error::RuntimeError(
                                            format!("Transform error in {}: {}", resolved.path, e)
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                return Err(mlua::Error::RuntimeError(
                                    format!("Parse error in {}: {}", resolved.path, e)
                                ));
                            }
                        }
                    } else {
                        // For .lua files, use directly (no caching for plain Lua files)
                        //println!("DEBUG: Direct Lua file (not LUAT): {}", resolved.path);
                        (resolved.source, None)
                    };

                    // Create a loader function for the module
                    // Set the chunk name to relative path for readable error messages
                    // The @ prefix tells Lua this is a file path
                    let display_path = to_relative_path(&resolved.path, &root_path_for_searcher);
                    let chunk_name = format!("@{}", display_path);
                    match lua.load(&content).set_name(&chunk_name).into_function() {
                        Ok(loader) => {
                            // Return the loader function and the module path with source info
                            // Use the original module name from require() as the identifier
                            let search_path = if let Some(hash) = source_hash {
                                format!("resolver:{}:{}", original_module_name, hash)
                            } else {
                                format!("resolver:{}", original_module_name)
                            };
                            Ok((Some(loader), Some(search_path)))
                        }
                        Err(_e) => {
                            //println!("DEBUG: Error loading module: {:?}", _e);
                            Ok((None, None)) // Loading error
                        }
                    }
                }
                Err(_e) => {
                    eprintln!(
                        "DEBUG: Failed to resolve '{}' with importer '{}': {:?}",
                        original_module_name, importer_path, _e
                    );

                    // If original_module_name failed, try with .luat extension appended
                    if !original_module_name.ends_with(".luat")
                        && !original_module_name.ends_with(".lua")
                    {
                        let with_ext = format!("{}.luat", original_module_name);
                        //println!("DEBUG: Trying with .luat extension: '{}'", with_ext);

                        match resolver.resolve(&importer_path, &with_ext) {
                            Ok(resolved) => {
                                //println!(
                                //    "DEBUG: Successfully resolved with extension: {}",
                                //    resolved.path
                                //);
                                // Process successfully resolved module with extension...
                                // (Similar to above processing)

                                // Simplified for now - this is duplicate code
                                // Set chunk name to relative path for readable error messages
                                let display_path = to_relative_path(&resolved.path, &root_path_for_searcher);
                                let chunk_name = format!("@{}", display_path);
                                match lua.load(&resolved.source).set_name(&chunk_name).into_function() {
                                    Ok(loader) => {
                                        Ok((
                                            Some(loader),
                                            Some(format!("resolver:{}", with_ext)),
                                        ))
                                    }
                                    Err(_) => Ok((None, None)),
                                }
                            }
                            Err(_) => Ok((None, None)), // Not found even with extension, try next searcher
                        }
                    } else {
                        Ok((None, None)) // Not found, try next searcher
                    }
                }
            }
        })?;

        // Get the length of the existing searchers table
        let searchers_len = searchers.raw_len();
        // println!("DEBUG: Current searchers length: {}", searchers_len);
        // Insert our custom searchers after the existing ones
        // This preserves the standard searchers while adding our custom ones
        searchers.set(searchers_len + 1, cache_searcher)?;
        searchers.set(searchers_len + 2, resolver_searcher)?;
        // println!("DEBUG: Added custom searchers to package.searchers");
        // println!("SEarchers: {:?}", searchers);
        // println!("Searchers length: {}", searchers.raw_len());

        Ok(())
    }

    /// Creates a new engine with an in-memory LRU cache.
    ///
    /// This is the recommended constructor for most use cases.
    ///
    /// # Arguments
    ///
    /// * `resolver` - Resource resolver for loading template sources
    /// * `cache_size` - Maximum number of compiled modules to cache
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let resolver = FileSystemResolver::new("./templates");
    /// let engine = Engine::with_memory_cache(resolver, 100)?;
    /// ```
    pub fn with_memory_cache(resolver: R, cache_size: usize) -> Result<Self> {
        let cache = Box::new(MemoryCache::new(cache_size));
        Self::new(resolver, cache)
    }

    /// Creates a new engine with a filesystem-backed cache.
    ///
    /// Compiled modules are persisted to disk for faster startup on subsequent runs.
    /// Only available on native builds with the `filesystem` feature enabled.
    ///
    /// # Arguments
    ///
    /// * `resolver` - Resource resolver for loading template sources
    /// * `cache_dir` - Directory path for storing cached modules
    /// * `memory_size` - Size of in-memory LRU cache on top of disk cache
    #[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
    pub fn with_filesystem_cache<P: AsRef<std::path::Path>>(
        resolver: R,
        cache_dir: P,
        memory_size: usize,
    ) -> Result<Self> {
        let cache = Box::new(FileSystemCache::new(cache_dir, memory_size)?);
        Self::new(resolver, cache)
    }

    /// Compiles a template entry point and returns the compiled module.
    ///
    /// This method resolves the template file, parses it, transforms the AST
    /// to IR, generates Lua code, and caches the result for reuse.
    ///
    /// # Arguments
    ///
    /// * `entry` - Path to the template file (relative to resolver root)
    ///
    /// # Returns
    ///
    /// A shared pointer to the compiled [`Module`] containing the Lua code.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The template file cannot be found
    /// - Parsing fails due to syntax errors
    /// - Transformation or code generation fails
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let module = engine.compile_entry("pages/index.luat")?;
    /// ```
    pub fn compile_entry(&self, entry: &str) -> Result<SharedPtr<Module>> {
        //println!("DEBUG: compile_entry called for entry: {}", entry);

        let mut compiled_modules = HashMap::new();
        let mut pending = Vec::new();

        pending.push(entry.to_string());
        //println!("DEBUG: Initial pending modules: {:?}", pending);

        while let Some(module_path) = pending.pop() {
            //println!("DEBUG: Processing module path: {}", module_path);

            if compiled_modules.contains_key(&module_path) {
                //println!("DEBUG: Module {} already compiled, skipping", module_path);
                continue;
            }

            //println!("DEBUG: Compiling module: {}", module_path);

            if module_path.ends_with(".lua") {
                //println!("DEBUG: Module {} is lua", module_path);

                //println!("DEBUG: Dependency {} is a plain Lua file", module_path);
                let resolved = self.resolver.resolve("", &module_path)?;

                #[cfg(not(target_arch = "wasm32"))]
                let module = Arc::new(Module::with_path(
                    module_path.clone(),
                    resolved.source,
                    vec![], // Plain Lua files don't have dependencies
                    resolved.path,
                ));
                #[cfg(target_arch = "wasm32")]
                let module = Rc::new(Module::with_path(
                    module_path.clone(),
                    resolved.source,
                    vec![], // Plain Lua files don't have dependencies
                    resolved.path,
                ));

                // Cache the module
                compiled_modules.insert(module_path, module);
            } else {
                // Use the resource resolver to get the actual file content
                let resolved = match self.resolver.resolve("", &module_path) {
                    Ok(r) => {
                        //println!("DEBUG: Successfully resolved file for module {}: {}", module_path, r.path);
                        r
                    },
                    Err(e) => {
                        //println!("DEBUG: Error resolving module {}: {:?}", module_path, e);
                        return Err(e);
                    }
                };

                // Now compile the template with actual file content
                // Pass the resolved path so it can be used for relative import resolution
                let module = match self.compile_template_string_with_path(
                    &module_path,
                    &resolved.source,
                    Some(resolved.path),
                ) {
                    Ok(m) => {
                        //println!("DEBUG: Successfully compiled module {}", module_path);
                        //if !pending.is_empty() {
                        //    println!("DEBUG: New pending modules: {:?}", pending);
                        //}
                        m
                    }
                    Err(e) => {
                        //println!("DEBUG: Error compiling module {}: {:?}", module_path, e);
                        return Err(e);
                    }
                };

                compiled_modules.insert(module_path, module);
            }
        }

        //println!(
        //    "DEBUG: All modules compiled: {:?}",
        //    compiled_modules.keys().collect::<Vec<_>>()
        //);

        match compiled_modules.get(entry) {
            Some(module) => {
                //println!("DEBUG: Returning compiled entry module: {}", entry);

                // Cache the compiled module
                let cache_key = format!("module:{}", entry);
                //println!("DEBUG: Caching module with key: {}", cache_key);

                let _ = self.cache.set(&cache_key, module.clone());
                //match self.cache.set(&cache_key, module.clone()) {
                //    Ok(_) => println!("DEBUG: Successfully cached module: {}", entry),
                //    Err(e) => println!("DEBUG: Error caching module: {:?}", e),
                //}

                Ok(module.clone())
            }
            None => {
                //println!(
                //    "DEBUG: Entry module not found in compiled modules: {}",
                //    entry
                //);
                Err(LuatError::ModuleNotFound(entry.to_string()))
            }
        }
    }

    /// Renders a compiled template with the given context data.
    ///
    /// This method executes the template's Lua code with the provided context,
    /// returning the rendered HTML string.
    ///
    /// # Arguments
    ///
    /// * `module` - A compiled module from [`compile_entry`](Self::compile_entry)
    /// * `context` - Lua value containing template data (typically from [`to_value`](Self::to_value))
    ///
    /// # Returns
    ///
    /// The rendered HTML string.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The module doesn't have a `render` function
    /// - Lua execution fails at runtime
    /// - A required dependency cannot be loaded
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let module = engine.compile_entry("hello.luat")?;
    /// let context = engine.to_value(serde_json::json!({ "name": "World" }))?;
    /// let html = engine.render(&module, &context)?;
    /// assert!(html.contains("Hello, World"));
    /// ```
    pub fn render(&self, module: &Module, context: &Value) -> Result<String> {
        // First, ensure all dependencies are loaded recursively
        //println!("DEBUG: Loading dependencies for module: {}", module.name);
        if !module.dependencies.is_empty() {
            //println!("DEBUG: Dependencies to load: {:?}", module.dependencies);

            // Load each dependency
            let mut loaded_deps: Vec<String> = Vec::new();

            for dep in &module.dependencies {
                if !loaded_deps.contains(dep) {
                    // Try to resolve and load the dependency
                    match self.resolver.resolve("", dep) {
                        Ok(resolved) => {
                            // Successfully resolved the dependency
                            //println!("DEBUG: Successfully resolved dependency: {} to path: {}", dep, resolved.path);

                            // Create a module from the resolved dependency
                            let compiled = if dep.ends_with(".luat") {
                                // Parse and compile the template
                                let ast = parse_template(&resolved.source)?;
                                let ir = transform_ast(ast)?;
                                validate_ir(&ir)?;

                                let module_name = std::path::Path::new(dep)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string();

                                generate_lua_code(ir, &module_name)?
                            } else {
                                // For .lua files, use directly
                                resolved.source
                            };

                            // Load the module into the Lua state
                            // Store the path in registry for nested requires, then set chunk name
                            self.lua.set_named_registry_value("__luat_current_module", resolved.path.clone())?;
                            let chunk_name = format!("@{}", self.make_relative_path(&resolved.path));
                            let lua_module = match self.lua.load(&compiled).set_name(&chunk_name).eval::<Table>() {
                                Ok(m) => m,
                                Err(e) => {
                                    //println!("DEBUG: Error loading dependency {}: {:?}", dep, e);
                                    return Err(LuatError::LuaError(e));
                                }
                            };

                            // Add to package.loaded so it can be required
                            let globals = self.lua.globals();
                            let package: Table = globals.get("package")?;
                            let loaded: Table = package.get("loaded")?;
                            loaded.set(dep.clone(), lua_module)?;

                            loaded_deps.push(dep.clone());
                        },
                        Err(_e) => {
                            //println!("DEBUG: Error resolving dependency {}: {:?}", dep, e);
                            // Continue with other dependencies even if one fails
                            // This might be desired behavior or we could return an error
                        }
                    }
                }
            }

            //if !loaded_deps.is_empty() {
            //    println!("DEBUG: Successfully loaded dependencies: {:?}", loaded_deps);
            //}
        }
        //else {
        //    println!("DEBUG: No dependencies declared for this module");
        //}

        // Load the Lua module
        // Store the current module path in registry so nested require() calls can find it
        let module_path = module.path.clone().unwrap_or_else(|| module.name.clone());
        self.lua.set_named_registry_value("__luat_current_module", module_path.clone())?;

        let chunk = self.lua.load(&module.lua_code);
        let chunk = chunk.set_name(format!("@{}", self.make_relative_path(&module_path)));
        let lua_func = match chunk.eval::<Table>() {
            Ok(f) => f,
            Err(e) => {
                // Translate error line numbers using source map if available
                if let Some(source_map) = &module.source_map {
                    let original_msg = e.to_string();
                    let translated_msg = source_map.translate_error(&original_msg);
                    if translated_msg != original_msg {
                        return Err(LuatError::TemplateRuntimeError {
                            template: module.path.clone().unwrap_or_else(|| module.name.clone()),
                            message: translated_msg,
                            lua_traceback: None,
                            source_context: None,
                        });
                    }
                }
                return Err(LuatError::LuaError(e));
            }
        };

        // Check if the module has a render function
        if !lua_func.contains_key("render")? {
            //println!("DEBUG: Module does not have a render function");
            return Err(LuatError::InvalidTemplate(
                "Module doesn't have a render function".to_string(),
            ));
        }

        let render_func = lua_func.get::<mlua::Function>("render")?;

        // Get the shared runtime from registry (initialized by handle_page_route)
        // This preserves the context_stack across all renders in a request
        let runtime: Table = match self.lua.named_registry_value::<Table>("__luat_request_runtime") {
            Ok(existing) => existing,
            Err(_) => {
                // Fallback: create a temporary runtime for standalone renders
                let runtime = self.lua.create_table()?;
                let stack: Table = self.lua.create_sequence_from::<Table>(vec![])?;
                runtime.set("context_stack", stack)?;
                runtime
            }
        };

        // Call render function with both context and runtime
        let result: String = match render_func.call((self.lua.to_value(context)?, &runtime)) {
            Ok(r) => r,
            Err(e) => {
                // Translate error line numbers using source map if available
                if let Some(source_map) = &module.source_map {
                    let original_msg = e.to_string();
                    let translated_msg = source_map.translate_error(&original_msg);
                    if translated_msg != original_msg {
                        // Return custom error with translated line numbers
                        return Err(LuatError::TemplateRuntimeError {
                            template: module.path.clone().unwrap_or_else(|| module.name.clone()),
                            message: translated_msg,
                            lua_traceback: None,
                            source_context: None,
                        });
                    }
                }
                return Err(LuatError::LuaError(e));
            }
        };

        Ok(result)
    }

    /// Load a dependency module and make it available to Lua
    #[allow(dead_code)]
    fn load_dependency(&self, module_path: &str) -> Result<()> {
        // If it's already in the cache, use the cached version
        let cache_key = format!("module:{}", module_path);
        let module = if let Some(cached) = self.cache.get(&cache_key)? {
            cached
        } else {
            // Otherwise, compile it
            self.compile_template_string(module_path, module_path)?
        };

        // Extract module name (remove extension)
        let module_name = std::path::Path::new(module_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Load the module into Lua state
        let lua_module = self.lua.load(&module.lua_code).eval::<Table>()?;

        // Add it to package.loaded
        let globals = self.lua.globals();
        let package: Table = globals.get("package")?;
        let loaded: Table = package.get("loaded")?;
        loaded.set(module_path, lua_module.clone())?;

        // Also set it as a global variable for direct access (both with and without extension)
        globals.set(module_name.clone(), lua_module.clone())?;

        // Make it available both as "Card.luat" and as "Card" since the component lookup uses "Card"
        if module_path.ends_with(".luat") {
            globals.set(module_path, lua_module)?;
        }

        Ok(())
    }

    /// Renders a template from source string without caching.
    ///
    /// Useful for one-off template rendering or testing. For repeated renders,
    /// prefer [`compile_entry`](Self::compile_entry) + [`render`](Self::render).
    ///
    /// # Arguments
    ///
    /// * `source` - Raw template source code
    /// * `context` - HashMap of template data
    pub fn render_source(&self, source: &str, context: &HashMap<String, Value>) -> Result<String> {
        // Parse template
        let ast = parse_template(source)?;

        // Transform to IR
        let ir = transform_ast(ast)?;
        validate_ir(&ir)?;

        // Generate Lua code with a consistent module name
        let module_name = "source_template"; // Use a consistent module name
        let lua_code = generate_lua_code(ir, module_name)?;

        // Create temporary module
        let module = Module::new(module_name.to_string(), lua_code, vec![]);

        // Load the module in the Lua state directly
        let lua_module = self.lua.load(&module.lua_code).eval::<mlua::Table>()?;

        // Get the render function
        let render_func: mlua::Function = lua_module.get("render")?;

        // Create a Lua table from the context
        let props = self.lua.create_table()?;
        for (key, value) in context {
            props.set(key.clone(), value.clone())?;
        }

        // Call the render function directly
        let result: String = render_func.call(props)?;

        Ok(result)
    }

    /// Creates an empty Lua table for building template context.
    pub fn create_context(&self) -> LuatContext {
        let table = self.lua.create_table().unwrap();
        LuatContext(table)
    }

    /// Converts any serializable Rust value into a Lua value.
    ///
    /// Use this method to prepare context data for template rendering.
    /// Supports any type implementing `serde::Serialize`, including
    /// `serde_json::Value` for dynamic JSON data.
    ///
    /// # Arguments
    ///
    /// * `value` - Any serializable Rust value
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // From a struct
    /// #[derive(serde::Serialize)]
    /// struct PageData { title: String, items: Vec<String> }
    /// let context = engine.to_value(PageData { ... })?;
    ///
    /// // From JSON
    /// let context = engine.to_value(serde_json::json!({
    ///     "title": "Hello",
    ///     "items": ["one", "two", "three"]
    /// }))?;
    /// ```
    pub fn to_value<T: serde::Serialize>(&self, value: T) -> Result<Value> {
        self.lua.to_value(&value).map_err(LuatError::LuaError)
    }

    /// Converts a serializable value into a wrapped Lua context value.
    ///
    /// Similar to [`to_value`](Self::to_value) but returns a
    /// [`LuatContextValue`] wrapper for specific use cases.
    pub fn create_context_value<T: serde::Serialize>(&self, value: T) -> Result<LuatContextValue> {
        match self.lua.to_value(&value) {
            Ok(val) => Ok(LuatContextValue(val)),
            Err(e) => Err(LuatError::LuaError(e)),
        }
    }

    /// Compiles Lua code into bytecode for distribution.
    ///
    /// Bytecode can be loaded faster than source and protects source code.
    /// Use with [`preload_bundle_code_from_binary`](Self::preload_bundle_code_from_binary).
    ///
    /// # Arguments
    ///
    /// * `lua_code` - Lua source code to compile
    ///
    /// # Returns
    ///
    /// Binary bytecode that can be stored and loaded later.
    pub fn compile_bundle(&self, lua_code: &str) -> Result<Vec<u8>> {
        let func = self
            .lua
            .load(lua_code)
            .set_name("@luat_bundle")
            .into_function()?;
        let bytecode = func.dump(false);
        Ok(bytecode)
    }

    /// Renders a template from a pre-loaded bundle module asynchronously.
    ///
    /// Only available when the `async-lua` feature is enabled.
    #[cfg(feature = "async-lua")]
    pub async fn render_from_bundle(&self, module_name: &str, context: &Value) -> Result<String> {
        // println!("DEBUG: Rendering from bundle module: {}", module_name);
        let require: mlua::Function = self.lua.globals().get("require")?;
        // println!("DEBUG: Calling require for module: {}", module_name);
        let module: Table = require.call(module_name)?;
        // println!("DEBUG: Module loaded: {:?}", module);
        if !module.contains_key("render")? {
            return Err(LuatError::InvalidTemplate(format!(
                "Module '{}' has no 'render' function",
                module_name
            )));
        }

        let render_func: mlua::Function = module.get("render")?;

        // Get the shared runtime from registry (initialized by handle_page_route)
        // This preserves the context_stack across all renders in a request
        let runtime: Table = match self.lua.named_registry_value::<Table>("__luat_request_runtime") {
            Ok(existing) => existing,
            Err(_) => {
                // Fallback: create a temporary runtime for standalone renders
                let runtime = self.lua.create_table()?;
                let stack: Table = self.lua.create_sequence_from::<Table>(vec![])?;
                runtime.set("context_stack", stack)?;
                runtime
            }
        };

        let result: String = render_func.call_async((context, &runtime)).await?;
        Ok(result)
    }

    /// Loads Lua code directly into the engine's runtime.
    ///
    /// The code is executed immediately, making any defined modules
    /// or functions available for subsequent `require()` calls.
    pub fn preload_bundle_code(&self, lua_code: &str) -> Result<()> {
        self.lua.load(lua_code).set_name("@luat_bundle").exec()?;
        Ok(())
    }

    /// Loads pre-compiled Lua bytecode into the engine's runtime.
    ///
    /// Use with bytecode produced by [`compile_bundle`](Self::compile_bundle).
    pub fn preload_bundle_code_from_binary(&self, bytecode: &[u8]) -> Result<()> {
        let func = self.lua.load(bytecode).into_function()?;
        let _: () = func.call(())?;
        Ok(())
    }

    /// Bundles multiple template sources into a single Lua file.
    ///
    /// Creates a self-contained bundle with all templates and their
    /// dependencies, suitable for distribution or embedding.
    ///
    /// # Arguments
    ///
    /// * `sources` - Vec of (path, source_code) tuples
    /// * `progress` - Callback invoked with (current, total) progress
    ///
    /// # Returns
    ///
    /// A tuple of (bundle string, source map for error translation).
    pub fn bundle_sources<F>(
        &self,
        sources: Vec<(String, String)>,
        mut progress: F,
    ) -> Result<(String, BundleSourceMap)>
    where
        F: FnMut(usize, usize),
    {
        // Compile all sources first
        let mut compiled_sources = Vec::new();

        for (i, (name, source)) in sources.iter().enumerate() {
            progress(i * 2, sources.len() * 2);

            // Parse and compile the template
            let ast = parse_template(source)?;
            let ir = transform_ast(ast)?;
            validate_ir(&ir)?;

            let lua_code = if name.ends_with(".luat") {
                generate_lua_code(ir, name)?
            } else {
                // For .lua files, use the source directly
                source.clone()
            };

            compiled_sources.push((name.clone(), lua_code));

            progress(i * 2 + 1, sources.len() * 2);
        }

        // Order sources based on their dependencies
        let ordered_sources = match crate::dependencies::order_sources(compiled_sources) {
            Ok(sources) => sources,
            Err(err) => {
                return Err(LuatError::InvalidTemplate(format!(
                    "Failed to order sources by dependency: {}",
                    err
                )))
            }
        };

        // Bundle the ordered sources
        bundle_sources(ordered_sources, progress)
    }

    /// Bundles multiple sources with source map for debugging.
    ///
    /// Like [`bundle_sources`](Self::bundle_sources) but includes source mapping
    /// information for better error messages and debugging.
    pub fn bundle_sources_with_sourcemap<F>(
        &self,
        sources: Vec<(String, String)>,
        mut progress_callback: F,
    ) -> Result<(String, BundleSourceMap)>
    where
        F: FnMut(u8, usize),
    {
        let mut bundle = String::new();
        let mut source_map = BundleSourceMap::new();
        
        // Add source map table
        bundle.push_str("-- SOURCE MAP TABLE\n");
        bundle.push_str("local __source_map = {}\n");
        
        // Add enhanced error handler
        bundle.push_str(r#"
-- Enhanced error handler
local function __wrap_error(module_name, error_msg, traceback)
    local enhanced_msg = string.format(
        "Error in module '%s': %s\nTraceback:\n%s",
        module_name,
        error_msg,
        traceback or debug.traceback()
    )
    error(enhanced_msg, 0)
end

-- Wrap require to add module context
local __original_require = require
local function __enhanced_require(module_name)
    local ok, result = pcall(__original_require, module_name)
    if not ok then
        __wrap_error(module_name, result, debug.traceback())
    end
    return result
end
_G.require = __enhanced_require
"#);

        // Compile all sources first
        let mut compiled_sources = Vec::new();
        let total = sources.len();
        
        for (i, (name, source)) in sources.iter().enumerate() {
            let progress = ((i as f32 / total as f32) * 100.0) as u8;
            progress_callback(progress, total);

            // Parse and compile the template
            let compiled = match self.compile_template_string(name, source) {
                Ok(module) => (name.clone(), module.lua_code.clone()),
                Err(e) => {
                    // Include parsing errors in the compilation but mark them
                    (name.clone(), format!("-- PARSE ERROR: {}\nreturn {{ parse_error = true, error = [[{}]] }}", e, e))
                }
            };
            
            compiled_sources.push(compiled);
        }

        // Order sources based on their dependencies
        let ordered_sources = match crate::dependencies::order_sources(compiled_sources) {
            Ok(sources) => sources,
            Err(err) => {
                return Err(LuatError::InvalidTemplate(format!(
                    "Failed to order sources by dependency: {}",
                    err
                )))
            }
        };

        // Bundle the ordered sources
        bundle.push_str("\n-- BUNDLED MODULES\n");
        bundle.push_str("local __module_loaders = {}\n");
        bundle.push_str("local __modules = {}\n");
        bundle.push_str("local __original_require = require\n\n");

        bundle.push_str("local function __normalize_path(path)\n");
        bundle.push_str("  path = string.gsub(path, \"\\\\\", \"/\")\n");
        bundle.push_str("  local parts = {}\n");
        bundle.push_str("  for part in string.gmatch(path, \"[^/]+\") do\n");
        bundle.push_str("    if part == \"..\" then\n");
        bundle.push_str("      if #parts > 0 then table.remove(parts) end\n");
        bundle.push_str("    elseif part ~= \".\" and part ~= \"\" then\n");
        bundle.push_str("      table.insert(parts, part)\n");
        bundle.push_str("    end\n");
        bundle.push_str("  end\n");
        bundle.push_str("  return table.concat(parts, \"/\")\n");
        bundle.push_str("end\n\n");

        bundle.push_str("local function __dirname(path)\n");
        bundle.push_str("  if not path or path == \"\" then return \"\" end\n");
        bundle.push_str("  local dir = string.match(path, \"^(.*)/\")\n");
        bundle.push_str("  if not dir then return \"\" end\n");
        bundle.push_str("  return dir\n");
        bundle.push_str("end\n\n");

        bundle.push_str("local function __expand_alias(name)\n");
        bundle.push_str("  if string.sub(name, 1, 5) == \"$lib/\" then\n");
        bundle.push_str("    return \"lib/\" .. string.sub(name, 6)\n");
        bundle.push_str("  end\n");
        bundle.push_str("  return name\n");
        bundle.push_str("end\n\n");

        bundle.push_str("local function __add_candidates(candidates, base)\n");
        bundle.push_str("  if base == \"\" then return end\n");
        bundle.push_str("  table.insert(candidates, base)\n");
        bundle.push_str("  if not string.match(base, \"%.luat$\") and not string.match(base, \"%.lua$\") then\n");
        bundle.push_str("    table.insert(candidates, base .. \".luat\")\n");
        bundle.push_str("    table.insert(candidates, base .. \".lua\")\n");
        bundle.push_str("  end\n");
        bundle.push_str("end\n\n");

        bundle.push_str("local function __resolve_module(name, importer)\n");
        bundle.push_str("  local require_map = rawget(_G, \"__require_map\")\n");
        bundle.push_str("  if require_map ~= nil then\n");
        bundle.push_str("    local importer_map = require_map[importer] or require_map[\"\"]\n");
        bundle.push_str("    if importer_map and importer_map[name] then\n");
        bundle.push_str("      local resolved = importer_map[name]\n");
        bundle.push_str("      if __modules[resolved] ~= nil then\n");
        bundle.push_str("        return \"module\", resolved\n");
        bundle.push_str("      end\n");
        bundle.push_str("      if __module_loaders[resolved] ~= nil then\n");
        bundle.push_str("        return \"loader\", resolved\n");
        bundle.push_str("      end\n");
        bundle.push_str("      if __server_sources and __server_sources[resolved] ~= nil then\n");
        bundle.push_str("        return \"server\", resolved\n");
        bundle.push_str("      end\n");
        bundle.push_str("    end\n");
        bundle.push_str("  end\n");
        bundle.push_str("  local candidates = {}\n");
        bundle.push_str("  local expanded = __expand_alias(name)\n");
        bundle.push_str("  local base_dir = \"\"\n");
        bundle.push_str("  if importer and importer ~= \"\" then\n");
        bundle.push_str("    base_dir = __dirname(importer)\n");
        bundle.push_str("  end\n");
        bundle.push_str("  if string.sub(expanded, 1, 1) == \"/\" then\n");
        bundle.push_str("    __add_candidates(candidates, __normalize_path(string.sub(expanded, 2)))\n");
        bundle.push_str("  elseif string.sub(expanded, 1, 2) == \"./\" or string.sub(expanded, 1, 3) == \"../\" then\n");
        bundle.push_str("    local joined = expanded\n");
        bundle.push_str("    if base_dir ~= \"\" then joined = base_dir .. \"/\" .. expanded end\n");
        bundle.push_str("    __add_candidates(candidates, __normalize_path(joined))\n");
        bundle.push_str("  else\n");
        bundle.push_str("    if base_dir ~= \"\" then\n");
        bundle.push_str("      __add_candidates(candidates, __normalize_path(base_dir .. \"/\" .. expanded))\n");
        bundle.push_str("    end\n");
        bundle.push_str("    __add_candidates(candidates, __normalize_path(expanded))\n");
        bundle.push_str("  end\n");
        bundle.push_str("  local basename = string.match(expanded, \"([^/]+)$\") or expanded\n");
        bundle.push_str("  if basename ~= expanded then\n");
        bundle.push_str("    if base_dir ~= \"\" then\n");
        bundle.push_str("      __add_candidates(candidates, __normalize_path(base_dir .. \"/\" .. basename))\n");
        bundle.push_str("    end\n");
        bundle.push_str("    __add_candidates(candidates, __normalize_path(basename))\n");
        bundle.push_str("  end\n");
        bundle.push_str("  for _, key in ipairs(candidates) do\n");
        bundle.push_str("    if __modules[key] ~= nil then\n");
        bundle.push_str("      return \"module\", key\n");
        bundle.push_str("    end\n");
        bundle.push_str("    if __module_loaders[key] ~= nil then\n");
        bundle.push_str("      return \"loader\", key\n");
        bundle.push_str("    end\n");
        bundle.push_str("    if __server_sources and __server_sources[key] ~= nil then\n");
        bundle.push_str("      return \"server\", key\n");
        bundle.push_str("    end\n");
        bundle.push_str("  end\n");
        bundle.push_str("  return nil\n");
        bundle.push_str("end\n\n");

        bundle.push_str("local function __load_server_module(key)\n");
        bundle.push_str("  if package.loaded[key] ~= nil then return package.loaded[key] end\n");
        bundle.push_str("  if not __server_sources then return nil end\n");
        bundle.push_str("  local source = __server_sources[key]\n");
        bundle.push_str("  if not source then return nil end\n");
        bundle.push_str("  local prev = _G.__luat_current_module\n");
        bundle.push_str("  _G.__luat_current_module = key\n");
        bundle.push_str("  local fn = load(source, \"@\" .. key)\n");
        bundle.push_str("  local ok, result = pcall(fn)\n");
        bundle.push_str("  _G.__luat_current_module = prev\n");
        bundle.push_str("  if not ok then error(result, 2) end\n");
        bundle.push_str("  if result == nil then result = true end\n");
        bundle.push_str("  package.loaded[key] = result\n");
        bundle.push_str("  return result\n");
        bundle.push_str("end\n\n");

        for (index, (name, lua_code)) in ordered_sources.iter().enumerate() {
            let line_offset = bundle.lines().count() + 1;

            // Store source mapping info
            source_map.add_module(name, name, line_offset, lua_code);

            // Add source reference comment
            bundle.push_str(&format!("\n-- MODULE: {} (line {})\n", name, line_offset));
            bundle.push_str(&format!("__source_map['{}'] = {{\n", name));
            bundle.push_str(&format!("  start_line = {},\n", line_offset));
            bundle.push_str(&format!("  source = [=[{}]=]\n", lua_code));
            bundle.push_str("}\n");

            // Add the module loader
            let escaped_name = escape_lua_string(name);
            bundle.push_str(&format!("__module_loaders['{}'] = function()\n", escaped_name));
            bundle.push_str("  local __prev = _G.__luat_current_module\n");
            bundle.push_str(&format!("  _G.__luat_current_module = '{}'\n", escaped_name));
            bundle.push_str("  local ok, result = pcall(function()\n");
            bundle.push_str(lua_code);
            bundle.push_str("\n  end)\n");
            bundle.push_str("  _G.__luat_current_module = __prev\n");
            bundle.push_str("  if not ok then\n");
            bundle.push_str(&format!("    __wrap_error('{}', result)\n", escaped_name));
            bundle.push_str("  end\n");
            bundle.push_str("  return result\n");
            bundle.push_str("end\n");

            let progress = 50 + ((index as f32 / ordered_sources.len() as f32) * 50.0) as u8;
            progress_callback(progress, total);
        }

        // Add enhanced module loader
        bundle.push_str(r#"
-- ENHANCED MODULE LOADER
local function __module_keys()
    local keys = {}
    for k, _ in pairs(__module_loaders) do
        table.insert(keys, k)
    end
    for k, _ in pairs(__modules) do
        table.insert(keys, k)
    end
    return keys
end

local function __require(name)
    local importer = _G.__luat_current_module or ""
    local kind, key = __resolve_module(name, importer)
    if kind == "module" then
        local module = __modules[key]
        if type(module) == "table" and module.parse_error then
            error(string.format("Module '%s' has parse error: %s", key, module.error), 2)
        end
        return module
    end
    if kind == "loader" then
        local module = __module_loaders[key]()
        if module == nil then module = true end
        __modules[key] = module
        package.loaded[key] = module
        if type(module) == "table" and module.parse_error then
            error(string.format("Module '%s' has parse error: %s", key, module.error), 2)
        end
        return module
    end
    if kind == "server" then
        local module = __load_server_module(key)
        if module ~= nil then return module end
    end
    local ok, result = pcall(__original_require, name)
    if ok then return result end
    error(string.format("Module '%s' not found in bundle. Available modules: %s. Require error: %s",
        name, table.concat(__module_keys(), ", "), result), 2)
end

-- Replace global require
_G.require = __require

-- Export for debugging
_G.__bundle_debug = {
    modules = __modules,
    source_map = __source_map,
    get_source = function(module_name, line_num)
        local map = __source_map[module_name]
        if not map then return nil end
        
        local lines = {}
        for line in map.source:gmatch("[^\n]+") do
            table.insert(lines, line)
        end
        
        if line_num and lines[line_num] then
            return lines[line_num]
        end
        return lines
    end,
    get_module_from_line = function(bundle_line)
        for name, info in pairs(__source_map) do
            if info.start_line <= bundle_line and 
               info.start_line + #(info.source:gmatch("[^\n]+)")) >= bundle_line then
                return name, bundle_line - info.start_line + 1
            end
        end
        return nil, nil
    end
}
"#);
        
        Ok((bundle, source_map))
    }

    /// Enables development mode for enhanced error messages.
    ///
    /// When enabled, errors include detailed stack traces and source context.
    /// Recommended during development but adds some runtime overhead.
    pub fn set_development_mode(&self, enabled: bool) -> Result<()> {
        self.lua.globals().set("__DEV_MODE", enabled)?;
        
        if enabled {
            // Install debug hooks
            self.lua.load(r#"
                -- Development mode error handler
                function __dev_error_handler(err)
                    local traceback = debug.traceback(err, 2)
                    local info = debug.getinfo(2)
                    
                    local enhanced_error = {
                        message = err,
                        traceback = traceback,
                        source = info.source,
                        line = info.currentline,
                        function_name = info.name or "anonymous"
                    }
                    
                    return enhanced_error
                end
                
                -- Development mode wrap all module renders with error handler
                local original_pcall = pcall
                _G.pcall = function(f, ...)
                    return original_pcall(function(...)
                        local ok, result = original_pcall(f, ...)
                        if not ok and __DEV_MODE then
                            result = __dev_error_handler(result)
                        end
                        return ok, result
                    end, ...)
                end
            "#).exec()?;
        }
        
        Ok(())
    }
    
    /// Loads a bundle and extracts source map information.
    ///
    /// Use this when loading bundles created with
    /// [`bundle_sources_with_sourcemap`](Self::bundle_sources_with_sourcemap).
    pub fn preload_bundle_code_with_sourcemap(&self, lua_code: &str) -> Result<BundleSourceMap> {
        // Extract source map from bundle if it exists
        let source_map = BundleSourceMap::new();
        
        // Execute the bundle code
        self.lua.load(lua_code).exec()?;
        
        // TODO: Extract sourcemap from Lua if needed
        
        Ok(source_map)
    }

    // ============================================================================
    // Helper Methods for Building Context
    // ============================================================================

    /// Creates an empty Lua table.
    pub fn create_table(&self) -> mlua::Result<mlua::Table> {
        self.lua.create_table()
    }

    /// Creates a Lua string value.
    pub fn create_string(&self, s: &str) -> mlua::Result<mlua::Value> {
        let lua_str = self.lua.create_string(s)?;
        Ok(mlua::Value::String(lua_str))
    }

    /// Creates a Lua boolean value.
    pub fn create_boolean(&self, b: bool) -> mlua::Result<mlua::Value> {
        Ok(mlua::Value::Boolean(b))
    }

    /// Creates a Lua table from a HashMap of string keys to values.
    pub fn create_table_from_hashmap<K: AsRef<str>>(&self, map: HashMap<K, mlua::Value>) -> mlua::Result<mlua::Table> {
        let table = self.lua.create_table()?;
        for (k, v) in map {
            table.set(k.as_ref(), v)?;
        }
        Ok(table)
    }

    /// Creates a Lua array (sequence table) from a Vec.
    pub fn create_table_from_vec<T: mlua::IntoLua>(&self, vec: Vec<T>) -> mlua::Result<mlua::Table> {
        let table = self.lua.create_table()?;
        for (i, item) in vec.into_iter().enumerate() {
            table.set(i + 1, item)?;
        }
        Ok(table)
    }

    /// Creates a Lua array and wraps it as a Value.
    pub fn create_table_to_value<T: mlua::IntoLua>(&self, vec: Vec<T>) -> mlua::Result<mlua::Value> {
        let table = self.create_table_from_vec(vec)?;
        Ok(mlua::Value::Table(table))
    }

    /// Inserts a string value into a context HashMap.
    pub fn insert_string(&self, context: &mut HashMap<String, mlua::Value>, key: &str, value: &str) -> mlua::Result<()> {
        let lua_str = self.lua.create_string(value)?;
        context.insert(key.to_string(), mlua::Value::String(lua_str));
        Ok(())
    }

    /// Inserts a value into a context HashMap.
    pub fn insert_value<K: Into<String>>(&self, context: &mut HashMap<String, mlua::Value>, key: K, value: mlua::Value) {
        context.insert(key.into(), value);
    }

    /// Inserts a table into a context HashMap.
    pub fn insert_table<K: Into<String>>(&self, context: &mut HashMap<String, mlua::Value>, key: K, table: mlua::Table) {
        context.insert(key.into(), mlua::Value::Table(table));
    }

    // ============================================================================
    // Cache Management
    // ============================================================================

    /// Checks if a module is in the cache.
    pub fn cache_contains(&self, module_path: &str) -> bool {
        self.cache.contains_key(module_path)
    }

    /// Clears all cached compiled modules.
    pub fn clear_cache(&self) -> Result<()> {
        self.cache.clear()
    }

    /// Sets up dev mode: require() always loads fresh from disk, no caching.
    ///
    /// This replaces Lua's require with a version that:
    /// 1. Always clears package.loaded for the module before loading
    /// 2. Never caches the result for subsequent calls
    ///
    /// Call this once after creating the engine for dev server usage.
    pub fn setup_dev_mode(&self) -> Result<()> {
        // Replace require with a non-caching version
        let lua_code = r#"
            local _original_require = require
            local _builtin = {
                json = true, _G = true, package = true,
                string = true, table = true, math = true,
                io = true, os = true, debug = true,
                coroutine = true, utf8 = true, kv = true
            }
            function require(name)
                -- Always clear from cache before loading (except builtins)
                if not _builtin[name] then
                    package.loaded[name] = nil
                end
                local result = _original_require(name)
                -- Clear again after loading so next require reloads fresh
                if not _builtin[name] then
                    package.loaded[name] = nil
                end
                return result
            end
        "#;
        self.lua.load(lua_code).exec()?;
        Ok(())
    }

    /// Clears Lua's internal module cache.
    ///
    /// Forces `require()` to reload modules from source on next call.
    /// Built-in modules (string, table, math, etc.) are preserved.
    pub fn clear_lua_module_cache(&self) -> Result<()> {
        // Get package.loaded table
        let globals = self.lua.globals();
        let package: Table = globals.get("package")?;
        let loaded: Table = package.get("loaded")?;

        // Collect keys to remove (can't modify while iterating)
        let mut keys_to_remove: Vec<String> = Vec::new();
        for (key, _) in loaded.pairs::<String, mlua::Value>().flatten() {
            // Keep built-in modules
            if key != "json" && key != "_G" && key != "package"
               && key != "string" && key != "table" && key != "math"
               && key != "io" && key != "os" && key != "debug"
               && key != "coroutine" && key != "utf8" {
                keys_to_remove.push(key);
            }
        }

        // Remove the modules
        for key in keys_to_remove {
            loaded.set(key, mlua::Value::Nil)?;
        }

        Ok(())
    }

    /// Converts a HashMap of tables to a HashMap of values.
    ///
    /// Utility method for working with mixed context data.
    pub fn convert_table_hashmap_to_value_hashmap(&self, map: &HashMap<String, mlua::Table>) -> HashMap<String, mlua::Value> {
        let mut result = HashMap::new();
        for (k, v) in map {
            result.insert(k.clone(), mlua::Value::Table(v.clone()));
        }
        result
    }

    /// Returns a reference to the underlying Lua runtime.
    ///
    /// Use for advanced operations not covered by the Engine API.
    /// Most users should not need to access this directly.
    pub fn lua(&self) -> &mlua::Lua {
        &self.lua
    }

    fn resolve_server_source(&self, path: &str) -> Result<String> {
        match self.resolver.resolve("", path) {
            Ok(resolved) => Ok(resolved.source),
            Err(err) => {
                if let Some(source) = self.server_source_from_bundle(path) {
                    Ok(source)
                } else {
                    Err(err)
                }
            }
        }
    }

    fn server_source_from_bundle(&self, path: &str) -> Option<String> {
        let globals = self.lua.globals();
        let server_sources: Table = globals.get("__server_sources").ok()?;
        server_sources.get::<String>(path).ok()
    }

    fn is_action_request(
        &self,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> bool {
        if route.page_server.is_none() {
            return false;
        }
        if !request.method.eq_ignore_ascii_case("GET") {
            return true;
        }
        request.action_name().is_some()
    }

    fn build_action_context(
        &self,
        request: &crate::request::LuatRequest,
        params: &std::collections::HashMap<String, String>,
    ) -> std::result::Result<crate::actions::ActionContext, String> {
        use crate::body::parse_action_body;

        let body = match request.body.as_ref() {
            Some(body) => parse_action_body(body, request.content_type())
                .map_err(|e| e.to_string())?,
            None => serde_json::Value::Null,
        };

        let query = request
            .query
            .iter()
            .filter(|(k, _)| !k.starts_with('/'))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let url = self.build_action_url(request);

        Ok(crate::actions::ActionContext::new(&request.method, &url)
            .with_params(params.clone())
            .with_query(query)
            .with_headers(request.headers.clone())
            .with_cookies(request.cookies.clone())
            .with_body(body)
            .with_action(request.action_name().map(|s| s.to_string())))
    }

    fn build_action_url(&self, request: &crate::request::LuatRequest) -> String {
        if request.query.is_empty() {
            return request.path.clone();
        }

        let mut pairs = Vec::new();
        for (key, value) in &request.query {
            if value.is_empty() {
                pairs.push(key.clone());
            } else {
                pairs.push(format!("{}={}", key, value));
            }
        }

        format!("{}?{}", request.path, pairs.join("&"))
    }

    fn action_template_candidates(ctx: &crate::actions::ActionContext) -> [String; 2] {
        let action_name = ctx.effective_action_name();
        let method_upper = ctx.method.to_uppercase();
        [
            format!("{}-{}", method_upper, action_name), // "POST-delete"
            action_name.to_string(),                      // "delete"
        ]
    }

    fn action_template_path(
        route: &crate::router::Route,
        candidate: &str,
    ) -> Option<String> {
        // Case-insensitive lookup in action_templates map
        // Only use templates discovered by router - no fallback
        let candidate_upper = candidate.to_uppercase();
        for (key, path) in &route.action_templates {
            if key.to_uppercase() == candidate_upper {
                return Some(path.clone());
            }
        }
        None
    }

    fn is_not_found_error(&self, err: &LuatError) -> bool {
        match err {
            LuatError::ResolutionError(_) | LuatError::ModuleNotFound(_) => true,
            LuatError::LuaError(lua_err) => {
                let msg = lua_err.to_string();
                // Match both bundle errors and Lua's standard "module not found" errors
                msg.contains("not found in bundle") || msg.contains("not found:")
            }
            _ => false,
        }
    }

    fn action_error_response(status: u16, message: impl Into<String>) -> crate::response::LuatResponse {
        crate::response::LuatResponse::json(
            status,
            serde_json::json!({ "error": message.into() }),
        )
    }

    fn action_response_to_luat(
        &self,
        response: crate::actions::ActionResponse,
        rendered_html: Option<String>,
    ) -> crate::response::LuatResponse {
        if let Some(html) = rendered_html {
            let mut headers = response.headers;
            headers.insert("x-luat-fragment".to_string(), "1".to_string());
            return crate::response::LuatResponse::html_with_headers(response.status, html, headers);
        }

        crate::response::LuatResponse::json_with_headers(response.status, response.data, response.headers)
    }

    /// Renders a template synchronously (reserved for future use).
    #[allow(dead_code)]
    fn render_template_sync(&self, module_path: &str, context: &Value) -> Result<String> {
        let module = self.compile_entry(module_path)?;
        self.render(&module, context)
    }

    fn render_action_template_sync(
        &self,
        route: &crate::router::Route,
        ctx: &crate::actions::ActionContext,
        response: &crate::actions::ActionResponse,
    ) -> Result<Option<String>> {
        // Find template in router-discovered action_templates (no fallback)
        for candidate in Self::action_template_candidates(ctx) {
            if let Some(template_path) = Self::action_template_path(route, &candidate) {
                // Template found - load and render it
                let context = self.to_value(&response.data)?;
                let module = self.compile_entry(&template_path)?;
                let html = self.render(&module, &context)?;
                return Ok(Some(html));
            }
        }
        // No template found - return None (caller will return JSON)
        Ok(None)
    }

    #[cfg(feature = "async-lua")]
    async fn render_template_async(&self, module_path: &str, context: &Value) -> Result<String> {
        match self.compile_entry(module_path) {
            Ok(module) => self.render(&module, context),
            Err(err) => {
                if self.is_not_found_error(&err) {
                    return self.render_from_bundle(module_path, context).await;
                }
                Err(err)
            }
        }
    }

    #[cfg(feature = "async-lua")]
    async fn render_action_template_async(
        &self,
        route: &crate::router::Route,
        ctx: &crate::actions::ActionContext,
        response: &crate::actions::ActionResponse,
    ) -> Result<Option<String>> {
        // Find template in router-discovered action_templates (no fallback)
        for candidate in Self::action_template_candidates(ctx) {
            if let Some(template_path) = Self::action_template_path(route, &candidate) {
                // Template found - load and render it
                // Use render_template_async which has fallback to bundle rendering
                let context = self.to_value(&response.data)?;
                let html = self.render_template_async(&template_path, &context).await?;
                return Ok(Some(html));
            }
        }
        // No template found - return None (caller will return JSON)
        Ok(None)
    }

    // ============================================================================
    // Request Handling (SvelteKit-style unified entry point)
    // ============================================================================

    /// Handles a request using a pre-matched route.
    ///
    /// This is the main entry point for request handling. It executes:
    /// 1. Layout load functions (from root to current route)
    /// 2. Page load function OR API handler
    /// 3. Template rendering (for page routes)
    ///
    /// All Lua execution uses the Engine's single Lua instance, ensuring
    /// all modules (json, KV, etc.) are available.
    ///
    /// # Arguments
    ///
    /// * `route` - The matched route from the Router
    /// * `request` - The incoming HTTP request
    ///
    /// # Returns
    ///
    /// A `LuatResponse` containing the response to send to the client.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use luat::{Engine, FileSystemResolver, Router, LuatRequest};
    ///
    /// let resolver = FileSystemResolver::new("./routes");
    /// let engine = Engine::with_memory_cache(resolver, 100)?;
    ///
    /// // Build router from route files
    /// let router = Router::from_paths(route_files.into_iter());
    ///
    /// // Handle request
    /// let request = LuatRequest::new("/blog/hello", "GET");
    /// if let Some(route) = router.match_url(&request.path) {
    ///     let response = engine.respond(&route, &request)?;
    /// }
    /// ```
    pub fn respond(
        &self,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::runtime::Runtime;

        let runtime = Runtime::new(&self.lua);

        // For API-only routes (+server.lua without +page.luat)
        if route.is_api_route() {
            return self.handle_api_route(&runtime, route, request);
        }

        if self.is_action_request(route, request) {
            return self.handle_action_request_sync(route, request);
        }

        // For page routes, run load functions and render
        self.handle_page_route(&runtime, route, request)
    }

    /// Async request handler that can fall back to bundle rendering.
    #[cfg(feature = "async-lua")]
    pub async fn respond_async(
        &self,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::runtime::Runtime;

        let runtime = Runtime::new(&self.lua);

        if route.is_api_route() {
            return self.handle_api_route(&runtime, route, request);
        }

        if self.is_action_request(route, request) {
            return self.handle_action_request_async(route, request).await;
        }

        self.handle_page_route_async(&runtime, route, request).await
    }

    fn handle_action_request_sync(
        &self,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::actions::ActionExecutor;

        let Some(ref server_path) = route.page_server else {
            return Ok(Self::action_error_response(405, "No server handler"));
        };

        let ctx = match self.build_action_context(request, &route.params) {
            Ok(ctx) => ctx,
            Err(message) => return Ok(Self::action_error_response(400, message)),
        };

        let source = match self.resolve_server_source(server_path) {
            Ok(source) => source,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Server source error: {}", err),
                ))
            }
        };

        let executor = ActionExecutor::new(&self.lua);
        let response = match executor.execute(&source, server_path, &ctx) {
            Ok(resp) => resp,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Action error: {}", err),
                ))
            }
        };

        let rendered = match self.render_action_template_sync(route, &ctx, &response) {
            Ok(html) => html,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Action template error: {}", err),
                ))
            }
        };

        Ok(self.action_response_to_luat(response, rendered))
    }

    #[cfg(feature = "async-lua")]
    async fn handle_action_request_async(
        &self,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::actions::ActionExecutor;

        let Some(ref server_path) = route.page_server else {
            return Ok(Self::action_error_response(405, "No server handler"));
        };

        let ctx = match self.build_action_context(request, &route.params) {
            Ok(ctx) => ctx,
            Err(message) => return Ok(Self::action_error_response(400, message)),
        };

        let source = match self.resolve_server_source(server_path) {
            Ok(source) => source,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Server source error: {}", err),
                ))
            }
        };

        let executor = ActionExecutor::new(&self.lua);
        let response = match executor.execute(&source, server_path, &ctx) {
            Ok(resp) => resp,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Action error: {}", err),
                ))
            }
        };

        let rendered = match self
            .render_action_template_async(route, &ctx, &response)
            .await
        {
            Ok(html) => html,
            Err(err) => {
                return Ok(Self::action_error_response(
                    500,
                    format!("Action template error: {}", err),
                ))
            }
        };

        Ok(self.action_response_to_luat(response, rendered))
    }

    /// Handles an API-only route (+server.lua).
    fn handle_api_route(
        &self,
        runtime: &crate::runtime::Runtime,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::response::LuatResponse;

        let api_path = route.api.as_ref().ok_or_else(|| {
            LuatError::InvalidTemplate("API route has no +server.lua".to_string())
        })?;

        // Load the API handler source
        let source = self.resolve_server_source(api_path)?;

        // Run the API handler
        let api_result = runtime
            .run_api(&source, api_path, request, &route.params)
            .map_err(LuatError::LuaError)?;

        // Check for redirect
        if let Some(location) = api_result.headers.get("Location") {
            return Ok(LuatResponse::redirect_with_status(
                api_result.status,
                location.clone(),
            ));
        }

        // Return JSON response
        Ok(LuatResponse::json_with_headers(
            api_result.status,
            api_result.body,
            api_result.headers,
        ))
    }

    /// Handles a page route (+page.luat with optional load functions).
    fn handle_page_route(
        &self,
        runtime: &crate::runtime::Runtime,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::response::LuatResponse;
        use serde_json::Value as JsonValue;

        // Initialize shared runtime for this request (enables setContext/getContext in templates)
        let request_runtime: Table = self.lua.create_table()?;
        let context_stack: Table = self.lua.create_sequence_from::<Table>(vec![])?;
        let page_context: Table = self.lua.create_table()?;  // Non-scoped page context for view_title etc.
        request_runtime.set("context_stack", context_stack)?;
        request_runtime.set("page_context", page_context)?;
        self.lua.set_named_registry_value("__luat_request_runtime", request_runtime.clone())?;

        let mut merged_props = serde_json::Map::new();

        // 1. Run layout server load functions (from root to current)
        for layout_server_path in &route.layout_servers {
            let load_result = self.run_load_file(runtime, layout_server_path, request, &route.params)?;

            // Check for redirect
            if let Some(redirect) = load_result.redirect {
                let status = load_result.status.unwrap_or(302);
                return Ok(LuatResponse::redirect_with_status(status, redirect));
            }

            // Merge props
            if let JsonValue::Object(props) = load_result.props {
                for (k, v) in props {
                    merged_props.insert(k, v);
                }
            }
        }

        // 2. Run page server load function if present
        if let Some(ref page_server_path) = route.page_server {
            let load_result = self.run_load_file(runtime, page_server_path, request, &route.params)?;

            // Check for redirect
            if let Some(redirect) = load_result.redirect {
                let status = load_result.status.unwrap_or(302);
                return Ok(LuatResponse::redirect_with_status(status, redirect));
            }

            // Merge props
            if let JsonValue::Object(props) = load_result.props {
                for (k, v) in props {
                    merged_props.insert(k, v);
                }
            }
        }

        // 3. Render the page template
        let page_path = route.page.as_ref().ok_or_else(|| {
            LuatError::InvalidTemplate("Page route has no +page.luat".to_string())
        })?;

        // Compile the page template
        let module = self.compile_entry(page_path)?;

        // Convert merged props to Lua value
        let context = self.to_value(JsonValue::Object(merged_props.clone()))?;

        // Render the page
        let mut body_html = self.render(&module, &context)?;

        // 4. Wrap in layouts (from innermost to outermost)
        for layout_path in route.layouts.iter().rev() {
            // Create layout props with children
            let mut layout_props = merged_props.clone();
            layout_props.insert("children".to_string(), JsonValue::String(body_html.clone()));

            let layout_context = self.to_value(JsonValue::Object(layout_props))?;

            // Compile and render the layout
            let layout_module = self.compile_entry(layout_path)?;
            body_html = self.render(&layout_module, &layout_context)?;
        }

        // Extract view_title from context_stack if set by any template
        let view_title = self.extract_view_title_from_context(&request_runtime)?;

        // Clean up request runtime from registry
        let _ = self.lua.unset_named_registry_value("__luat_request_runtime");

        // Build response with optional view_title header
        let mut headers = std::collections::HashMap::new();
        if let Some(title) = view_title {
            headers.insert("x-luat-title".to_string(), title);
        }

        Ok(LuatResponse::Html {
            status: 200,
            headers,
            body: body_html,
        })
    }

    /// Extracts view_title from page_context (preferred) or context_stack (fallback).
    fn extract_view_title_from_context(&self, runtime: &Table) -> Result<Option<String>> {
        // First check page_context (non-scoped, takes precedence)
        if let Ok(page_ctx) = runtime.get::<Table>("page_context") {
            if let Ok(mlua::Value::String(s)) = page_ctx.get::<mlua::Value>("view_title") {
                if let Ok(title) = s.to_str() {
                    return Ok(Some(title.to_string()));
                }
            }
        }

        // Fall back to context_stack (for backwards compatibility)
        let stack: Table = match runtime.get("context_stack") {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        let len = stack.len().unwrap_or(0);
        // Search from top to bottom of stack (most recent context first)
        for i in (1..=len).rev() {
            if let Ok(scope) = stack.get::<Table>(i) {
                if let Ok(mlua::Value::String(s)) = scope.get::<mlua::Value>("view_title") {
                    if let Ok(title) = s.to_str() {
                        return Ok(Some(title.to_string()));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Handles a page route with async rendering (bundle-aware).
    #[cfg(feature = "async-lua")]
    async fn handle_page_route_async(
        &self,
        runtime: &crate::runtime::Runtime<'_>,
        route: &crate::router::Route,
        request: &crate::request::LuatRequest,
    ) -> Result<crate::response::LuatResponse> {
        use crate::response::LuatResponse;
        use serde_json::Value as JsonValue;

        // Initialize shared runtime for this request (enables setContext/getContext in templates)
        let request_runtime: Table = self.lua.create_table()?;
        let context_stack: Table = self.lua.create_sequence_from::<Table>(vec![])?;
        let page_context: Table = self.lua.create_table()?;  // Non-scoped page context for view_title etc.
        request_runtime.set("context_stack", context_stack)?;
        request_runtime.set("page_context", page_context)?;
        self.lua.set_named_registry_value("__luat_request_runtime", request_runtime.clone())?;

        let mut merged_props = serde_json::Map::new();

        for layout_server_path in &route.layout_servers {
            let load_result = self.run_load_file(runtime, layout_server_path, request, &route.params)?;

            if let Some(redirect) = load_result.redirect {
                let status = load_result.status.unwrap_or(302);
                return Ok(LuatResponse::redirect_with_status(status, redirect));
            }

            if let JsonValue::Object(props) = load_result.props {
                for (k, v) in props {
                    merged_props.insert(k, v);
                }
            }
        }

        if let Some(ref page_server_path) = route.page_server {
            let load_result = self.run_load_file(runtime, page_server_path, request, &route.params)?;

            if let Some(redirect) = load_result.redirect {
                let status = load_result.status.unwrap_or(302);
                return Ok(LuatResponse::redirect_with_status(status, redirect));
            }

            if let JsonValue::Object(props) = load_result.props {
                for (k, v) in props {
                    merged_props.insert(k, v);
                }
            }
        }

        let page_path = route.page.as_ref().ok_or_else(|| {
            LuatError::InvalidTemplate("Page route has no +page.luat".to_string())
        })?;

        let context = self.to_value(JsonValue::Object(merged_props.clone()))?;
        let mut body_html = self.render_template_async(page_path, &context).await?;

        for layout_path in route.layouts.iter().rev() {
            let mut layout_props = merged_props.clone();
            layout_props.insert("children".to_string(), JsonValue::String(body_html.clone()));

            let layout_context = self.to_value(JsonValue::Object(layout_props))?;
            body_html = self.render_template_async(layout_path, &layout_context).await?;
        }

        // Extract view_title from context_stack if set by any template
        let view_title = self.extract_view_title_from_context(&request_runtime)?;

        // Clean up request runtime from registry
        let _ = self.lua.unset_named_registry_value("__luat_request_runtime");

        // Build response with optional view_title header
        let mut headers = std::collections::HashMap::new();
        if let Some(title) = view_title {
            headers.insert("x-luat-title".to_string(), title);
        }

        Ok(LuatResponse::Html {
            status: 200,
            headers,
            body: body_html,
        })
    }

    /// Runs a load file and returns the result.
    fn run_load_file(
        &self,
        runtime: &crate::runtime::Runtime,
        path: &str,
        request: &crate::request::LuatRequest,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<crate::runtime::LoadResult> {
        let source = self.resolve_server_source(path)?;
        runtime
            .run_load(&source, path, request, params)
            .map_err(LuatError::LuaError)
    }
}
