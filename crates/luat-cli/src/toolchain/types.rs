// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Type definitions for the frontend toolchain module.

use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};
use thiserror::Error;

/// Represents the available frontend tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Dart Sass - CSS preprocessor
    Sass,
    /// Tailwind CSS - utility-first CSS framework
    Tailwind,
    /// esbuild - JavaScript/TypeScript bundler (labeled as TypeScript for clarity)
    TypeScript,
}

impl Tool {
    /// Returns the string identifier for this tool.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tool::Sass => "sass",
            Tool::Tailwind => "tailwind",
            Tool::TypeScript => "typescript",
        }
    }
}

impl std::str::FromStr for Tool {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sass" => Ok(Tool::Sass),
            "tailwind" | "tailwindcss" => Ok(Tool::Tailwind),
            "typescript" | "ts" | "esbuild" => Ok(Tool::TypeScript),
            _ => Err(format!("Unknown tool: {}", s)),
        }
    }
}

/// Represents the target platform for binary downloads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux x86_64
    LinuxX64,
    /// Linux ARM64/AArch64
    LinuxArm64,
    /// macOS x86_64 (Intel)
    DarwinX64,
    /// macOS ARM64 (Apple Silicon)
    DarwinArm64,
    /// Windows x86_64
    WindowsX64,
}

impl Platform {
    /// Get the current platform
    pub fn current() -> Option<Self> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Some(Platform::LinuxX64)
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            Some(Platform::LinuxArm64)
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            Some(Platform::DarwinX64)
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some(Platform::DarwinArm64)
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Some(Platform::WindowsX64)
        }
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64")
        )))]
        {
            None
        }
    }

    /// Get the platform string for the given tool
    pub fn asset_name(&self, tool: Tool, version: &str) -> String {
        match (tool, self) {
            (Tool::Sass, Platform::LinuxX64) => format!("dart-sass-{}-linux-x64.tar.gz", version),
            (Tool::Sass, Platform::LinuxArm64) => format!("dart-sass-{}-linux-arm64.tar.gz", version),
            (Tool::Sass, Platform::DarwinX64) => format!("dart-sass-{}-macos-x64.tar.gz", version),
            (Tool::Sass, Platform::DarwinArm64) => format!("dart-sass-{}-macos-arm64.tar.gz", version),
            (Tool::Sass, Platform::WindowsX64) => format!("dart-sass-{}-windows-x64.zip", version),

            (Tool::Tailwind, Platform::LinuxX64) => "tailwindcss-linux-x64".to_string(),
            (Tool::Tailwind, Platform::LinuxArm64) => "tailwindcss-linux-arm64".to_string(),
            (Tool::Tailwind, Platform::DarwinX64) => "tailwindcss-macos-x64".to_string(),
            (Tool::Tailwind, Platform::DarwinArm64) => "tailwindcss-macos-arm64".to_string(),
            (Tool::Tailwind, Platform::WindowsX64) => "tailwindcss-windows-x64.exe".to_string(),

            (Tool::TypeScript, _) => {
                let os = match self {
                    Platform::LinuxX64 | Platform::LinuxArm64 => "linux",
                    Platform::DarwinX64 | Platform::DarwinArm64 => "darwin",
                    Platform::WindowsX64 => "win32",
                };

                let arch = match self {
                    Platform::LinuxX64 | Platform::DarwinX64 | Platform::WindowsX64 => "x64",
                    Platform::LinuxArm64 | Platform::DarwinArm64 => "arm64",
                };

                format!("@esbuild/{}-{}", os, arch)
            }
        }
    }

    /// Get the executable path inside the archive
    pub fn executable_path(&self, tool: Tool) -> &'static str {
        match (tool, self) {
            (Tool::Sass, Platform::WindowsX64) => "dart-sass/sass.bat",
            (Tool::Sass, _) => "dart-sass/sass",

            (Tool::Tailwind, _) => "", // The downloaded file is the executable

            (Tool::TypeScript, Platform::WindowsX64) => "package/esbuild.exe",
            (Tool::TypeScript, _) => "package/bin/esbuild",
        }
    }
}

/// Configuration for frontend toolchain from luat.toml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolchainConfig {
    /// List of enabled tools. Valid values: "sass", "tailwind"/"tailwindcss", "typescript"/"ts"/"esbuild"
    #[serde(default = "default_enabled_tools")]
    pub enabled: Vec<String>,

    /// Sass version - specific version number or "latest" to use the most recent release
    #[serde(default = "default_sass_version")]
    pub sass_version: String,

    /// Tailwind CSS version - specific version number or "latest"
    #[serde(default = "default_tailwind_version")]
    pub tailwind_version: String,

    /// esbuild version - specific version number or "latest"
    #[serde(default = "default_esbuild_version")]
    pub esbuild_version: String,

    /// Sass input file path (relative to project root)
    #[serde(default = "default_sass_entrypoint")]
    pub sass_entrypoint: String,

    /// Sass output file path (relative to project root)
    #[serde(default = "default_sass_output")]
    pub sass_output: String,

    /// Tailwind input file path (relative to project root)
    /// If not specified, defaults to the sass_output path
    #[serde(default)]
    pub tailwind_entrypoint: Option<String>,

    /// Tailwind output file path (relative to project root)
    #[serde(default = "default_tailwind_output")]
    pub tailwind_output: String,

    /// Tailwind content glob patterns for PurgeCSS
    #[serde(default = "default_tailwind_content")]
    pub tailwind_content: Vec<String>,

    /// TypeScript/esbuild input file path (relative to project root)
    #[serde(default = "default_typescript_entrypoint")]
    pub typescript_entrypoint: String,

    /// TypeScript/esbuild output file path (relative to project root)
    #[serde(default = "default_typescript_output")]
    pub typescript_output: String,

    /// Custom build scripts to run between Tailwind and TypeScript steps
    #[serde(default)]
    pub scripts: Vec<String>,
}

impl ToolchainConfig {
    /// Returns a set of enabled tools based on the string configuration
    pub fn get_enabled_tools(&self) -> HashSet<Tool> {
        self.enabled
            .iter()
            .filter_map(|tool_str| tool_str.parse::<Tool>().ok())
            .collect()
    }

    /// Returns the tailwind entrypoint, falling back to sass_entrypoint if not explicitly set
    pub fn get_tailwind_entrypoint(&self) -> &str {
        self.tailwind_entrypoint.as_deref().unwrap_or(&self.sass_entrypoint)
    }
}

/// Build graph node status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    /// Build has not started yet.
    Pending,
    /// Build is currently running.
    InProgress,
    /// Build completed successfully.
    Completed,
    /// Build failed.
    Failed,
}

/// Path to a tool's executable.
#[derive(Debug, Clone)]
pub struct ToolPath {
    /// The tool type.
    pub tool: Tool,
    /// Version string.
    pub version: String,
    /// Path to the executable.
    pub path: PathBuf,
}

// Default version functions
fn default_sass_version() -> String {
    "latest".to_string()
}

fn default_tailwind_version() -> String {
    "latest".to_string()
}

fn default_esbuild_version() -> String {
    "latest".to_string()
}

fn default_sass_entrypoint() -> String {
    "assets/css/app.css".to_string()
}

fn default_sass_output() -> String {
    "public/css/app.css".to_string()
}

fn default_tailwind_output() -> String {
    "public/css/app.css".to_string()
}

fn default_tailwind_content() -> Vec<String> {
    vec![
        "./templates/**/*.luat".to_string(),
        "./templates/**/*.html".to_string(),
        "./assets/**/*.js".to_string(),
        "./assets/**/*.ts".to_string(),
    ]
}

fn default_typescript_entrypoint() -> String {
    "assets/js/app.ts".to_string()
}

fn default_typescript_output() -> String {
    "public/js/app.js".to_string()
}

fn default_enabled_tools() -> Vec<String> {
    vec!["tailwind".to_string()] // Tailwind enabled by default
}

/// Errors related to toolchain operations
#[derive(Debug, Error)]
pub enum ToolchainError {
    /// The current platform is not supported by the toolchain
    #[error("Unsupported platform: {0}")]
    UnsupportedPlatform(String),

    /// An error occurred during tool download
    #[error("Download failed: {0}")]
    DownloadFailed(String),

    /// An error occurred while extracting a downloaded archive
    #[error("Extraction failed: {0}")]
    ExtractionFailed(String),

    /// Failed to fetch the latest release information from GitHub or NPM
    #[error("Failed to fetch release information: {0}")]
    ReleaseFetchFailed(String),

    /// A tool execution failed (non-zero exit code or other error)
    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    /// An I/O error occurred (file not found, permission denied, etc.)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An error occurred while working with zip archives
    #[error("Zip error: {0}")]
    Zip(String),

    /// A network request error occurred
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// JSON parsing failed (usually when parsing API responses)
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type alias for toolchain operations
pub type ToolchainResult<T> = Result<T, ToolchainError>;

impl From<zip::result::ZipError> for ToolchainError {
    fn from(e: zip::result::ZipError) -> Self {
        ToolchainError::Zip(e.to_string())
    }
}
