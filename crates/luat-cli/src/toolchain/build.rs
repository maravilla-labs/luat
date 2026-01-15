// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Build orchestration for frontend assets.
//!
//! This module is responsible for orchestrating the execution of frontend tools
//! in the correct order, with the appropriate arguments, and with proper dependency tracking.

use super::output::{spawn_stderr_filter, spawn_stdout_reader, OutputFilter};
use super::types::{BuildStatus, Tool, ToolchainConfig, ToolchainError, ToolchainResult};
use console::style;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Instant,
};
use tokio::process::Command as TokioCommand;
use tokio::sync::RwLock;

/// Manages the build pipeline for frontend assets
#[derive(Clone)]
pub struct BuildOrchestrator {
    /// Configuration for the build
    config: ToolchainConfig,

    /// Paths to tool executables
    tools: HashMap<Tool, PathBuf>,

    /// Working directory
    working_dir: PathBuf,

    /// Build graph status
    build_status: Arc<RwLock<HashMap<Tool, BuildStatus>>>,

    /// Flag to indicate if this is a production build
    is_production: bool,

    /// Flag to indicate if this is a build for watch mode
    is_watch_mode: bool,

    /// Flag to enable verbose output (show all tool output)
    verbose: bool,
}

impl BuildOrchestrator {
    /// Creates a new build orchestrator
    pub fn new(
        config: ToolchainConfig,
        working_dir: PathBuf,
        is_production: bool,
        is_watch_mode: bool,
    ) -> Self {
        Self {
            config,
            tools: HashMap::new(),
            working_dir,
            build_status: Arc::new(RwLock::new(HashMap::new())),
            is_production,
            is_watch_mode,
            verbose: false,
        }
    }

    /// Sets verbose mode (show all tool output without filtering)
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Registers a tool with the orchestrator
    pub fn register_tool(&mut self, tool: Tool, path: PathBuf) {
        self.tools.insert(tool, path);
    }

    /// Run the full build pipeline for all enabled tools
    pub async fn build_all(&mut self) -> ToolchainResult<()> {
        // Reset build status
        let mut status = self.build_status.write().await;
        for tool in self.config.get_enabled_tools() {
            status.insert(tool, BuildStatus::Pending);
        }
        drop(status);

        // Track total build time
        let total_start = Instant::now();

        // Build sass first if enabled
        if self.config.get_enabled_tools().contains(&Tool::Sass) {
            self.update_status(Tool::Sass, BuildStatus::InProgress).await;
            let start = Instant::now();

            match self.build_sass().await {
                Ok(_) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  {:<12} {} {}",
                        style("Sass").cyan(),
                        style("✓").green(),
                        style(format!("{}ms", elapsed.as_millis())).dim()
                    );
                    self.update_status(Tool::Sass, BuildStatus::Completed).await;
                }
                Err(err) => {
                    println!(
                        "  {:<12} {} {}",
                        style("Sass").cyan(),
                        style("✗").red(),
                        style(err.to_string()).red()
                    );
                    self.update_status(Tool::Sass, BuildStatus::Failed).await;
                    return Err(err);
                }
            }
        }

        // Build tailwind if enabled
        if self.config.get_enabled_tools().contains(&Tool::Tailwind) {
            self.update_status(Tool::Tailwind, BuildStatus::InProgress)
                .await;
            let start = Instant::now();

            match self.build_tailwind().await {
                Ok(_) => {
                    let elapsed = start.elapsed();
                    // In watch mode, timing is less relevant since it returns immediately
                    if self.is_watch_mode {
                        // Output is handled by the filtered output task
                    } else {
                        println!(
                            "  {:<12} {} {}",
                            style("Tailwind").cyan(),
                            style("✓").green(),
                            style(format!("{}ms", elapsed.as_millis())).dim()
                        );
                    }
                    self.update_status(Tool::Tailwind, BuildStatus::Completed)
                        .await;
                }
                Err(err) => {
                    println!(
                        "  {:<12} {} {}",
                        style("Tailwind").cyan(),
                        style("✗").red(),
                        style(err.to_string()).red()
                    );
                    self.update_status(Tool::Tailwind, BuildStatus::Failed).await;
                    return Err(err);
                }
            }
        }

        // Run custom scripts if any
        if !self.config.scripts.is_empty() {
            let start = Instant::now();

            match self.run_custom_scripts().await {
                Ok(_) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  {:<12} {} {} {}",
                        style("Scripts").cyan(),
                        style("✓").green(),
                        style(format!("{} scripts", self.config.scripts.len())).dim(),
                        style(format!("{}ms", elapsed.as_millis())).dim()
                    );
                }
                Err(err) => {
                    println!(
                        "  {:<12} {} {}",
                        style("Scripts").cyan(),
                        style("✗").red(),
                        style(err.to_string()).red()
                    );
                    return Err(err);
                }
            }
        }

        // Build typescript if enabled
        if self.config.get_enabled_tools().contains(&Tool::TypeScript) {
            self.update_status(Tool::TypeScript, BuildStatus::InProgress)
                .await;
            let start = Instant::now();

            match self.build_typescript().await {
                Ok(_) => {
                    let elapsed = start.elapsed();
                    // In watch mode, timing is less relevant since it returns immediately
                    if self.is_watch_mode {
                        // Output is handled by the filtered output task
                    } else {
                        println!(
                            "  {:<12} {} {}",
                            style("TypeScript").cyan(),
                            style("✓").green(),
                            style(format!("{}ms", elapsed.as_millis())).dim()
                        );
                    }
                    self.update_status(Tool::TypeScript, BuildStatus::Completed)
                        .await;
                }
                Err(err) => {
                    println!(
                        "  {:<12} {} {}",
                        style("TypeScript").cyan(),
                        style("✗").red(),
                        style(err.to_string()).red()
                    );
                    self.update_status(Tool::TypeScript, BuildStatus::Failed)
                        .await;
                    return Err(err);
                }
            }
        }

        // Show total time for non-watch builds
        if !self.is_watch_mode {
            let total_elapsed = total_start.elapsed();
            tracing::debug!("Total frontend build time: {:?}", total_elapsed);
        }

        Ok(())
    }

    /// Determine which tools need to be run based on file path
    pub fn get_affected_tools(&self, file_path: &Path) -> HashSet<Tool> {
        let path_str = file_path.to_string_lossy();
        let mut tools = HashSet::new();
        let enabled = self.config.get_enabled_tools();

        // Check if this is a Sass file
        if (path_str.ends_with(".scss") || path_str.ends_with(".sass"))
            && enabled.contains(&Tool::Sass)
        {
            tools.insert(Tool::Sass);

            // If Tailwind is enabled, it also needs to run after Sass
            if enabled.contains(&Tool::Tailwind) {
                tools.insert(Tool::Tailwind);
            }
        }

        // Check if this is a CSS file that might affect Tailwind
        if path_str.ends_with(".css") && enabled.contains(&Tool::Tailwind) {
            tools.insert(Tool::Tailwind);
        }

        // Check if this is a TypeScript/JavaScript file
        if (path_str.ends_with(".ts")
            || path_str.ends_with(".js")
            || path_str.ends_with(".tsx")
            || path_str.ends_with(".jsx"))
            && enabled.contains(&Tool::TypeScript)
        {
            tools.insert(Tool::TypeScript);
        }

        // Check if this is an HTML or template file that might affect Tailwind
        if (path_str.ends_with(".html")
            || path_str.ends_with(".htm")
            || path_str.ends_with(".lua")
            || path_str.ends_with(".luat"))
            && enabled.contains(&Tool::Tailwind)
        {
            tools.insert(Tool::Tailwind);
        }

        tools
    }

    /// Build one or more specific tools in the correct order
    pub async fn build_tools(&mut self, tools: HashSet<Tool>) -> ToolchainResult<()> {
        let mut ordered_tools = Vec::new();

        // Enforce the correct build order
        if tools.contains(&Tool::Sass) {
            ordered_tools.push(Tool::Sass);
        }

        if tools.contains(&Tool::Tailwind) {
            ordered_tools.push(Tool::Tailwind);
        }

        if tools.contains(&Tool::TypeScript) {
            ordered_tools.push(Tool::TypeScript);
        }

        // Build each tool in order
        for tool in ordered_tools {
            match tool {
                Tool::Sass => {
                    self.update_status(Tool::Sass, BuildStatus::InProgress).await;
                    match self.build_sass().await {
                        Ok(_) => self.update_status(Tool::Sass, BuildStatus::Completed).await,
                        Err(err) => {
                            self.update_status(Tool::Sass, BuildStatus::Failed).await;
                            return Err(err);
                        }
                    }
                }

                Tool::Tailwind => {
                    self.update_status(Tool::Tailwind, BuildStatus::InProgress)
                        .await;
                    match self.build_tailwind().await {
                        Ok(_) => {
                            self.update_status(Tool::Tailwind, BuildStatus::Completed)
                                .await
                        }
                        Err(err) => {
                            self.update_status(Tool::Tailwind, BuildStatus::Failed).await;
                            return Err(err);
                        }
                    }
                }

                Tool::TypeScript => {
                    // Run custom scripts before TypeScript
                    if !self.config.scripts.is_empty() {
                        self.run_custom_scripts().await?;
                    }

                    self.update_status(Tool::TypeScript, BuildStatus::InProgress)
                        .await;
                    match self.build_typescript().await {
                        Ok(_) => {
                            self.update_status(Tool::TypeScript, BuildStatus::Completed)
                                .await
                        }
                        Err(err) => {
                            self.update_status(Tool::TypeScript, BuildStatus::Failed).await;
                            return Err(err);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Build Sass files
    async fn build_sass(&self) -> ToolchainResult<()> {
        if let Some(sass_path) = self.tools.get(&Tool::Sass) {
            let input_path = self.working_dir.join(&self.config.sass_entrypoint);
            let output_path = self.working_dir.join(&self.config.sass_output);

            // Create output directory if it doesn't exist
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Build command
            let mut cmd = Command::new(sass_path);
            cmd.arg(&input_path)
                .arg(&output_path)
                .current_dir(&self.working_dir);

            // Add production flags if needed
            if self.is_production {
                cmd.arg("--no-source-map").arg("--style=compressed");
            }

            // Execute command
            let output = cmd.output()?;

            if !output.status.success() {
                return Err(ToolchainError::ExecutionFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }

            // If successful but there are warnings, print them
            if !output.stderr.is_empty() {
                eprintln!("{}", style("Sass warnings:").yellow());
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
        }

        Ok(())
    }

    /// Build Tailwind CSS
    async fn build_tailwind(&self) -> ToolchainResult<()> {
        if let Some(tailwind_path) = self.tools.get(&Tool::Tailwind) {
            let input_path = self.working_dir.join(self.config.get_tailwind_entrypoint());
            let output_path = self.working_dir.join(&self.config.tailwind_output);

            // Create output directory if it doesn't exist
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if self.is_watch_mode {
                // Use async command with filtered output for watch mode
                let mut cmd = TokioCommand::new(tailwind_path);
                cmd.arg("-i")
                    .arg(&input_path)
                    .arg("-o")
                    .arg(&output_path)
                    .arg("--watch")
                    .current_dir(&self.working_dir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                // Add content paths
                for content_path in &self.config.tailwind_content {
                    cmd.arg("--content").arg(content_path);
                }

                let mut child = cmd.spawn()?;

                // Spawn filtered output readers
                // Suppress watch rebuilds since luat shows combined luat+css output
                if let Some(stderr) = child.stderr.take() {
                    let filter = OutputFilter::new("Tailwind", self.verbose)
                        .with_suppress_watch_rebuilds(true);
                    spawn_stderr_filter(stderr, filter);
                }
                if let Some(stdout) = child.stdout.take() {
                    spawn_stdout_reader(stdout, "Tailwind".to_string(), self.verbose);
                }

                println!(
                    "  {} {} {}",
                    style("Tailwind").cyan(),
                    style("watching").dim(),
                    style("(filtered output)").dim()
                );

                // Don't wait for it to complete - it will run in the background
                return Ok(());
            }

            // Non-watch mode: use synchronous command
            let mut cmd = std::process::Command::new(tailwind_path);
            cmd.arg("-i")
                .arg(&input_path)
                .arg("-o")
                .arg(&output_path)
                .current_dir(&self.working_dir);

            // Add content paths
            for content_path in &self.config.tailwind_content {
                cmd.arg("--content").arg(content_path);
            }

            // Add production flags if needed
            if self.is_production {
                cmd.arg("--minify");
            }

            // Execute command
            let start = Instant::now();
            let output = cmd.output()?;
            let elapsed = start.elapsed();

            if !output.status.success() {
                return Err(ToolchainError::ExecutionFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }

            // If successful but there are warnings, print them
            if !output.stderr.is_empty() && self.verbose {
                eprintln!("{}", style("Tailwind warnings:").yellow());
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }

            // Store timing for structured output (Phase 4)
            tracing::debug!("Tailwind build completed in {:?}", elapsed);
        } else {
            println!("{}", style("Tailwind CSS tool not found.").yellow());
        }

        Ok(())
    }

    /// Run custom scripts
    async fn run_custom_scripts(&self) -> ToolchainResult<()> {
        for script in &self.config.scripts {
            let status = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(["/C", script])
                    .current_dir(&self.working_dir)
                    .status()?
            } else {
                Command::new("sh")
                    .args(["-c", script])
                    .current_dir(&self.working_dir)
                    .status()?
            };

            if !status.success() {
                return Err(ToolchainError::ExecutionFailed(format!(
                    "Script '{}' failed with exit code: {:?}",
                    script,
                    status.code()
                )));
            }
        }

        Ok(())
    }

    /// Build TypeScript files
    async fn build_typescript(&mut self) -> ToolchainResult<()> {
        if let Some(esbuild_path) = self.tools.get(&Tool::TypeScript) {
            let input_path = self.working_dir.join(&self.config.typescript_entrypoint);
            let output_path = self.working_dir.join(&self.config.typescript_output);

            // Create output directory if it doesn't exist
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            if self.is_watch_mode {
                // Use async command with filtered output for watch mode
                let mut cmd = TokioCommand::new(esbuild_path);
                cmd.arg(&input_path)
                    .arg(format!("--outfile={}", output_path.to_string_lossy()))
                    .arg("--bundle")
                    .arg("--loader:.woff=file")
                    .arg("--loader:.woff2=file")
                    .arg("--loader:.ttf=file")
                    .arg("--loader:.svg=file")
                    .arg("--loader:.png=file")
                    .arg("--loader:.jpg=file")
                    .arg("--loader:.gif=file")
                    .arg("--sourcemap=inline")
                    .arg("--watch")
                    .current_dir(&self.working_dir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                let mut child = cmd.spawn()?;

                // Spawn filtered output readers
                if let Some(stderr) = child.stderr.take() {
                    let filter = OutputFilter::new("esbuild", self.verbose);
                    spawn_stderr_filter(stderr, filter);
                }
                if let Some(stdout) = child.stdout.take() {
                    spawn_stdout_reader(stdout, "esbuild".to_string(), self.verbose);
                }

                println!(
                    "  {} {} {}",
                    style("esbuild").cyan(),
                    style("watching").dim(),
                    style("(filtered output)").dim()
                );

                // Don't wait for it to complete - it will run in the background
                return Ok(());
            }

            // Non-watch mode: use synchronous command
            let mut cmd = std::process::Command::new(esbuild_path);
            cmd.arg(&input_path)
                .arg(format!("--outfile={}", output_path.to_string_lossy()))
                .arg("--bundle")
                .arg("--loader:.woff=file")
                .arg("--loader:.woff2=file")
                .arg("--loader:.ttf=file")
                .arg("--loader:.svg=file")
                .arg("--loader:.png=file")
                .arg("--loader:.jpg=file")
                .arg("--loader:.gif=file")
                .current_dir(&self.working_dir);

            // Add production flags if needed
            if self.is_production {
                cmd.arg("--minify").arg("--sourcemap=external");
            } else {
                cmd.arg("--sourcemap=inline");
            }

            // Execute command
            let start = Instant::now();
            let output = cmd.output()?;
            let elapsed = start.elapsed();

            if !output.status.success() {
                return Err(ToolchainError::ExecutionFailed(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }

            // If successful but there are warnings, print them
            if !output.stderr.is_empty() && self.verbose {
                eprintln!("{}", style("esbuild warnings:").yellow());
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }

            // Store timing for structured output (Phase 4)
            tracing::debug!("esbuild build completed in {:?}", elapsed);
        }

        Ok(())
    }

    /// Update the build status of a tool
    async fn update_status(&self, tool: Tool, status: BuildStatus) {
        let mut status_map = self.build_status.write().await;
        status_map.insert(tool, status);
    }
}
