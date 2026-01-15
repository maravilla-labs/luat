// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::error::{LuatError, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// Normalizes a module name by removing file extensions
fn normalize_module_name(name: &str) -> String {
    let name = name.trim();
    if let Some(stripped) = name.strip_suffix(".lua") {
        return stripped.to_string();
    } else if let Some(stripped) = name.strip_suffix(".luat") {
        return stripped.to_string();
    }
    name.to_string()
}

/// Orders sources based on their dependencies to ensure correct load order
/// Returns a new vector with the same sources, but ordered by dependency
pub fn order_sources(sources: Vec<(String, String)>) -> Result<Vec<(String, String)>> {
    // Create a regex to find require statements
    let require_re = Regex::new(r#"require\s*\(\s*["']([^"']+)["']\s*\)"#).unwrap();
    
    // Map from module name to its source code
    let mut sources_map: HashMap<String, String> = HashMap::new();
    
    // Map from normalized module name to original module name
    let mut normalized_to_original: HashMap<String, String> = HashMap::new();
    
    // Build the maps
    for (name, source) in &sources {
        let normalized_name = normalize_module_name(name);
        sources_map.insert(name.clone(), source.clone());
        normalized_to_original.insert(normalized_name, name.clone());
    }
    
    // Build dependency graph (from module name to its dependencies)
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    
    // Extract dependencies from each source
    for (name, src) in &sources_map {
        let mut module_deps = Vec::new();
        
        for cap in require_re.captures_iter(src) {
            let dep_path = cap[1].to_string();
            let normalized_dep = normalize_module_name(&dep_path);
            
            // Only include dependencies that are in our source list
            if let Some(original_dep) = normalized_to_original.get(&normalized_dep) {
                module_deps.push(original_dep.clone());
            }
        }
        
        deps.insert(name.clone(), module_deps);
    }
    
    // Perform topological sort
    let sorted_names = topo_sort(&deps)?;
    
    // Build the ordered sources list
    let mut ordered_sources = Vec::new();
    for name in sorted_names {
        if let Some(source) = sources_map.get(&name) {
            ordered_sources.push((name.clone(), source.clone()));
        }
    }
    
    Ok(ordered_sources)
}

/// Perform a topological sort on the dependency graph
fn topo_sort(deps: &HashMap<String, Vec<String>>) -> Result<Vec<String>> {
    // Build in-degree map (how many modules depend on this module)
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    
    // Initialize in-degree for all modules to 0
    for module_name in deps.keys() {
        in_degree.entry(module_name.clone()).or_insert(0);
    }
    
    // Count incoming edges
    for dependencies in deps.values() {
        for dep in dependencies {
            *in_degree.entry(dep.clone()).or_insert(0) += 1;
        }
    }
    
    // Add nodes with 0 in-degree to the queue (modules with no dependencies)
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter_map(|(module, &degree)| if degree == 0 { Some(module.clone()) } else { None })
        .collect();
    
    // Process the modules in topological order
    let mut sorted = Vec::new();
    let mut visited = HashSet::new();
    
    while let Some(module) = queue.pop() {
        // Skip if already processed (should not happen, but just in case)
        if visited.contains(&module) {
            continue;
        }
        
        sorted.push(module.clone());
        visited.insert(module.clone());
        
        // Update in-degree of dependencies
        if let Some(dependencies) = deps.get(&module) {
            for dep in dependencies {
                if let Some(in_deg) = in_degree.get_mut(dep) {
                    *in_deg -= 1;
                    if *in_deg == 0 && !visited.contains(dep) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
    }
    
    // Check for cycles
    if sorted.len() != deps.len() {
        return Err(LuatError::InvalidTemplate("Circular dependency detected in template modules".to_string()));
    }
    
    // For bundling, we want dependencies to come BEFORE modules that use them
    sorted.reverse();
    
    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_module_name() {
        assert_eq!(normalize_module_name("test"), "test");
        assert_eq!(normalize_module_name("test.lua"), "test");
        assert_eq!(normalize_module_name("test.luat"), "test");
        assert_eq!(normalize_module_name("path/to/module.luat"), "path/to/module");
    }

    #[test]
    fn test_topo_sort_simple() {
        let mut deps = HashMap::new();
        deps.insert("A".to_string(), vec!["B".to_string(), "C".to_string()]);
        deps.insert("B".to_string(), vec!["C".to_string()]);
        deps.insert("C".to_string(), vec![]);
        
        let sorted = topo_sort(&deps).unwrap();
        
        // With our implementation, dependencies come before the modules that use them
        // C should be first (no dependencies)
        // B should be next (depends on C)
        // A should be last (depends on B and C)
        
        // Find positions
        let c_pos = sorted.iter().position(|x| x == "C").unwrap();
        let b_pos = sorted.iter().position(|x| x == "B").unwrap();
        let a_pos = sorted.iter().position(|x| x == "A").unwrap();
        
        // In our topological sort, C must come before B (since B depends on C)
        // and both must come before A (since A depends on both)
        assert!(c_pos < b_pos, "C should come before B in topological order");
        assert!(b_pos < a_pos, "B should come before A in topological order");
    }
    
    #[test]
    fn test_topo_sort_cycle() {
        let mut deps = HashMap::new();
        deps.insert("A".to_string(), vec!["B".to_string()]);
        deps.insert("B".to_string(), vec!["C".to_string()]);
        deps.insert("C".to_string(), vec!["A".to_string()]);
        
        let result = topo_sort(&deps);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_order_sources_with_extensions() {
        let sources = vec![
            ("app.luat".to_string(), 
             "local Header = require('components/Header.luat')\nreturn {}".to_string()),
            ("components/Header.luat".to_string(), 
             "return {}".to_string()),
        ];

        let ordered = order_sources(sources).unwrap();
        
        // Header should come before app
        assert_eq!(ordered[0].0, "components/Header.luat");
        assert_eq!(ordered[1].0, "app.luat");
    }
    
    #[test]
    fn test_order_sources_with_mixed_extensions() {
        let sources = vec![
            ("app.luat".to_string(), 
             "local Header = require('components/Header')\nreturn {}".to_string()),
            ("components/Header.luat".to_string(), 
             "return {}".to_string()),
        ];

        let ordered = order_sources(sources).unwrap();
        
        // Header should come before app
        assert_eq!(ordered[0].0, "components/Header.luat");
        assert_eq!(ordered[1].0, "app.luat");
    }
}
