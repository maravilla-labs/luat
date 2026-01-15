// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Frontend toolchain module for luat-cli.
//!
//! This module provides automated management of frontend build tools
//! for development and production builds. It handles downloading, caching,
//! and executing various frontend build tools like Sass, Tailwind CSS, and esbuild.

pub mod build;
mod download;
pub mod output;
pub mod types;

use console::style;
use directories::ProjectDirs;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Mutex,
};

pub use self::types::{
    Platform, Tool, ToolPath, ToolchainConfig, ToolchainError, ToolchainResult,
};

/// Cache for failed download attempts during CLI execution
pub(crate) static FAILED_DOWNLOADS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Manages tool downloads and caching
pub struct ToolchainManager {
    cache_dir: PathBuf,
}

impl ToolchainManager {
    /// Creates a new toolchain manager
    pub fn new() -> ToolchainResult<Self> {
        let cache_dir = Self::get_cache_dir()?;
        fs::create_dir_all(&cache_dir)?;

        Ok(Self { cache_dir })
    }

    /// Returns the path to the cache directory
    fn get_cache_dir() -> ToolchainResult<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "maravilla-labs", "luat").ok_or_else(|| {
            ToolchainError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine cache directory",
            ))
        })?;

        let cache_dir = proj_dirs.cache_dir().join("tools");
        Ok(cache_dir)
    }

    /// Ensures that the specified tool is available, downloading it if necessary
    pub async fn ensure_tool(&self, tool: Tool, version: &str) -> ToolchainResult<ToolPath> {
        let platform = Platform::current().ok_or_else(|| {
            ToolchainError::UnsupportedPlatform("Current platform is not supported".to_string())
        })?;

        // Check if this download has already failed during this CLI execution
        {
            let failed = FAILED_DOWNLOADS.lock().unwrap();
            let key = format!("{:?}-{}", tool, version);
            if failed.contains(&key) {
                return Err(ToolchainError::DownloadFailed(
                    "Previous download attempt failed, skipping retry".to_string(),
                ));
            }
        }

        // Check if we already have this tool cached
        if let Some(tool_path) = self.find_cached_tool(tool, version)? {
            return Ok(tool_path);
        }

        // If not cached, download it
        self.download_tool(tool, platform, version).await
    }

    /// Finds a cached tool if it exists
    fn find_cached_tool(&self, tool: Tool, version: &str) -> ToolchainResult<Option<ToolPath>> {
        let tool_dir = self.cache_dir.join(tool.as_str());
        let actual_version: String;

        let version_dir = if version == "latest" {
            // Try to resolve latest symlink
            let latest_link = tool_dir.join("latest");
            if latest_link.exists() && latest_link.is_symlink() {
                match fs::read_link(&latest_link) {
                    Ok(target) => {
                        if let Some(version_name) = target.file_name() {
                            actual_version = version_name.to_string_lossy().to_string();
                            tool_dir.join(&actual_version)
                        } else {
                            return Ok(None);
                        }
                    }
                    Err(_) => return Ok(None),
                }
            } else {
                return Ok(None);
            }
        } else {
            actual_version = version.to_string();
            tool_dir.join(version)
        };

        let platform = Platform::current().ok_or_else(|| {
            ToolchainError::UnsupportedPlatform("Current platform is not supported".to_string())
        })?;

        // Determine executable name based on platform and tool
        let exec_path = platform.executable_path(tool);
        let executable = if exec_path.is_empty() {
            // For tools like Tailwind where the downloaded file is the executable
            // The binary is saved as version_dir/{tool_name}, e.g., version_dir/tailwind
            version_dir.join(tool.as_str())
        } else {
            version_dir.join(exec_path)
        };

        if !executable.exists() {
            return Ok(None);
        }

        Ok(Some(ToolPath {
            tool,
            path: executable,
            version: actual_version,
        }))
    }

    /// Downloads a tool and caches it
    async fn download_tool(
        &self,
        tool: Tool,
        platform: Platform,
        version: &str,
    ) -> ToolchainResult<ToolPath> {
        let (actual_version, path) =
            self::download::download_and_extract(tool, platform, version, &self.cache_dir).await?;

        Ok(ToolPath {
            tool,
            version: actual_version,
            path,
        })
    }
}

/// Returns `true` if the specified tool version is already cached and available
pub async fn is_tool_cached(tool: Tool, version: &str) -> ToolchainResult<bool> {
    let manager = ToolchainManager::new()?;
    let cached_tool = manager.find_cached_tool(tool, version)?;
    Ok(cached_tool.is_some())
}

/// Returns the path to a tool executable, downloading it if necessary
pub async fn ensure_tool(tool: Tool, version: &str) -> ToolchainResult<PathBuf> {
    let manager = ToolchainManager::new()?;
    let tool_path = manager.ensure_tool(tool, version).await?;
    Ok(tool_path.path)
}

/// Returns the path to a tool executable, forcing a download even if it exists
pub async fn upgrade_tool(tool: Tool, version: &str) -> ToolchainResult<PathBuf> {
    let manager = ToolchainManager::new()?;

    let platform = Platform::current().ok_or_else(|| {
        ToolchainError::UnsupportedPlatform("Current platform is not supported".to_string())
    })?;

    // Delete existing version if it exists
    if let Ok(Some(_)) = manager.find_cached_tool(tool, version) {
        let tool_dir = manager.cache_dir.join(tool.as_str());

        if version == "latest" {
            // For "latest", remove the symlink
            let _ = std::fs::remove_file(tool_dir.join("latest"));
        } else {
            // For specific versions, remove the version directory
            let _ = std::fs::remove_dir_all(tool_dir.join(version));
        }
    }

    // Now download the tool
    let tool_path = manager.download_tool(tool, platform, version).await?;

    Ok(tool_path.path)
}

/// Prepares the build tools needed for frontend asset compilation
pub async fn prepare_build_tools(
    frontend_config: &ToolchainConfig,
    upgrade_tools: bool,
) -> ToolchainResult<HashMap<Tool, PathBuf>> {
    let enabled_tools = frontend_config.get_enabled_tools();
    if enabled_tools.is_empty() {
        return Ok(HashMap::new());
    }

    if upgrade_tools {
        println!(
            "{}",
            style("Preparing build tools (with upgrade)...").cyan()
        );
    } else {
        println!("{}", style("Preparing build tools...").cyan());
    }

    let mut tool_paths = HashMap::new();
    let mut failed_tools = Vec::new();

    // Process tools sequentially to avoid race conditions
    for tool in &enabled_tools {
        let tool_version = match *tool {
            Tool::Sass => &frontend_config.sass_version,
            Tool::Tailwind => &frontend_config.tailwind_version,
            Tool::TypeScript => &frontend_config.esbuild_version,
        };

        // Decide whether to upgrade or just ensure the tool exists
        let tool_result = if upgrade_tools {
            upgrade_tool(*tool, tool_version).await
        } else {
            ensure_tool(*tool, tool_version).await
        };

        match tool_result {
            Ok(path) => {
                let tool_name = tool.as_str();

                // Check if the tool path exists in a .cache directory
                let is_cached = path.to_string_lossy().contains("/.cache/luat/tools/")
                    || path.to_string_lossy().contains("/Library/Caches/");

                if is_cached {
                    println!(
                        "{} {} {} {}",
                        style("✓").green(),
                        style(tool_name).cyan(),
                        style(format!("v{}", tool_version)).dim(),
                        style("(cached)").dim()
                    );
                } else {
                    println!(
                        "{} {} {}",
                        style("✓").green(),
                        style(tool_name).cyan(),
                        style(format!("v{}", tool_version)).dim()
                    );
                }
                tool_paths.insert(*tool, path);
            }
            Err(e) => {
                let tool_name = tool.as_str();
                println!(
                    "{} {} {}: {}",
                    style("✗").red(),
                    style(tool_name).cyan(),
                    style(format!("v{}", tool_version)).dim(),
                    style(e.to_string()).red()
                );
                failed_tools.push(tool_name.to_string());
            }
        }
    }

    // If any tools failed to initialize, return an error
    if !failed_tools.is_empty() {
        let error_msg = format!(
            "Failed to initialize build tools: {}. Check your internet connection or try again later.",
            failed_tools.join(", ")
        );
        return Err(ToolchainError::DownloadFailed(error_msg));
    }

    Ok(tool_paths)
}
