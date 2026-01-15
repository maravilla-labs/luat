// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Types for the file-based routing system.

use std::collections::HashMap;
use std::path::PathBuf;

/// Represents a discovered route in the filesystem
#[derive(Debug, Clone)]
pub struct Route {
    /// The URL pattern in matchit format (e.g., "/blog/:slug")
    pub pattern: String,

    /// The filesystem path relative to routes dir (e.g., "blog/[slug]")
    pub fs_path: PathBuf,

    /// Path to +page.luat if it exists
    pub page: Option<PathBuf>,

    /// Path to +layout.luat if it exists (for this segment only)
    pub layout: Option<PathBuf>,

    /// Path to +page.server.lua if it exists
    pub server: Option<PathBuf>,

    /// Path to +server.lua if it exists (API route)
    pub api: Option<PathBuf>,

    /// Path to +error.luat if it exists
    pub error: Option<PathBuf>,

    /// All layouts from root to this route (for composition)
    pub layouts: Vec<PathBuf>,

    /// Action templates discovered for this route.
    /// Maps action name to template path:
    /// - "default" -> "blog/[slug]/edit/default.luat"
    /// - "login" -> "blog/[slug]/edit/login.luat"
    /// - "publish" -> "blog/[slug]/edit/publish.luat"
    pub action_templates: HashMap<String, PathBuf>,
}

impl Route {
    /// Returns true if this is an API-only route (has +server.lua but no +page.luat)
    pub fn is_api_route(&self) -> bool {
        self.api.is_some() && self.page.is_none()
    }

    /// Returns true if this is a page route
    pub fn is_page_route(&self) -> bool {
        self.page.is_some()
    }
}

/// Result of matching a URL to a route
#[derive(Debug)]
pub struct RouteMatch<'a> {
    /// The matched route
    pub route: &'a Route,

    /// URL parameters extracted from the path
    pub params: Vec<(String, String)>,
}

impl<'a> RouteMatch<'a> {
    /// Get a parameter by name
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Segment type in a route path
#[derive(Debug, Clone, PartialEq)]
pub enum SegmentType {
    /// Static segment (e.g., "blog")
    Static(String),

    /// Dynamic parameter (e.g., "[slug]" -> ":slug")
    Dynamic(String),

    /// Optional parameter (e.g., "[[tab]]" -> ":tab?")
    Optional(String),

    /// Catch-all/rest parameter (e.g., "[...rest]" -> "*rest")
    CatchAll(String),
}

impl SegmentType {
    /// Parse a filesystem segment into a SegmentType
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

    /// Convert to matchit pattern segment
    /// matchit uses {param} for dynamic segments and {*param} for catch-all
    pub fn to_pattern(&self) -> String {
        match self {
            SegmentType::Static(s) => s.clone(),
            SegmentType::Dynamic(name) => format!("{{{}}}", name),
            SegmentType::Optional(name) => format!("{{{}}}", name), // Handled specially
            SegmentType::CatchAll(name) => format!("{{*{}}}", name),
        }
    }

    /// Returns true if this is an optional segment
    pub fn is_optional(&self) -> bool {
        matches!(self, SegmentType::Optional(_))
    }
}

/// Configuration for the routing system
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Directory containing routes (default: "src/routes")
    pub routes_dir: String,

    /// Directory for shared Lua modules (default: "src/lib")
    pub lib_dir: String,

    /// Directory for static files (default: "static")
    pub static_dir: String,

    /// Use simplified routing mode (direct file-to-URL mapping)
    pub simplified: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            routes_dir: "src/routes".to_string(),
            lib_dir: "src/lib".to_string(),
            static_dir: "static".to_string(),
            simplified: false,
        }
    }
}

/// Error types for routing operations.
#[derive(Debug, thiserror::Error)]
pub enum RoutingError {
    /// Route discovery failed during filesystem scanning.
    #[error("Route discovery failed: {0}")]
    DiscoveryFailed(String),

    /// Invalid route pattern syntax.
    #[error("Invalid route pattern: {0}")]
    InvalidPattern(String),

    /// Route was not found for the given path.
    #[error("Route not found: {0}")]
    NotFound(String),

    /// Error executing a load function.
    #[error("Load function error: {0}")]
    LoadError(String),

    /// Filesystem I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for routing operations.
pub type RoutingResult<T> = Result<T, RoutingError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_parse_static() {
        assert_eq!(
            SegmentType::parse("blog"),
            SegmentType::Static("blog".to_string())
        );
    }

    #[test]
    fn test_segment_parse_dynamic() {
        assert_eq!(
            SegmentType::parse("[slug]"),
            SegmentType::Dynamic("slug".to_string())
        );
    }

    #[test]
    fn test_segment_parse_optional() {
        assert_eq!(
            SegmentType::parse("[[tab]]"),
            SegmentType::Optional("tab".to_string())
        );
    }

    #[test]
    fn test_segment_parse_catchall() {
        assert_eq!(
            SegmentType::parse("[...rest]"),
            SegmentType::CatchAll("rest".to_string())
        );
    }

    #[test]
    fn test_segment_to_pattern() {
        assert_eq!(SegmentType::Static("blog".to_string()).to_pattern(), "blog");
        assert_eq!(
            SegmentType::Dynamic("slug".to_string()).to_pattern(),
            "{slug}"
        );
        assert_eq!(
            SegmentType::CatchAll("rest".to_string()).to_pattern(),
            "{*rest}"
        );
    }
}
