// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! LUAT project configuration.
//!
//! Configuration is loaded from `luat.toml` at the project root.
//!
//! # Example Configuration
//!
//! ```toml
//! [project]
//! name = "my-app"
//! version = "1.0.0"
//!
//! [dev]
//! port = 3000
//! host = "localhost"
//!
//! [build]
//! output_dir = "dist"
//! minify = true
//!
//! [routing]
//! routes_dir = "src/routes"
//! lib_dir = "src/lib"
//! static_dir = "static"
//!
//! [frontend]
//! enabled = true
//! port = 5173
//! ```

use crate::toolchain::ToolchainConfig;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Main configuration structure loaded from `luat.toml`.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Project metadata (name, version).
    pub project: ProjectConfig,
    /// Development server settings.
    #[serde(default)]
    pub dev: DevConfig,
    /// Production build settings.
    #[serde(default)]
    pub build: BuildConfig,
    /// Frontend toolchain configuration.
    #[serde(default)]
    pub frontend: ToolchainConfig,
    /// Routing configuration.
    #[serde(default)]
    pub routing: RoutingConfig,
}

/// Routing configuration for file-based routing.
#[derive(Debug, Deserialize, Clone)]
pub struct RoutingConfig {
    /// Use simplified routing (direct file-to-URL mapping).
    ///
    /// When true, uses direct file mapping instead of SvelteKit-style routing.
    #[serde(default)]
    pub simplified: bool,

    /// Directory containing route files (default: "src/routes").
    #[serde(default = "default_routes_dir")]
    pub routes_dir: String,

    /// Directory for shared Lua modules (default: "src/lib").
    #[serde(default = "default_lib_dir")]
    pub lib_dir: String,

    /// Directory for static files (default: "static").
    #[serde(default = "default_static_dir")]
    pub static_dir: String,

    /// HTML shell template path (default: "src/app.html").
    #[serde(default = "default_app_html")]
    pub app_html: String,

    /// Directory for persistent data storage like KV store (default: ".luat/data").
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_routes_dir() -> String {
    "src/routes".to_string()
}

fn default_lib_dir() -> String {
    "src/lib".to_string()
}

fn default_static_dir() -> String {
    "static".to_string()
}

fn default_app_html() -> String {
    "src/app.html".to_string()
}

fn default_data_dir() -> String {
    ".luat/data".to_string()
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            simplified: false,
            routes_dir: default_routes_dir(),
            lib_dir: default_lib_dir(),
            static_dir: default_static_dir(),
            app_html: default_app_html(),
            data_dir: default_data_dir(),
        }
    }
}

/// Project metadata configuration.
#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    /// Project name.
    pub name: String,
    /// Project version (default: "0.1.0").
    #[serde(default = "default_version")]
    pub version: String,
}

/// Development server configuration.
#[derive(Debug, Deserialize)]
pub struct DevConfig {
    /// Server port (default: 3000).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Server host (default: "127.0.0.1").
    #[serde(default = "default_host")]
    pub host: String,
    /// Templates directory (default: "templates").
    #[serde(default = "default_templates_dir")]
    pub templates_dir: String,
    /// Public assets directory (default: "public").
    #[serde(default = "default_public_dir")]
    pub public_dir: String,
}

/// Production build configuration.
#[derive(Debug, Deserialize)]
pub struct BuildConfig {
    /// Output directory for built files (default: "dist").
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    /// Bundle format: "lua" or "binary" (default: "lua").
    #[serde(default = "default_bundle_format")]
    pub bundle_format: String,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_templates_dir() -> String {
    "templates".to_string()
}

fn default_public_dir() -> String {
    "public".to_string()
}

fn default_output_dir() -> String {
    "dist".to_string()
}

fn default_bundle_format() -> String {
    "source".to_string()
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            templates_dir: default_templates_dir(),
            public_dir: default_public_dir(),
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            bundle_format: default_bundle_format(),
        }
    }
}

impl Config {
    /// Loads configuration from `luat.toml` in the current directory.
    ///
    /// If no configuration file exists, returns default configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file exists but cannot be parsed.
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Path::new("luat.toml");

        if !config_path.exists() {
            // Return default config if no config file exists
            return Ok(Config {
                project: ProjectConfig {
                    name: "unnamed".to_string(),
                    version: default_version(),
                },
                dev: DevConfig::default(),
                build: BuildConfig::default(),
                frontend: ToolchainConfig::default(),
                routing: RoutingConfig::default(),
            });
        }

        let content = fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
