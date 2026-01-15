// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! File-based routing for the Luat engine.
//!
//! This module provides SvelteKit-style file-based routing:
//! - `+page.luat` → Page routes
//! - `+server.lua` → API routes
//! - `+layout.luat` / `+layout.server.lua` → Layouts
//! - `[param]` directories → Dynamic parameters
//! - `[[optional]]` → Optional parameters
//! - `[...rest]` → Catch-all parameters

use std::collections::HashMap;
use std::path::Path;

/// Represents a discovered route.
#[derive(Debug, Clone)]
pub struct Route {
    /// The URL pattern (e.g., "/blog/{slug}")
    pub pattern: String,

    /// The filesystem path relative to routes dir
    pub fs_path: String,

    /// Extracted URL parameters
    pub params: HashMap<String, String>,

    /// Path to +page.luat if it exists
    pub page: Option<String>,

    /// Path to +layout.luat if it exists (for this segment only)
    pub layout: Option<String>,

    /// Path to +page.server.lua if it exists
    pub page_server: Option<String>,

    /// Path to +server.lua if it exists (API route)
    pub api: Option<String>,

    /// Path to +layout.server.lua if it exists
    pub layout_server: Option<String>,

    /// All layouts from root to this route (for composition)
    pub layouts: Vec<String>,

    /// All layout server files from root to this route
    pub layout_servers: Vec<String>,

    /// Path to +error.luat if it exists
    pub error: Option<String>,

    /// Action templates in the same directory (action name -> path)
    pub action_templates: HashMap<String, String>,
}

impl Route {
    /// Creates a new empty route with the given pattern.
    pub fn new(pattern: impl Into<String>, fs_path: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            fs_path: fs_path.into(),
            params: HashMap::new(),
            page: None,
            layout: None,
            page_server: None,
            api: None,
            layout_server: None,
            layouts: Vec::new(),
            layout_servers: Vec::new(),
            error: None,
            action_templates: HashMap::new(),
        }
    }

    /// Returns true if this is an API-only route (has +server.lua but no +page.luat)
    pub fn is_api_route(&self) -> bool {
        self.api.is_some() && self.page.is_none()
    }

    /// Returns true if this is a page route
    pub fn is_page_route(&self) -> bool {
        self.page.is_some()
    }

    /// Sets URL parameters extracted from matching.
    pub fn with_params(mut self, params: HashMap<String, String>) -> Self {
        self.params = params;
        self
    }
}

/// Segment type in a route path.
#[derive(Debug, Clone, PartialEq)]
pub enum SegmentType {
    /// Static segment (e.g., "blog")
    Static(String),

    /// Dynamic parameter (e.g., "[slug]" -> "{slug}")
    Dynamic(String),

    /// Optional parameter (e.g., "[[tab]]")
    Optional(String),

    /// Catch-all/rest parameter (e.g., "[...rest]" -> "{*rest}")
    CatchAll(String),
}

impl SegmentType {
    /// Parse a filesystem segment into a SegmentType.
    pub fn parse(segment: &str) -> Self {
        if segment.starts_with("[[") && segment.ends_with("]]") {
            // Optional parameter: [[name]]
            let name = segment[2..segment.len() - 2].to_string();
            SegmentType::Optional(name)
        } else if segment.starts_with("[...") && segment.ends_with(']') {
            // Catch-all parameter: [...rest]
            let name = segment[4..segment.len() - 1].to_string();
            SegmentType::CatchAll(name)
        } else if segment.starts_with('[') && segment.ends_with(']') {
            // Dynamic parameter: [name]
            let name = segment[1..segment.len() - 1].to_string();
            SegmentType::Dynamic(name)
        } else {
            // Static segment
            SegmentType::Static(segment.to_string())
        }
    }

    /// Convert to matchit pattern segment.
    /// matchit uses {param} for dynamic segments and {*param} for catch-all.
    pub fn to_pattern(&self) -> String {
        match self {
            SegmentType::Static(s) => s.clone(),
            SegmentType::Dynamic(name) => format!("{{{}}}", name),
            SegmentType::Optional(name) => format!("{{{}}}", name),
            SegmentType::CatchAll(name) => format!("{{*{}}}", name),
        }
    }

    /// Returns true if this is an optional segment.
    pub fn is_optional(&self) -> bool {
        matches!(self, SegmentType::Optional(_))
    }
}

/// Convert a filesystem path to a matchit URL pattern.
pub fn path_to_pattern(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }

    let segments: Vec<String> = Path::new(path)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| SegmentType::parse(s).to_pattern())
        .collect();

    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

/// The router that handles route discovery and URL matching.
pub struct Router {
    /// matchit router for fast URL matching
    matcher: matchit::Router<usize>,

    /// All discovered routes (indexed by matcher)
    routes: Vec<Route>,
}

impl Router {
    /// Creates a new empty router.
    pub fn new() -> Self {
        Self {
            matcher: matchit::Router::new(),
            routes: Vec::new(),
        }
    }

    /// Discovers routes from a list of file paths.
    ///
    /// This is the main entry point for route discovery. Pass all file paths
    /// from your resolver and this will identify routes based on file conventions.
    ///
    /// # Arguments
    ///
    /// * `paths` - Iterator of file paths (relative to routes root)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let paths = vec![
    ///     "+page.luat",
    ///     "about/+page.luat",
    ///     "blog/[slug]/+page.luat",
    ///     "api/posts/+server.lua",
    /// ];
    /// let router = Router::from_paths(paths.into_iter());
    /// ```
    pub fn from_paths<I, S>(paths: I) -> Self
    where
        I: Iterator<Item = S>,
        S: AsRef<str>,
    {
        let mut router = Self::new();
        let mut route_dirs: HashMap<String, Route> = HashMap::new();
        let mut layouts_by_dir: HashMap<String, String> = HashMap::new();
        let mut layout_servers_by_dir: HashMap<String, String> = HashMap::new();
        let mut action_templates_by_dir: HashMap<String, HashMap<String, String>> = HashMap::new();

        // First pass: collect all files by directory
        for path_ref in paths {
            let path = path_ref.as_ref();
            let path_obj = Path::new(path);

            let file_name = path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            let parent = path_obj
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            // Track layouts
            if file_name == "+layout.luat" {
                layouts_by_dir.insert(parent.clone(), path.to_string());
            } else if file_name == "+layout.server.lua" {
                layout_servers_by_dir.insert(parent.clone(), path.to_string());
            }

            // Track action templates - look for (fragments) subfolder pattern
            if file_name.ends_with(".luat") {
                let path_obj = Path::new(path);
                if let Some(parent_name) = path_obj
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                {
                    if parent_name == "(fragments)" {
                        // Get grandparent (the actual route dir)
                        if let Some(grandparent) = path_obj.parent().and_then(|p| p.parent()) {
                            let route_dir = grandparent.to_string_lossy().to_string();
                            let action_name = file_name.trim_end_matches(".luat").to_string();
                            action_templates_by_dir
                                .entry(route_dir)
                                .or_default()
                                .insert(action_name, path.to_string());
                        }
                    }
                }
            }

            // Create or update route for this directory
            let route = route_dirs
                .entry(parent.clone())
                .or_insert_with(|| {
                    let pattern = path_to_pattern(&parent);
                    Route::new(pattern, parent.clone())
                });

            match file_name {
                "+page.luat" => route.page = Some(path.to_string()),
                "+page.server.lua" => route.page_server = Some(path.to_string()),
                "+server.lua" => route.api = Some(path.to_string()),
                "+layout.luat" => route.layout = Some(path.to_string()),
                "+layout.server.lua" => route.layout_server = Some(path.to_string()),
                "+error.luat" => route.error = Some(path.to_string()),
                _ => {}
            }
        }

        // Second pass: collect layout chains and filter valid routes
        let mut routes: Vec<Route> = route_dirs
            .into_iter()
            .filter(|(_, route)| route.page.is_some() || route.api.is_some())
            .map(|(dir, mut route)| {
                // Collect layouts from root to this route
                route.layouts = Self::collect_layouts(&dir, &layouts_by_dir);
                route.layout_servers = Self::collect_layouts(&dir, &layout_servers_by_dir);
                if let Some(templates) = action_templates_by_dir.get(&dir) {
                    route.action_templates = templates.clone();
                }
                route
            })
            .collect();

        // Sort routes: static before dynamic
        routes.sort_by(|a, b| {
            let a_is_dynamic = a.pattern.contains('{');
            let b_is_dynamic = b.pattern.contains('{');
            match (a_is_dynamic, b_is_dynamic) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.pattern.cmp(&b.pattern),
            }
        });

        // Build the matchit router
        for (index, route) in routes.iter().enumerate() {
            if let Err(e) = router.matcher.insert(&route.pattern, index) {
                tracing::warn!("Could not register route {}: {}", route.pattern, e);
            }
        }

        router.routes = routes;
        router
    }

    /// Match a URL path to a route.
    pub fn match_url(&self, path: &str) -> Option<Route> {
        let normalized_path = if path.is_empty() || path == "/" {
            "/"
        } else {
            path.trim_end_matches('/')
        };

        match self.matcher.at(normalized_path) {
            Ok(matched) => {
                let route = self.routes[*matched.value].clone();
                let params: HashMap<String, String> = matched
                    .params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                Some(route.with_params(params))
            }
            Err(_) => None,
        }
    }

    /// Get all routes (for debugging/listing).
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Collect all layouts from root to the given directory.
    fn collect_layouts(dir: &str, layouts_by_dir: &HashMap<String, String>) -> Vec<String> {
        let mut layouts = Vec::new();
        let mut current = String::new();

        // Check root layout first
        if let Some(root_layout) = layouts_by_dir.get("") {
            layouts.push(root_layout.clone());
        }

        // Walk down the path and collect layouts
        for component in Path::new(dir).components() {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(&component.as_os_str().to_string_lossy());

            if let Some(layout) = layouts_by_dir.get(&current) {
                if !layouts.contains(layout) {
                    layouts.push(layout.clone());
                }
            }
        }

        layouts
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_parse() {
        assert_eq!(
            SegmentType::parse("blog"),
            SegmentType::Static("blog".to_string())
        );
        assert_eq!(
            SegmentType::parse("[slug]"),
            SegmentType::Dynamic("slug".to_string())
        );
        assert_eq!(
            SegmentType::parse("[[tab]]"),
            SegmentType::Optional("tab".to_string())
        );
        assert_eq!(
            SegmentType::parse("[...rest]"),
            SegmentType::CatchAll("rest".to_string())
        );
    }

    #[test]
    fn test_path_to_pattern() {
        assert_eq!(path_to_pattern(""), "/");
        assert_eq!(path_to_pattern("about"), "/about");
        assert_eq!(path_to_pattern("blog/[slug]"), "/blog/{slug}");
        assert_eq!(path_to_pattern("docs/[...rest]"), "/docs/{*rest}");
    }

    #[test]
    fn test_router_from_paths() {
        let paths = vec![
            "+page.luat",
            "+layout.luat",
            "about/+page.luat",
            "blog/+page.luat",
            "blog/+layout.luat",
            "blog/[slug]/+page.luat",
            "blog/[slug]/+page.server.lua",
            "api/posts/+server.lua",
        ];

        let router = Router::from_paths(paths.into_iter());
        let routes = router.routes();

        assert!(routes.len() >= 4, "Expected at least 4 routes");

        // Check patterns exist
        let patterns: Vec<&str> = routes.iter().map(|r| r.pattern.as_str()).collect();
        assert!(patterns.contains(&"/"), "Missing root route");
        assert!(patterns.contains(&"/about"), "Missing /about route");
        assert!(patterns.contains(&"/blog"), "Missing /blog route");
        assert!(patterns.contains(&"/blog/{slug}"), "Missing /blog/{{slug}} route");
        assert!(patterns.contains(&"/api/posts"), "Missing /api/posts route");
    }

    #[test]
    fn test_url_matching() {
        let paths = vec![
            "+page.luat",
            "about/+page.luat",
            "blog/[slug]/+page.luat",
        ];

        let router = Router::from_paths(paths.into_iter());

        // Test root match
        let root = router.match_url("/").unwrap();
        assert_eq!(root.pattern, "/");

        // Test static match
        let about = router.match_url("/about").unwrap();
        assert_eq!(about.pattern, "/about");

        // Test dynamic match
        let post = router.match_url("/blog/hello-world").unwrap();
        assert_eq!(post.pattern, "/blog/{slug}");
        assert_eq!(post.params.get("slug"), Some(&"hello-world".to_string()));
    }

    #[test]
    fn test_layout_chain() {
        let paths = vec![
            "+page.luat",
            "+layout.luat",
            "blog/+page.luat",
            "blog/+layout.luat",
            "blog/[slug]/+page.luat",
        ];

        let router = Router::from_paths(paths.into_iter());
        let post = router.match_url("/blog/hello").unwrap();

        // Should have root layout and blog layout
        assert_eq!(post.layouts.len(), 2);
        assert!(post.layouts[0].contains("+layout.luat"));
        assert!(post.layouts[1].contains("blog/+layout.luat"));
    }

    #[test]
    fn test_api_route() {
        let paths = vec!["api/posts/+server.lua"];

        let router = Router::from_paths(paths.into_iter());
        let api = router.match_url("/api/posts").unwrap();

        assert!(api.is_api_route());
        assert!(!api.is_page_route());
        assert!(api.api.is_some());
    }
}
