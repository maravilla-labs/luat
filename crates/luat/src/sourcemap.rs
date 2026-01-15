// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::collections::BTreeMap;

/// Source map information for a specific module in a bundle.
#[derive(Debug, Clone)]
pub struct ModuleSourceInfo {
    /// Original path of the module.
    pub path: String,
    /// Line offset in the bundle where this module starts.
    pub line_offset: usize,
    /// Original source code of the module.
    pub source: String,
    /// Number of lines in the source.
    pub line_count: usize,
}

/// Tracks the mapping between bundle lines and original module sources.
#[derive(Debug, Clone)]
pub struct BundleSourceMap {
    /// Maps module names to their source information.
    pub modules: BTreeMap<String, ModuleSourceInfo>,
}

impl Default for BundleSourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl BundleSourceMap {
    /// Creates a new empty source map.
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
        }
    }
    
    /// Add a module to the source map
    pub fn add_module(&mut self, name: &str, path: &str, line_offset: usize, source: &str) {
        let line_count = source.lines().count();
        
        self.modules.insert(
            name.to_string(),
            ModuleSourceInfo {
                path: path.to_string(),
                line_offset,
                source: source.to_string(),
                line_count,
            },
        );
    }
    
    /// Find which module contains the given line number in the bundle
    pub fn find_module_by_line(&self, bundle_line: usize) -> Option<(&String, &ModuleSourceInfo, usize)> {
        let mut current_module: Option<(&String, &ModuleSourceInfo, usize)> = None;
        
        // Find the module this line belongs to
        for (name, info) in &self.modules {
            // If this module starts after the target line, skip it
            if info.line_offset > bundle_line {
                continue;
            }
            
            // If this module contains the target line
            if info.line_offset <= bundle_line && 
               info.line_offset + info.line_count >= bundle_line {
                
                // Calculate the line number within the module's original source
                let relative_line = bundle_line - info.line_offset + 1;
                
                // If we haven't found a module yet, or this one starts later than
                // the current candidate (meaning it's more specific)
                if current_module.is_none() || 
                   info.line_offset > current_module.unwrap().1.line_offset {
                    current_module = Some((name, info, relative_line));
                }
            }
        }
        
        current_module
    }
    
    /// Get the source code context around a specific line
    pub fn get_source_context(&self, module_name: &str, line: usize, context_lines: usize) -> Option<Vec<(usize, String)>> {
        if let Some(info) = self.modules.get(module_name) {
            let lines: Vec<String> = info.source.lines().map(|s| s.to_string()).collect();

            // Calculate the range of lines to include
            let start_line = line.saturating_sub(context_lines);
            let end_line = (line + context_lines).min(lines.len());

            // Extract the lines with their line numbers
            let result: Vec<(usize, String)> = (start_line..=end_line)
                .filter_map(|l| {
                    if l < 1 || l > lines.len() {
                        None
                    } else {
                        Some((l, lines[l - 1].clone()))
                    }
                })
                .collect();

            Some(result)
        } else {
            None
        }
    }

    /// Adjusts all module line offsets by a given delta.
    ///
    /// Use this after prepending or inserting content into the bundle,
    /// which shifts all line numbers.
    pub fn adjust_offsets(&mut self, delta: isize) {
        for info in self.modules.values_mut() {
            if delta >= 0 {
                info.line_offset += delta as usize;
            } else {
                info.line_offset = info.line_offset.saturating_sub((-delta) as usize);
            }
        }
    }

    /// Translates a bundle error message to show the original module and line.
    ///
    /// Looks for patterns like `luat_bundle:4478:` and replaces them with
    /// `module_path:relative_line:` using the source map.
    pub fn translate_error(&self, error_msg: &str) -> String {
        use regex::Regex;
        use std::borrow::Cow;

        // Pattern: `luat_bundle:LINE:` where LINE is a number
        let re = Regex::new(r"luat_bundle:(\d+):").unwrap();

        let result: Cow<str> = re.replace_all(error_msg, |caps: &regex::Captures| {
            if let Ok(bundle_line) = caps[1].parse::<usize>() {
                if let Some((_, info, relative_line)) = self.find_module_by_line(bundle_line) {
                    return format!("{}:{}:", info.path, relative_line);
                }
            }
            caps[0].to_string()
        });

        result.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_source_map_translate_error() {
        let mut source_map = BundleSourceMap::new();

        // Simulate a module starting at line 100 in the bundle
        source_map.add_module(
            "src/routes/blog/+page.luat",
            "src/routes/blog/+page.luat",
            100,
            "line1\nline2\nline3\nline4\nline5", // 5 lines
        );

        // Test error translation
        let error = "syntax error: luat_bundle:103: unexpected symbol near '{'";
        let translated = source_map.translate_error(error);

        // Line 103 in bundle = line 4 in module (103 - 100 + 1 = 4)
        assert!(
            translated.contains("src/routes/blog/+page.luat:4:"),
            "Expected translated error, got: {}",
            translated
        );
        assert!(
            !translated.contains("luat_bundle"),
            "Should not contain luat_bundle"
        );
    }

    #[test]
    fn test_bundle_source_map_find_module() {
        let mut source_map = BundleSourceMap::new();

        source_map.add_module("ModuleA", "path/to/A.luat", 10, "line1\nline2\nline3");
        source_map.add_module("ModuleB", "path/to/B.luat", 20, "line1\nline2\nline3\nline4\nline5");

        // Line 12 should be in ModuleA (starts at 10, has 3 lines)
        let result = source_map.find_module_by_line(12);
        assert!(result.is_some());
        let (name, _info, relative_line) = result.unwrap();
        assert_eq!(name, "ModuleA");
        assert_eq!(relative_line, 3); // line 12 - 10 + 1 = 3

        // Line 22 should be in ModuleB (starts at 20)
        let result = source_map.find_module_by_line(22);
        assert!(result.is_some());
        let (name, _info, relative_line) = result.unwrap();
        assert_eq!(name, "ModuleB");
        assert_eq!(relative_line, 3); // line 22 - 20 + 1 = 3
    }
}
