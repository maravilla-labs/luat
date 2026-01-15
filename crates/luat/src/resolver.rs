// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Template resource resolution.
//!
//! This module provides the [`ResourceResolver`] trait and implementations
//! for locating and loading template files.
//!
//! # Resolver Implementations
//!
//! - [`FileSystemResolver`]: Loads templates from the filesystem (native builds)
//! - [`MemoryResourceResolver`]: Loads templates from in-memory storage (testing/WASM)
//!
//! # Resolution Algorithm
//!
//! The filesystem resolver supports multiple resolution strategies:
//!
//! 1. **Absolute paths** (`/components/Button`): Relative to root directory
//! 2. **Explicit relative** (`./Button`, `../shared/Card`): Relative to importer
//! 3. **Implicit relative** (`Button`): Tries current directory first, then root
//!
//! # Custom Resolvers
//!
//! Implement [`ResourceResolver`] for custom loading strategies (network, database, etc.).

use std::path::{Path, PathBuf};
use crate::error::{Result, LuatError};

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
use std::fs;

/// Converts a Path to a normalized string with forward slashes.
/// On Windows, uses path components to rebuild with `/` separators.
#[inline]
pub fn path_to_string<P: AsRef<Path>>(path: P) -> String {
    #[cfg(windows)]
    {
        use std::path::Component;
        let path = path.as_ref();
        let mut result = String::new();
        for (i, component) in path.components().enumerate() {
            if i > 0 {
                result.push('/');
            }
            match component {
                Component::Prefix(p) => result.push_str(&p.as_os_str().to_string_lossy()),
                Component::RootDir => result.push('/'),
                Component::CurDir => result.push('.'),
                Component::ParentDir => result.push_str(".."),
                Component::Normal(s) => result.push_str(&s.to_string_lossy()),
            }
        }
        result
    }
    #[cfg(not(windows))]
    {
        path.as_ref().to_string_lossy().to_string()
    }
}

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use crate::Engine;

/// A resolved template resource with its path and source code.
#[derive(Debug, Clone)]
pub struct ResolvedResource {
    /// The absolute, canonicalized path of the template.
    pub path: String,
    /// The template source code.
    pub source: String,
}

/// Trait for resolving and loading template resources.
///
/// Implement this trait to create custom template loading strategies.
/// On native builds, implementations must be thread-safe (`Send + Sync`).
#[cfg(not(target_arch = "wasm32"))]
pub trait ResourceResolver: Send + Sync + 'static {
    /// Resolves a module path and returns its source code.
    ///
    /// # Arguments
    ///
    /// * `importer_path` - Path of the template doing the import (empty for entry)
    /// * `module_name` - The module path to resolve
    fn resolve(&self, importer_path: &str, module_name: &str) -> Result<ResolvedResource>;

    /// Returns the resolved path without loading the source.
    fn get_resolved_path(&self, importer_path: &str, module_name: &str) -> Result<String>;

    /// Creates a boxed clone (for use in closures).
    fn clone_box(&self) -> Box<dyn ResourceResolver>;
}

/// Trait for resolving and loading template resources (WASM variant).
#[cfg(target_arch = "wasm32")]
pub trait ResourceResolver: 'static {
    /// Resolves a module path and returns its source code.
    fn resolve(&self, importer_path: &str, module_name: &str) -> Result<ResolvedResource>;
    /// Returns the resolved path without loading the source.
    fn get_resolved_path(&self, importer_path: &str, module_name: &str) -> Result<String>;
    /// Creates a boxed clone (for use in closures).
    fn clone_box(&self) -> Box<dyn ResourceResolver>;
}

impl Clone for Box<dyn ResourceResolver> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Filesystem-based resource resolver.
///
/// Loads templates from the filesystem relative to a root directory.
/// Only available on native builds with the `filesystem` feature.
///
/// # Examples
///
/// ```rust,ignore
/// use luat::FileSystemResolver;
///
/// let resolver = FileSystemResolver::new("./templates");
/// let resource = resolver.resolve("", "pages/index.luat")?;
///
/// // With $lib alias support:
/// let resolver = FileSystemResolver::new("./src/routes")
///     .with_lib_dir("./src/lib");
/// // Now you can use: require("$lib/components/Button.luat")
/// ```
#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
#[derive(Debug, Clone)]
pub struct FileSystemResolver {
    /// The root directory for template resolution.
    pub root_dir: String,
    /// The lib directory for $lib alias resolution.
    pub lib_dir: Option<String>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
impl FileSystemResolver {
    /// Creates a new filesystem resolver with the given root directory.
    pub fn new<P: AsRef<Path>>(root_dir: P) -> Self {
        Self {
            root_dir: path_to_string(root_dir.as_ref()),
            lib_dir: None,
        }
    }

    /// Sets the lib directory for `$lib` alias resolution.
    ///
    /// When set, module names starting with `$lib/` will be resolved
    /// relative to this directory instead of the root directory.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let resolver = FileSystemResolver::new("./src/routes")
    ///     .with_lib_dir("./src/lib");
    /// // require("$lib/components/Button") resolves to ./src/lib/components/Button.luat
    /// ```
    pub fn with_lib_dir<P: AsRef<Path>>(mut self, lib_dir: P) -> Self {
        self.lib_dir = Some(path_to_string(lib_dir.as_ref()));
        self
    }

    /// Expands path aliases like `$lib/...` to their actual paths.
    /// Returns (expanded_path, is_alias_absolute).
    fn expand_aliases(&self, module_name: &str) -> (String, bool) {
        if let Some(ref lib_dir) = self.lib_dir {
            if let Some(suffix) = module_name.strip_prefix("$lib/") {
                // Replace $lib/ with the lib directory path
                return (format!("{}/{}", lib_dir, suffix), true);
            }
            if let Some(suffix) = module_name.strip_prefix("lib/") {
                // Allow lib/ alias when lib_dir is configured
                return (format!("{}/{}", lib_dir, suffix), true);
            }
        }
        (module_name.to_string(), false)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
impl FileSystemResolver {
    // Helper method to try reading a file
    #[allow(dead_code)]
    fn try_read_file(&self, path: &Path) -> Result<ResolvedResource> {
        if !path.exists() || !path.is_file() {
            return Err(LuatError::ResolutionError(
                format!("File not found: {}", path.display())
            ));
        }
        
        let source = std::fs::read_to_string(path)
            .map_err(|e| LuatError::ResolutionError(
                format!("Cannot read '{}': {}", path.display(), e)
            ))?;
            
        Ok(ResolvedResource {
            path: path_to_string(&path),
            source,
        })
    }
    
    fn resolve_internal(&self, importer_path: &str, module_name: &str) -> Result<(PathBuf, String)> {
        let (expanded_module_name, alias_absolute) = self.expand_aliases(module_name);
        let module_name = expanded_module_name.as_str();

        let root_dir_path = Path::new(&self.root_dir);
        let mut base_path = root_dir_path.to_path_buf();
        
        // If we have an importer path, adjust the base path
        if !importer_path.is_empty() {
            let importer_as_path = Path::new(importer_path);
            if importer_as_path.is_absolute() {
                // If absolute, use its parent directory directly
                if importer_as_path.is_file() || importer_as_path.extension().is_some() {
                    base_path = importer_as_path.parent().unwrap_or(root_dir_path).to_path_buf();
                } else {
                    base_path = importer_as_path.to_path_buf();
                }
            } else {
                // If relative to the root dir, adjust accordingly
                let full_importer_path = root_dir_path.join(importer_as_path);
                if full_importer_path.is_file() || full_importer_path.extension().is_some() {
                    base_path = full_importer_path.parent().unwrap_or(root_dir_path).to_path_buf();
                } else {
                    base_path = full_importer_path;
                }
            }
        }

        let module_as_path = Path::new(module_name);
        let mut full_path = if module_as_path.is_absolute() {
            // Absolute path - use directly for alias paths, otherwise treat as root-relative
            if alias_absolute {
                module_as_path.to_path_buf()
            } else {
                root_dir_path.join(module_as_path.strip_prefix("/").unwrap_or(module_as_path))
            }
        } else if module_name.starts_with("./") || module_name.starts_with("../") {
            // Explicit relative path - resolve from the base path
            base_path.join(module_as_path)
        } else {
            // Implicit relative path - try base path first, then root path
            let base_resolved = base_path.join(module_as_path);
            if base_resolved.exists() {
                base_resolved
            } else {
                root_dir_path.join(module_as_path)
            }
        };

        // If a relative path resolves under routes/lib, remap to lib_dir when configured
        if !full_path.exists() {
            if let Some(ref lib_dir) = self.lib_dir {
                let lib_alias_root = root_dir_path.join("lib");
                let normalized = normalize_path_buf(&full_path);
                if normalized.starts_with(&lib_alias_root) {
                    if let Ok(rel) = normalized.strip_prefix(&lib_alias_root) {
                        full_path = Path::new(lib_dir).join(rel);
                    }
                }
            }
        }

        // Try to resolve with or without extensions
        let extensions = ["luat", "lua"];
        let mut resolved_path_option = None;

        // Check if the path exists with its current extension
        if full_path.extension().is_some() && full_path.exists() {
            resolved_path_option = Some(full_path.clone());
        } else {
            // Try with our supported extensions
            for ext in &extensions {
                let path_with_ext = full_path.with_extension(ext);
                if path_with_ext.exists() {
                    resolved_path_option = Some(path_with_ext);
                    break;
                }
            }
            
            // If still not found, check if it's a component name only
            if resolved_path_option.is_none() {
                // Try component name only from both root and base paths
                let basename = module_as_path.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(module_name);
                
                for ext in &extensions {
                    // Try from base path first
                    let basename_path = base_path.join(basename).with_extension(ext);
                    if basename_path.exists() {
                        resolved_path_option = Some(basename_path);
                        break;
                    }
                    
                    // Try from root path if different from base
                    if base_path != root_dir_path {
                        let root_basename_path = root_dir_path.join(basename).with_extension(ext);
                        if root_basename_path.exists() {
                            resolved_path_option = Some(root_basename_path);
                            break;
                        }
                    }
                }
            }
        }
        tracing::debug!("Resolved path for '{}' from '{}': {:?}", module_name, importer_path, resolved_path_option);
        if resolved_path_option.is_none() {
            // If not found in routes/templates, try lib_dir as a fallback for bare imports
            if let Some(ref lib_dir) = self.lib_dir {
                let lib_root = Path::new(lib_dir);
                let module_as_path = Path::new(module_name);
                let lib_full_path = if module_as_path.is_absolute() {
                    module_as_path.to_path_buf()
                } else {
                    lib_root.join(module_as_path)
                };

                if lib_full_path.extension().is_some() && lib_full_path.exists() {
                    resolved_path_option = Some(lib_full_path.clone());
                } else {
                    for ext in &extensions {
                        let path_with_ext = lib_full_path.with_extension(ext);
                        if path_with_ext.exists() {
                            resolved_path_option = Some(path_with_ext);
                            break;
                        }
                    }
                }
            }
        }

        match resolved_path_option {
            Some(resolved_path) => {
                let canonical_path = fs::canonicalize(&resolved_path).map_err(|e| LuatError::ResolutionError(
                    format!("Failed to canonicalize path '{}': {}", resolved_path.to_string_lossy(), e)
                ))?;

                // Security: Verify the resolved path is within root_dir OR lib_dir
                // This prevents symlink attacks and path traversal while allowing lib imports
                let canonical_root = fs::canonicalize(&self.root_dir).map_err(|e| LuatError::ResolutionError(
                    format!("Failed to canonicalize root '{}': {}", self.root_dir, e)
                ))?;

                let in_root = canonical_path.starts_with(&canonical_root);
                let in_lib = if let Some(ref lib_dir) = self.lib_dir {
                    if let Ok(canonical_lib) = fs::canonicalize(lib_dir) {
                        canonical_path.starts_with(&canonical_lib)
                    } else {
                        false
                    }
                } else {
                    false
                };

                if !in_root && !in_lib {
                    return Err(LuatError::ResolutionError(
                        format!("Security: Path '{}' escapes allowed directories", module_name)
                    ));
                }

                Ok((canonical_path.clone(), path_to_string(&canonical_path)))
            }
            None => Err(LuatError::ResolutionError(
                format!("Module '{}' not found from importer '{}'", module_name, importer_path)
            )),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
fn normalize_path_buf(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(seg) => normalized.push(seg),
        }
    }
    normalized
}

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
impl ResourceResolver for FileSystemResolver {
    fn resolve(&self, importer_path: &str, module_name: &str) -> Result<ResolvedResource> {
        let (resolved_path_buf, path_str) = self.resolve_internal(importer_path, module_name)?;
        let source = fs::read_to_string(&resolved_path_buf).map_err(LuatError::IoError)?;
        Ok(ResolvedResource {
            path: path_str,
            source,
        })
    }

    fn get_resolved_path(&self, importer_path: &str, module_name: &str) -> Result<String> {
        let (_, path_str) = self.resolve_internal(importer_path, module_name)?;
        Ok(path_str)
    }

    fn clone_box(&self) -> Box<dyn ResourceResolver> {
        Box::new(self.clone())
    }
}

/// Simple in-memory resource resolver for testing.
///
/// Stores templates in a HashMap for quick access without filesystem.
/// Primarily used for unit tests and embedding templates in binaries.
#[derive(Debug, Clone)]
pub struct MemoryResourceResolver {
    /// Map of path -> template source code.
    pub resources: std::collections::HashMap<String, String>,
}

impl Default for MemoryResourceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryResourceResolver {
    /// Creates an empty memory resolver.
    pub fn new() -> Self {
        Self {
            resources: std::collections::HashMap::new(),
        }
    }

    /// Adds a template to the resolver.
    ///
    /// # Arguments
    ///
    /// * `path` - Virtual path for the template
    /// * `source` - Template source code
    pub fn add_resource(&mut self, path: &str, source: &str) {
        self.resources.insert(path.to_string(), source.to_string());
    }
}

impl MemoryResourceResolver {
    fn resolve_internal(&self, importer_path: &str, module_name: &str) -> Result<(String, String)> { // Returns (canonical_key, content_key)
        let mut base_path = PathBuf::new();
        if !importer_path.is_empty() {
            let importer_as_path = Path::new(importer_path);
            // In memory resolver, importer_path is a key, not necessarily a file system path parent.
            // We assume importer_path is a "directory" key or a full "file" key.
            // If it's a "file" key, take its parent.
            if self.resources.contains_key(importer_path) && importer_path.contains('/') { // Check if it looks like a file path
                 base_path = importer_as_path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
            } else {
                 base_path = importer_as_path.to_path_buf(); // Assume it's a directory key or empty
            }
        }
        
        let module_as_path = Path::new(module_name);
        let potential_path_buf = if module_as_path.is_absolute() {
            // For memory resolver, absolute paths start from "root" (empty base_path)
            // Or, if they have a scheme like "memory:/", handle that. For now, assume they are like /foo/bar.luat
             module_as_path.strip_prefix("/").unwrap_or(module_as_path).to_path_buf()
        } else {
            base_path.join(module_as_path)
        };

        let extensions = ["luat", "lua"];
        let mut found_key = None;

        let path_str = path_to_string(&potential_path_buf);
        if path_str.ends_with(".luat") || path_str.ends_with(".lua") {
            if self.resources.contains_key(&path_str) {
                found_key = Some(path_str);
            }
        } else {
            for ext in &extensions {
                let key_with_ext = format!("{}.{}", path_str, ext);
                if self.resources.contains_key(&key_with_ext) {
                    found_key = Some(key_with_ext);
                    break;
                }
            }
        }

        match found_key {
            Some(key) => Ok((key.clone(), key)), // For memory, canonical key is the key itself
            None => Err(LuatError::ResolutionError(
                format!("Module '{}' not found in memory resources from importer '{}'", module_name, importer_path)
            )),
        }
    }
}


impl ResourceResolver for MemoryResourceResolver {
    fn resolve(&self, importer_path: &str, module_name: &str) -> Result<ResolvedResource> {
        let (resolved_key, _) = self.resolve_internal(importer_path, module_name)?;
        
        let source = self.resources.get(&resolved_key)
            .ok_or_else(|| LuatError::ResolutionError(
                format!("Module somehow disappeared from memory resources after key resolution for: {}", resolved_key)
            ))?
            .clone();
            
        Ok(ResolvedResource {
            path: resolved_key, // path is the key itself
            source,
        })
    }

    fn get_resolved_path(&self, importer_path: &str, module_name: &str) -> Result<String> {
        let (resolved_key, _) = self.resolve_internal(importer_path, module_name)?;
        Ok(resolved_key)
    }

    fn clone_box(&self) -> Box<dyn ResourceResolver> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "filesystem")]
    use std::fs;
    #[cfg(feature = "filesystem")]
    use tempfile::TempDir;

    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver() {
        let temp_dir = TempDir::new().unwrap();
        let template_content = r#"<div>Hello World</div>"#;
        
        let template_path = temp_dir.path().join("test.luat");
        fs::write(&template_path, template_content).unwrap();

        let resolver = FileSystemResolver::new(temp_dir.path());
        let resolved = resolver.resolve("", "test").unwrap();
        
        assert_eq!(resolved.source, template_content);
        assert!(resolved.path.ends_with("test.luat"));
    }
    
    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver_with_relative_paths() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create subdirectory structure
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        
        // Create template files
        let root_template = r#"<div>Root template</div>"#;
        let subdir_template = r#"<div>Subdir template</div>"#;
        
        let root_path = temp_dir.path().join("root.luat");
        let subdir_path = subdir.join("child.luat");
        
        fs::write(&root_path, root_template).unwrap();
        fs::write(&subdir_path, subdir_template).unwrap();
        
        let resolver = FileSystemResolver::new(temp_dir.path());
        
        // Test relative path from subdirectory to root using ../
        let subdir_importing = subdir_path.to_string_lossy().to_string();
        let resolved_parent = resolver.resolve(&subdir_importing, "../root").unwrap();
        assert_eq!(resolved_parent.source, root_template);
        
        // Test importing a file in the same directory
        let resolved_sibling = resolver.resolve(&root_path.to_string_lossy(), "./root").unwrap();
        assert_eq!(resolved_sibling.source, root_template);
        
        // Test absolute path
        let abs_path = root_path.canonicalize().unwrap();
        let resolved_abs = resolver.resolve("", abs_path.to_string_lossy().as_ref()).unwrap();
        assert_eq!(resolved_abs.source, root_template);
    }
    
    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver_with_subdirectory_imports() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create subdirectory structure
        let ui_dir = temp_dir.path().join("ui");
        fs::create_dir_all(&ui_dir).unwrap();
        
        // Create template files
        let main_template = r#"<script>local Header = require("./ui/Header")</script><div>Main</div>"#;
        let header_template = r#"<header>Header Component</header>"#;
        
        let main_path = temp_dir.path().join("main.luat");
        let header_path = ui_dir.join("Header.luat");
        
        fs::write(&main_path, main_template).unwrap();
        fs::write(&header_path, header_template).unwrap();
        
        let resolver = FileSystemResolver::new(temp_dir.path());
        
        // Test resolving a file from a subdirectory
        let resolved_header = resolver.resolve(&main_path.to_string_lossy(), "./ui/Header").unwrap();
        assert_eq!(resolved_header.source, header_template);
        
        // Test resolving when using empty importer (root-based)
        let resolved_from_root = resolver.resolve("", "ui/Header").unwrap();
        assert_eq!(resolved_from_root.source, header_template);
    }
    
    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver_with_extensions() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create files with different extensions
        let luat_content = r#"<div>LUAT Template</div>"#;
        let lua_content = r#"return { render = function() return "Lua Module" end }"#;
        
        fs::write(temp_dir.path().join("module.luat"), luat_content).unwrap();
        fs::write(temp_dir.path().join("script.lua"), lua_content).unwrap();
        
        let resolver = FileSystemResolver::new(temp_dir.path());
        
        // Test resolving with no extension (should find .luat)
        let resolved_luat = resolver.resolve("", "module").unwrap();
        assert_eq!(resolved_luat.source, luat_content);
        
        // Test resolving with no extension (should find .lua)
        let resolved_lua = resolver.resolve("", "script").unwrap();
        assert_eq!(resolved_lua.source, lua_content);
        
        // Test resolving with explicit extension
        let resolved_explicit = resolver.resolve("", "module.luat").unwrap();
        assert_eq!(resolved_explicit.source, luat_content);
    }
    
    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver_directory_index() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create directory with index files
        let component_dir = temp_dir.path().join("component");
        fs::create_dir_all(&component_dir).unwrap();
        
        let index_content = r#"<div>Default Component</div>"#;
        fs::write(component_dir.join("index.luat"), index_content).unwrap();
        
        // Create an app.luat file that requires the component index
        let app_content = r#"<script>
        local Component = require("./component/index.luat");</script>
        
        <Component />"#;
        fs::write(temp_dir.path().join("app.luat"), app_content).unwrap();
        
        let resolver = FileSystemResolver::new(temp_dir.path());
        
        // // Test resolving directory (should find index.luat)
        // let resolved_index = resolver.resolve("", "component").unwrap();
        // assert_eq!(resolved_index.source, index_content);
        
        // // Test resolving directory with trailing slash
        // let resolved_index_slash = resolver.resolve("", "component/").unwrap();
        // assert_eq!(resolved_index_slash.source, index_content);
        
        // Test with engine to verify full workflow
        let engine = Engine::with_memory_cache(resolver.clone(), 10).unwrap();
        let app_path = temp_dir.path().join("app.luat").to_string_lossy().to_string();
        let module = engine.compile_entry(&app_path).unwrap();
        let context = engine.to_value(HashMap::<String, String>::new()).unwrap();
        let rendered = engine.render(&module, &context).unwrap();
        assert!(rendered.contains("Default Component"), "Should render component index content");
    }
    
    #[cfg(feature = "filesystem")]
    #[test]
    fn test_filesystem_resolver_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let resolver = FileSystemResolver::new(temp_dir.path());
        
        // Create a valid file for relative path testing
        let valid_content = "<div>Valid file</div>";
        fs::write(temp_dir.path().join("valid.luat"), valid_content).unwrap();
        
        // Test file that doesn't exist (absolute path)
        let result = resolver.resolve("", "nonexistent");
        assert!(result.is_err());
        assert!(matches!(result, Err(LuatError::ResolutionError(_))));
        
        // Test file that doesn't exist (relative path from a valid file)
        let valid_path = temp_dir.path().join("valid.luat").to_string_lossy().to_string();
        let result = resolver.resolve(&valid_path, "./nonexistent");
        assert!(result.is_err());
        assert!(matches!(result, Err(LuatError::ResolutionError(_))));
        
        // Test with engine to verify error handling
        let engine = Engine::with_memory_cache(resolver.clone(), 10).unwrap();
        let result = engine.compile_entry("nonexistent.luat");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_memory_resolver() {
        let mut resolver = MemoryResourceResolver::new();
        let template_content = r#"<div>Hello Memory</div>"#;
        
        // Add resources to memory
        resolver.add_resource("test.luat", template_content);
        resolver.add_resource("app/main.luat", r#"<script>local Test = require("test");</script><Test/>"#);
        resolver.add_resource("components/Button.luat", "<button>Click me</button>");
        resolver.add_resource("app/ui/Card.luat", r#"<script>local Button = require("../../components/Button");</script><div class="card"><Button/></div>"#);
        
        // Resolve without importer (root level)
        let resolved = resolver.resolve("", "test").unwrap();
        println!("Resolved path: {}", resolved.path);
        assert_eq!(resolved.source, template_content);
        assert_eq!(resolved.path, "test.luat");
        
        // Resolve with importer (simulating a file in the same "directory")
        // let resolved_with_importer = resolver.resolve("app/ui/card", "Card.laut").unwrap();
        // assert_eq!(resolved_with_importer.source, template_content);
        // assert_eq!(resolved_with_importer.path, "Card");
        // TODO all this tests are wrong and need to be fixed
        // Test relative path resolution
        // let card = resolver.resolve("", "app/ui/Card").unwrap();
        // assert!(card.source.contains("<div class=\"card\">"));
        
        // let button_from_card = resolver.resolve("app/ui/", "../../components/Button").unwrap();
        // assert_eq!(button_from_card.source, "<button>Click me</button>");
        
        // Test with engine to verify full workflow
        // let engine = Engine::with_memory_cache(resolver.clone(), 10).unwrap();
        // let main_module = engine.compile_entry("app/main.luat").unwrap();
        // let rendered = engine.render(&main_module, &HashMap::new()).unwrap();
        // assert!(rendered.contains("Hello Memory"), "Should render test content from main");
        
        // let card_module = engine.compile_entry("app/ui/Card.luat").unwrap();
        // let card_rendered = engine.render(&card_module, &HashMap::new()).unwrap();
        // assert!(card_rendered.contains("Click me"), "Card should include Button content");
    }
    
    #[test]
    fn test_memory_resolver_not_found() {
        let resolver = MemoryResourceResolver::new();
        
        // Test resolving a non-existent module
        let result = resolver.resolve("", "nonexistent");
        assert!(result.is_err());
        
        match result {
            Err(LuatError::ResolutionError(message)) => {
                assert!(message.contains("Module 'nonexistent' not found"));
            },
            _ => panic!("Expected ResolutionError"),
        }
    }
    
    #[test]
    fn test_memory_resolver_with_extensions() {
        let mut resolver = MemoryResourceResolver::new();
        
        // Add resources with different extensions
        resolver.add_resource("module.luat", r#"<div>LUAT Memory Module</div>"#);
        resolver.add_resource("script.lua", r#"return { render = function() return "Lua Memory Module" end }"#);
        
        // Resolve without extension (should find .luat)
        let resolved_luat = resolver.resolve("", "module").unwrap();
        assert_eq!(resolved_luat.source, r#"<div>LUAT Memory Module</div>"#);
        
        // Resolve without extension (should find .lua)
        let resolved_lua = resolver.resolve("", "script").unwrap();
        assert_eq!(resolved_lua.source, r#"return { render = function() return "Lua Memory Module" end }"#);
        
        // Resolve with explicit extension
        let resolved_explicit = resolver.resolve("", "module.luat").unwrap();
        assert_eq!(resolved_explicit.source, r#"<div>LUAT Memory Module</div>"#);
    }
}
