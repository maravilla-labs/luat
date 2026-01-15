// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! SvelteKit-style file-based routing for luat-cli.
//!
//! This module provides:
//! - File-based route discovery from `src/routes/` directory
//! - Dynamic routes with `[param]`, `[[optional]]`, and `[...rest]` syntax
//! - URL pattern matching using matchit
//! - Layout chain resolution

pub mod types;

use glob::glob;
use matchit::Router as MatchitRouter;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use types::{Route, RouteMatch, RoutingError, RoutingResult, SegmentType};

/// The main router that handles route discovery and URL matching
pub struct Router {
    /// matchit router for fast URL matching
    matcher: MatchitRouter<usize>,

    /// All discovered routes (indexed by matcher)
    routes: Vec<Route>,

    /// Routes directory
    routes_dir: PathBuf,
}

impl Router {
    /// Discover routes from the filesystem and build the router
    pub fn discover(routes_dir: &Path) -> RoutingResult<Self> {
        let mut routes = Vec::new();
        let mut layouts_by_dir: HashMap<PathBuf, PathBuf> = HashMap::new();

        // First pass: find all layouts
        let layout_pattern = format!("{}/**/+layout.luat", routes_dir.display());
        for path in (glob(&layout_pattern).map_err(|e| RoutingError::DiscoveryFailed(e.to_string()))?).flatten() {
            if let Some(parent) = path.parent() {
                let relative = parent
                    .strip_prefix(routes_dir)
                    .unwrap_or(parent)
                    .to_path_buf();
                layouts_by_dir.insert(relative, path);
            }
        }

        // Second pass: find all +page.luat files
        let page_pattern = format!("{}/**/+page.luat", routes_dir.display());
        for path in glob(&page_pattern).map_err(|e| RoutingError::DiscoveryFailed(e.to_string()))?.flatten() {
            if let Some(parent) = path.parent() {
                let relative_dir = parent
                    .strip_prefix(routes_dir)
                    .unwrap_or(parent)
                    .to_path_buf();

                // Build the URL pattern from the filesystem path
                let pattern = Self::path_to_pattern(&relative_dir);

                // Find all layouts from root to this route
                let layouts = Self::collect_layouts(&relative_dir, &layouts_by_dir);

                // Check for associated files
                let server = Self::find_sibling(&path, "+page.server.lua");
                let layout = layouts_by_dir.get(&relative_dir).cloned();
                let error = Self::find_sibling(&path, "+error.luat");
                let api = Self::find_sibling(&path, "+server.lua");

                // Discover action templates
                let action_templates = Self::discover_action_templates(&path);

                routes.push(Route {
                    pattern,
                    fs_path: relative_dir,
                    page: Some(path),
                    layout,
                    server,
                    api,
                    error,
                    layouts,
                    action_templates,
                });
            }
        }

        // Third pass: find API-only routes (+server.lua without +page.luat)
        let api_pattern = format!("{}/**/+server.lua", routes_dir.display());
        for path in glob(&api_pattern).map_err(|e| RoutingError::DiscoveryFailed(e.to_string()))?.flatten() {
            if let Some(parent) = path.parent() {
                let relative_dir = parent
                    .strip_prefix(routes_dir)
                    .unwrap_or(parent)
                    .to_path_buf();

                // Skip if we already have a page route for this path
                let pattern = Self::path_to_pattern(&relative_dir);
                if routes.iter().any(|r| r.pattern == pattern) {
                    continue;
                }

                routes.push(Route {
                    pattern,
                    fs_path: relative_dir,
                    page: None,
                    layout: None,
                    server: None,
                    api: Some(path),
                    error: None,
                    layouts: Vec::new(),
                    action_templates: HashMap::new(),
                });
            }
        }

        // Sort routes to ensure static routes come before dynamic ones
        // This helps matchit handle conflicts correctly
        routes.sort_by(|a, b| {
            // Static patterns should come first, then dynamic
            let a_is_dynamic = a.pattern.contains(':') || a.pattern.contains('*');
            let b_is_dynamic = b.pattern.contains(':') || b.pattern.contains('*');
            match (a_is_dynamic, b_is_dynamic) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => a.pattern.cmp(&b.pattern),
            }
        });

        // Build the matchit router
        let mut matcher = MatchitRouter::new();
        for (index, route) in routes.iter().enumerate() {
            // Handle optional parameters by registering multiple patterns
            if route.pattern.contains("/:") && Self::has_optional_segment(&route.fs_path) {
                // Register both with and without the optional segment
                let base_pattern = Self::pattern_without_optional(&route.pattern);
                let _ = matcher.insert(&base_pattern, index);
            }

            if let Err(e) = matcher.insert(&route.pattern, index) {
                // Log warning but continue - might be a duplicate pattern
                eprintln!("Warning: Could not register route {}: {}", route.pattern, e);
            }
        }

        Ok(Self {
            matcher,
            routes,
            routes_dir: routes_dir.to_path_buf(),
        })
    }

    /// Match a URL path to a route
    pub fn match_url(&self, path: &str) -> Option<RouteMatch<'_>> {
        let normalized_path = if path.is_empty() || path == "/" {
            "/"
        } else {
            path.trim_end_matches('/')
        };

        match self.matcher.at(normalized_path) {
            Ok(matched) => {
                let route = &self.routes[*matched.value];
                let params: Vec<(String, String)> = matched
                    .params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                Some(RouteMatch { route, params })
            }
            Err(_) => None,
        }
    }

    /// Get all routes (for debugging/listing)
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Get the routes directory
    pub fn routes_dir(&self) -> &Path {
        &self.routes_dir
    }

    /// Convert a filesystem path to a matchit URL pattern
    fn path_to_pattern(path: &Path) -> String {
        if path.as_os_str().is_empty() {
            return "/".to_string();
        }

        let segments: Vec<String> = path
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

    /// Check if path has an optional segment
    fn has_optional_segment(path: &Path) -> bool {
        path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| s.starts_with("[[") && s.ends_with("]]"))
                .unwrap_or(false)
        })
    }

    /// Get pattern without optional segment (for registering alternate route)
    fn pattern_without_optional(pattern: &str) -> String {
        // Remove the last segment if it's a parameter (simplified approach)
        let parts: Vec<&str> = pattern.trim_end_matches('/').split('/').collect();
        if parts.len() > 1 {
            parts[..parts.len() - 1].join("/")
        } else {
            "/".to_string()
        }
    }

    /// Collect all layouts from root to the given directory
    fn collect_layouts(
        dir: &Path,
        layouts_by_dir: &HashMap<PathBuf, PathBuf>,
    ) -> Vec<PathBuf> {
        let mut layouts = Vec::new();
        let mut current = PathBuf::new();

        // Check root layout first
        if let Some(root_layout) = layouts_by_dir.get(&PathBuf::new()) {
            layouts.push(root_layout.clone());
        }

        // Walk down the path and collect layouts
        for component in dir.components() {
            current.push(component);
            if let Some(layout) = layouts_by_dir.get(&current) {
                // Don't duplicate root layout
                if !layouts.contains(layout) {
                    layouts.push(layout.clone());
                }
            }
        }

        layouts
    }

    /// Find a sibling file (same directory, different name)
    fn find_sibling(file: &Path, sibling_name: &str) -> Option<PathBuf> {
        file.parent().map(|p| p.join(sibling_name)).filter(|p| p.exists())
    }

    /// Discover action templates in the (fragments) subfolder of the page directory.
    ///
    /// Action templates are .luat files in the (fragments) folder.
    ///
    /// For example, if you have:
    /// - src/routes/blog/[slug]/edit/+page.luat
    /// - src/routes/blog/[slug]/edit/(fragments)/delete.luat
    /// - src/routes/blog/[slug]/edit/(fragments)/POST-delete.luat
    /// - src/routes/blog/[slug]/edit/(fragments)/login.luat
    ///
    /// The action templates would be: delete, POST-delete, login
    fn discover_action_templates(page_file: &Path) -> HashMap<String, PathBuf> {
        let mut templates = HashMap::new();

        let Some(parent_dir) = page_file.parent() else {
            return templates;
        };

        // Look in (fragments) subfolder
        let fragments_dir = parent_dir.join("(fragments)");
        if !fragments_dir.is_dir() {
            return templates;
        }

        // Read directory and find .luat files
        if let Ok(entries) = std::fs::read_dir(&fragments_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Must be a file with .luat extension
                if !path.is_file() {
                    continue;
                }

                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                // Skip if not a .luat file
                if !file_name.ends_with(".luat") {
                    continue;
                }

                // Extract action name (remove .luat extension)
                let action_name = file_name.trim_end_matches(".luat").to_string();
                templates.insert(action_name, path);
            }
        }

        templates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup_test_routes(dir: &Path) {
        // Create root layout and page
        fs::write(dir.join("+layout.luat"), "<html>{@render props.children()}</html>").unwrap();
        fs::write(dir.join("+page.luat"), "<h1>Home</h1>").unwrap();

        // Create about page
        fs::create_dir_all(dir.join("about")).unwrap();
        fs::write(dir.join("about/+page.luat"), "<h1>About</h1>").unwrap();

        // Create blog with layout
        fs::create_dir_all(dir.join("blog")).unwrap();
        fs::write(dir.join("blog/+layout.luat"), "<div class='blog'>{@render props.children()}</div>").unwrap();
        fs::write(dir.join("blog/+page.luat"), "<h1>Blog</h1>").unwrap();
        fs::write(dir.join("blog/+page.server.lua"), "function load(ctx) return {} end").unwrap();

        // Create dynamic blog post route
        fs::create_dir_all(dir.join("blog/[slug]")).unwrap();
        fs::write(dir.join("blog/[slug]/+page.luat"), "<h1>{props.slug}</h1>").unwrap();
        fs::write(dir.join("blog/[slug]/+page.server.lua"), "function load(ctx) return { slug = ctx.params.slug } end").unwrap();

        // Create API route
        fs::create_dir_all(dir.join("api/posts")).unwrap();
        fs::write(dir.join("api/posts/+server.lua"), "function GET(ctx) return { posts = {} } end").unwrap();
    }

    #[test]
    fn test_route_discovery() {
        let dir = tempdir().unwrap();
        setup_test_routes(dir.path());

        let router = Router::discover(dir.path()).unwrap();
        let routes = router.routes();

        // Should find: /, /about, /blog, /blog/:slug, /api/posts
        assert!(routes.len() >= 4, "Expected at least 4 routes, found {}", routes.len());

        // Check patterns
        let patterns: Vec<&str> = routes.iter().map(|r| r.pattern.as_str()).collect();
        assert!(patterns.contains(&"/"), "Missing root route");
        assert!(patterns.contains(&"/about"), "Missing /about route");
        assert!(patterns.contains(&"/blog"), "Missing /blog route");
        assert!(patterns.contains(&"/blog/{slug}"), "Missing /blog/{{slug}} route");
    }

    #[test]
    fn test_url_matching() {
        let dir = tempdir().unwrap();
        setup_test_routes(dir.path());

        let router = Router::discover(dir.path()).unwrap();

        // Test root match
        let root_match = router.match_url("/");
        assert!(root_match.is_some(), "Should match root");
        assert_eq!(root_match.unwrap().route.pattern, "/");

        // Test static match
        let about_match = router.match_url("/about");
        assert!(about_match.is_some(), "Should match /about");
        assert_eq!(about_match.unwrap().route.pattern, "/about");

        // Test dynamic match
        let post_match = router.match_url("/blog/hello-world");
        assert!(post_match.is_some(), "Should match /blog/hello-world");
        let matched = post_match.unwrap();
        assert_eq!(matched.route.pattern, "/blog/{slug}");
        assert_eq!(matched.param("slug"), Some("hello-world"));
    }

    #[test]
    fn test_layout_chain() {
        let dir = tempdir().unwrap();
        setup_test_routes(dir.path());

        let router = Router::discover(dir.path()).unwrap();

        // Find the blog post route
        let post_route = router.routes().iter().find(|r| r.pattern == "/blog/{slug}");
        assert!(post_route.is_some(), "Should find blog post route");

        let route = post_route.unwrap();
        // Should have root layout and blog layout
        assert!(!route.layouts.is_empty(), "Should have at least root layout");
    }

    #[test]
    fn test_path_to_pattern() {
        assert_eq!(Router::path_to_pattern(Path::new("")), "/");
        assert_eq!(Router::path_to_pattern(Path::new("about")), "/about");
        assert_eq!(Router::path_to_pattern(Path::new("blog/[slug]")), "/blog/{slug}");
        assert_eq!(Router::path_to_pattern(Path::new("docs/[...rest]")), "/docs/{*rest}");
    }

    #[test]
    fn test_matchit_basic() {
        // Test matchit directly to understand its behavior
        let mut router: MatchitRouter<usize> = MatchitRouter::new();

        // Try with {slug} syntax - matchit uses {param} format
        router.insert("/", 0).unwrap();
        router.insert("/about", 1).unwrap();

        // matchit uses {param} format, not :param
        let pattern = "/blog/{slug}";
        eprintln!("Trying pattern: {}", pattern);
        let result1 = router.insert(pattern, 3);
        eprintln!("Insert result: {:?}", result1);

        assert!(router.at("/").is_ok());
        assert!(router.at("/about").is_ok());

        let blog_post = router.at("/blog/hello-world");
        eprintln!("matchit /blog/hello-world: {:?}", blog_post);
        assert!(blog_post.is_ok(), "Should match /blog/hello-world");
        assert_eq!(*blog_post.unwrap().value, 3);
    }

    #[test]
    fn test_api_route_detection() {
        let dir = tempdir().unwrap();
        setup_test_routes(dir.path());

        let router = Router::discover(dir.path()).unwrap();

        let api_route = router.routes().iter().find(|r| r.pattern == "/api/posts");
        assert!(api_route.is_some(), "Should find API route");
        assert!(api_route.unwrap().is_api_route(), "Should be an API route");
    }
}
