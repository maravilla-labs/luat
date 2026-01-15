// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::resolver::{ResourceResolver, ResolvedResource};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::error::{Result, LuatError};

// Conditional imports for thread primitives
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, Arc};

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

/// Memory-based resource resolver that stores templates in memory
#[derive(Clone)]
pub struct MemoryResourceResolver {
    #[cfg(not(target_arch = "wasm32"))]
    templates: Arc<Mutex<HashMap<String, String>>>,
    #[cfg(target_arch = "wasm32")]
    templates: Rc<RefCell<HashMap<String, String>>>,
}

impl Default for MemoryResourceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryResourceResolver {
    /// Create a new memory resource resolver
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                templates: Arc::new(Mutex::new(HashMap::new())),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                templates: Rc::new(RefCell::new(HashMap::new())),
            }
        }
    }

    /// Helper to access templates with mutable reference (handles WASM/Native differences)
    fn with_templates_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<String, String>) -> R,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            f(&mut self.templates.lock().unwrap())
        }
        #[cfg(target_arch = "wasm32")]
        {
            f(&mut self.templates.borrow_mut())
        }
    }

    /// Add a template to the memory resolver
    pub fn add_template(&self, path: &str, content: String) {
        self.with_templates_mut(|templates| {
            templates.insert(path.to_string(), content);
        });
    }

    /// Add a resource to the memory resolver (for compatibility with ResourceResolver)
    pub fn add_resource(&self, path: &str, content: &str) {
        self.add_template(path, content.to_string());
    }

    /// Remove a template from the memory resolver
    pub fn remove_template(&self, path: &str) {
        self.with_templates_mut(|templates| {
            templates.remove(path);
        });
    }

    /// Clear all templates
    pub fn clear(&self) {
        self.with_templates_mut(HashMap::clear);
    }

    /// Helper function to resolve internal paths
    #[allow(dead_code)]
    fn resolve_internal(&self, importer_path: &str, module_name: &str) -> Result<(String, String)> {
        // Handle absolute paths
        if module_name.starts_with('/') {
            // For absolute paths, strip leading slash and treat as root-relative
            let clean_name = module_name.strip_prefix('/').unwrap_or(module_name);
            return self.try_resolve_with_variants(clean_name);
        }
        
        // No importer context or absolute module name, resolve directly
        if importer_path.is_empty() {
            return self.try_resolve_with_variants(module_name);
        }
        
        // We have an importer path, calculate the new path relative to it
        let importer = Path::new(importer_path);
        let base_dir = if importer.is_file() || importer.extension().is_some() {
            // If the importer path looks like a file, use its parent directory
            if let Some(parent) = importer.parent() {
                parent.to_path_buf()
            } else {
                PathBuf::from("")
            }
        } else {
            // Otherwise, assume importer_path is already a directory
            PathBuf::from(importer_path)
        };
        
        // Now normalize the module_name relative to base_dir
        let target_path = if module_name.starts_with("../") || module_name.starts_with("./") {
            // It's a relative path, properly join with base dir
            base_dir.join(module_name)
        } else {
            // Regular path, could be from root or from current directory
            if base_dir.as_os_str().is_empty() {
                // No base dir, just use module name
                PathBuf::from(module_name)
            } else {
                // Join with base dir
                base_dir.join(module_name)
            }
        };
        
        // Normalize the path (resolve ../.. etc)
        let normalized = self.normalize_path(&target_path.to_string_lossy());
        
        // Try to resolve with the normalized path
        self.try_resolve_with_variants(&normalized)
    }
    
    /// Helper to normalize path by resolving parent directory navigation (..)
    fn normalize_path(&self, path: &str) -> String {
        let mut components = Vec::new();
        for comp in path.split('/') {
            match comp {
                "" | "." => {}, // Skip empty or current dir
                ".." => { 
                    if !components.is_empty() {
                        components.pop(); 
                    }
                }, // Go up one level
                _ => components.push(comp), // Regular component
            }
        }
        components.join("/")
    }
    
    /// Try to resolve a path with various strategies
    #[allow(dead_code)]
    fn try_resolve_with_variants(&self, path: &str) -> Result<(String, String)> {
        #[cfg(not(target_arch = "wasm32"))]
        let templates_guard = self.templates.lock().unwrap();
        #[cfg(target_arch = "wasm32")]
        let templates_guard = self.templates.borrow();
        
        // Normalize the path for consistent lookup
        let normalized_path = path.replace('\\', "/");
        
        // Strategy 1: Direct lookup
        if templates_guard.contains_key(&normalized_path) {
            return Ok((normalized_path.clone(), normalized_path));
        }
        
        // Strategy 2: Try with extensions if needed
        let p = Path::new(&normalized_path);
        if p.extension().is_none() {
            let with_luat_ext = format!("{}.luat", normalized_path);
            if templates_guard.contains_key(&with_luat_ext) {
                return Ok((with_luat_ext.clone(), with_luat_ext));
            }
            
            let with_lua_ext = format!("{}.lua", normalized_path);
            if templates_guard.contains_key(&with_lua_ext) {
                return Ok((with_lua_ext.clone(), with_lua_ext));
            }
        }
        
        // Strategy 3: Try without leading ./
        if normalized_path.starts_with("./") {
            let no_dot = normalized_path.strip_prefix("./").unwrap();
            if templates_guard.contains_key(no_dot) {
                return Ok((no_dot.to_string(), no_dot.to_string()));
            }
            
            // Also try with extensions if needed
            if !no_dot.ends_with(".luat") && !no_dot.ends_with(".lua") {
                let no_dot_luat = format!("{}.luat", no_dot);
                if templates_guard.contains_key(&no_dot_luat) {
                    return Ok((no_dot_luat.clone(), no_dot_luat));
                }
                
                let no_dot_lua = format!("{}.lua", no_dot);
                if templates_guard.contains_key(&no_dot_lua) {
                    return Ok((no_dot_lua.clone(), no_dot_lua));
                }
            }
        }
        
        // Strategy 4: Try with component name only (helps with resolving just base component names like "UI" instead of "ui/UI")
        let basename = p.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(normalized_path.as_str());
        
        if templates_guard.contains_key(basename) {
            return Ok((basename.to_string(), basename.to_string()));
        }
        
        // Strategy 5: Try component name with first letter capitalized (common React-style)
        let capitalized_basename = basename.chars().enumerate()
            .map(|(i, c)| if i == 0 { c.to_uppercase().next().unwrap_or(c) } else { c })
            .collect::<String>();
            
        if capitalized_basename != basename && templates_guard.contains_key(&capitalized_basename) {
            return Ok((capitalized_basename.clone(), capitalized_basename));
        }
        
        // Strategy 6: Try component name with extensions
        if !basename.ends_with(".luat") && !basename.ends_with(".lua") {
            let basename_luat = format!("{}.luat", basename);
            if templates_guard.contains_key(&basename_luat) {
                return Ok((basename_luat.clone(), basename_luat));
            }
            
            let basename_lua = format!("{}.lua", basename);
            if templates_guard.contains_key(&basename_lua) {
                return Ok((basename_lua.clone(), basename_lua));
            }
            
            // Try with capitalized name too
            let capitalized_luat = format!("{}.luat", capitalized_basename);
            if capitalized_basename != basename && templates_guard.contains_key(&capitalized_luat) {
                return Ok((capitalized_luat.clone(), capitalized_luat));
            }
        }
        
        // Not found
        Err(LuatError::ResolutionError(format!(
            "Module '{}' not found in memory resolver", 
            path
        )))
    }
}

impl ResourceResolver for MemoryResourceResolver {
    fn resolve(&self, importer_path: &str, module_name: &str) -> Result<ResolvedResource> {
        //println!("DEBUG: Memory resolver resolving: importer='{}', module='{}'", importer_path, module_name);

        // First, check for direct match with the exact path
        #[cfg(not(target_arch = "wasm32"))]
        let templates_guard = self.templates.lock().unwrap();
        #[cfg(target_arch = "wasm32")]
        let templates_guard = self.templates.borrow();
        
        // Direct match - if the module_name is an exact key
        if templates_guard.contains_key(module_name) {
            //println!("DEBUG: Direct key match found for '{}'", module_name);
            let source = templates_guard.get(module_name).unwrap().clone();
            return Ok(ResolvedResource { path: module_name.to_string(), source });
        }
        
        // Try with extensions for direct lookup
        if !module_name.ends_with(".luat") && !module_name.ends_with(".lua") {
            // Try with .luat extension
            let luat_name = format!("{}.luat", module_name);
            if templates_guard.contains_key(&luat_name) {
                //println!("DEBUG: Found with .luat extension: '{}'", luat_name);
                let source = templates_guard.get(&luat_name).unwrap().clone();
                return Ok(ResolvedResource { path: luat_name, source });
            }
            
            // Try with .lua extension
            let lua_name = format!("{}.lua", module_name);
            if templates_guard.contains_key(&lua_name) {
                //println!("DEBUG: Found with .lua extension: '{}'", lua_name);
                let source = templates_guard.get(&lua_name).unwrap().clone();
                return Ok(ResolvedResource { path: lua_name, source });
            }
        }
        
        // Handle relative paths
        if !importer_path.is_empty() && (module_name.starts_with("./") || module_name.starts_with("../")) {
            //println!("DEBUG: Handling relative path: '{}'", module_name);
            
            // Get importer's directory
            let importer_dir = std::path::Path::new(importer_path)
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .to_string_lossy()
                .to_string();
                
            // Join with relative path
            let joined_path = if importer_dir.is_empty() {
                module_name.trim_start_matches("./").to_string()
            } else {
                format!("{}/{}", importer_dir, module_name)
            };
            
            // Normalize the path
            let normalized_path = self.normalize_path(&joined_path);
            //println!("DEBUG: Normalized path: '{}'", normalized_path);
            
            // Check if it exists
            if templates_guard.contains_key(&normalized_path) {
                //println!("DEBUG: Found normalized path: '{}'", normalized_path);
                let source = templates_guard.get(&normalized_path).unwrap().clone();
                return Ok(ResolvedResource { path: normalized_path.clone(), source });
            }
            
            // Try with extensions
            if !normalized_path.ends_with(".luat") && !normalized_path.ends_with(".lua") {
                // Try with .luat extension
                let luat_path = format!("{}.luat", normalized_path);
                if templates_guard.contains_key(&luat_path) {
                    //println!("DEBUG: Found normalized path with .luat: '{}'", luat_path);
                    let source = templates_guard.get(&luat_path).unwrap().clone();
                    return Ok(ResolvedResource { path: luat_path, source });
                }
                
                // Try with .lua extension
                let lua_path = format!("{}.lua", normalized_path);
                if templates_guard.contains_key(&lua_path) {
                    //println!("DEBUG: Found normalized path with .lua: '{}'", lua_path);
                    let source = templates_guard.get(&lua_path).unwrap().clone();
                    return Ok(ResolvedResource { path: lua_path, source });
                }
            }
        }
        
        // If we get here, we couldn't find the module
        //println!("DEBUG: Could not resolve module: '{}'", module_name);
        Err(LuatError::ResolutionError(format!(
            "Module '{}' not found in memory resolver", module_name
        )))
    }

    fn get_resolved_path(&self, importer_path: &str, module_name: &str) -> Result<String> {
        // We'll directly use resolve and return the path
        let resolved = self.resolve(importer_path, module_name)?;
        Ok(resolved.path)
    }

    fn clone_box(&self) -> Box<dyn ResourceResolver> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use super::*;
    
    #[test]
    fn test_memory_resolver_direct() {
        let resolver = MemoryResourceResolver::new();
        resolver.add_template("test.luat", "<div>Hello</div>".to_string());
        
        let resolved = resolver.resolve("", "test.luat").unwrap();
        assert_eq!(resolved.path, "test.luat");
        assert_eq!(resolved.source, "<div>Hello</div>");
        
        let resolved_no_ext = resolver.resolve("", "test").unwrap();
        assert_eq!(resolved_no_ext.path, "test.luat");
        assert_eq!(resolved_no_ext.source, "<div>Hello</div>");
        
        assert!(resolver.resolve("", "nonexistent").is_err());
    }

    #[test]
    fn test_memory_resolver_relative_paths() {
        let resolver = MemoryResourceResolver::new();
        resolver.add_template("app/main.luat", "require('./component')".to_string());
        resolver.add_template("app/component.luat", "<p>Component</p>".to_string());

        let resolved = resolver.resolve("app/main.luat", "./component").unwrap();
        assert_eq!(resolved.path, "app/component.luat");
        assert_eq!(resolved.source, "<p>Component</p>");

        let resolved_no_ext = resolver.resolve("app/main.luat", "./component").unwrap();
        assert_eq!(resolved_no_ext.path, "app/component.luat");
    }

    #[test]
    fn test_memory_resolver_parent_paths() {
        let resolver = MemoryResourceResolver::new();
        resolver.add_template("app/ui/button.luat", "require('../common/utils')".to_string());
        resolver.add_template("app/common/utils.luat", "function util(){}".to_string());

        let resolved = resolver.resolve("app/ui/button.luat", "../common/utils").unwrap();
        assert_eq!(resolved.path, "app/common/utils.luat");
        assert_eq!(resolved.source, "function util(){}");

        let resolved_no_ext = resolver.resolve("app/ui/button.luat", "../common/utils").unwrap();
        assert_eq!(resolved_no_ext.path, "app/common/utils.luat");
    }

    #[test]
    #[ignore]
    fn test_memory_resolver_absolute_paths_from_root() {
        let resolver = MemoryResourceResolver::new();
        resolver.add_resource("lib/core.luat", "<div>Core content</div>");
        resolver.add_resource("app/main.luat", r#"<script>local Core = require("lib/core");</script><Core />"#);

        // Using absolute path from root (without leading /)
        let resolved = resolver.resolve("app/main.luat", "lib/core").unwrap();
        assert_eq!(resolved.path, "lib/core.luat");
        assert_eq!(resolved.source, "<div>Core content</div>");

        // Test with path from empty importer
        let resolved_from_empty_importer = resolver.resolve("", "lib/core").unwrap();
        assert_eq!(resolved_from_empty_importer.path, "lib/core.luat");
        
        // Test with engine to verify full workflow
        let engine = Engine::with_memory_cache(resolver.clone(), 10).unwrap();
        let module = engine.compile_entry("app/main.luat").unwrap();
        let context = engine.to_value(HashMap::<String, String>::new()).unwrap();
        let rendered = engine.render(&module, &context).unwrap();
        assert!(rendered.contains("Core content"), "Should render content from absolute import");
    }

     #[test]
    fn test_memory_resolver_add_remove_clear() {
        let resolver = MemoryResourceResolver::new();
        resolver.add_template("temp.luat", "test".to_string());
        assert!(resolver.resolve("", "temp").is_ok());

        resolver.remove_template("temp.luat");
        assert!(resolver.resolve("", "temp").is_err());

        resolver.add_template("temp1.luat", "test1".to_string());
        resolver.add_template("temp2.luat", "test2".to_string());
        resolver.clear();
        assert!(resolver.resolve("", "temp1").is_err());
        assert!(resolver.resolve("", "temp2").is_err());
    }
}
