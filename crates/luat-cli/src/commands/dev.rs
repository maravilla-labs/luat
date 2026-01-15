// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Development server command with hot reload support.

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::config::Config;
use crate::server::http::create_server;
use crate::toolchain::{build::BuildOrchestrator, prepare_build_tools, Tool};
use crate::watcher::FileWatcher;

/// Runs the development server with hot reload.
pub async fn run(host: &str, port: u16, verbose: bool, quiet: bool) -> anyhow::Result<()> {
    let config = Config::load()?;
    let working_dir = std::env::current_dir()?;

    // Prepare frontend build tools if any are enabled
    let enabled_tools = config.frontend.get_enabled_tools();
    if !enabled_tools.is_empty() {
        if !quiet {
            let tools_list: Vec<_> = enabled_tools.iter().map(|t| t.as_str()).collect();
            println!(
                "{} {}",
                style("Frontend tools:").cyan(),
                style(tools_list.join(", ")).dim()
            );
        }

        // Download/ensure tools are available
        let tool_paths = prepare_build_tools(&config.frontend, false).await?;

        // Create build orchestrator for initial build
        let mut orchestrator =
            BuildOrchestrator::new(config.frontend.clone(), working_dir.clone(), false, false)
                .with_verbose(verbose);

        // Register tools with orchestrator
        for (tool, path) in &tool_paths {
            orchestrator.register_tool(*tool, path.clone());
        }

        // Run initial build with timing
        let start = Instant::now();
        if let Err(e) = orchestrator.build_all().await {
            eprintln!(
                "  {} {}",
                style("✗").red(),
                style(format!("Initial build failed: {}", e)).red()
            );
        } else if !quiet {
            println!(
                "  {} {} {}",
                style("✓").green(),
                style("Initial build").dim(),
                style(format!("{}ms", start.elapsed().as_millis())).dim()
            );
        }

        // Start watch-mode builds in background
        let mut watch_orchestrator =
            BuildOrchestrator::new(config.frontend.clone(), working_dir.clone(), false, true)
                .with_verbose(verbose);

        for (tool, path) in &tool_paths {
            watch_orchestrator.register_tool(*tool, path.clone());
        }

        // Start the watchers for tools that support it (Tailwind, esbuild)
        if enabled_tools.contains(&Tool::Tailwind) || enabled_tools.contains(&Tool::TypeScript) {
            tokio::spawn(async move {
                if let Err(e) = watch_orchestrator.build_all().await {
                    eprintln!(
                        "  {} {}",
                        style("✗").red(),
                        style(format!("Watch mode failed: {}", e)).red()
                    );
                }
            });
        }

        if !quiet {
            println!();
        }
    }

    // Create a broadcast channel for live reload notifications
    let (reload_tx, _) = broadcast::channel::<()>(16);
    let reload_tx = Arc::new(reload_tx);

    // Check which tools are enabled for the spinner label
    let has_tailwind = config.frontend.get_enabled_tools().contains(&Tool::Tailwind);
    let tool_label = if has_tailwind { "luat+css" } else { "luat" };

    let quiet_watcher = quiet;

    // Start file watcher - watch entire src directory for .luat and .lua changes
    let watcher_tx = reload_tx.clone();
    let src_dir = "src".to_string();
    let tool_label = tool_label.to_string();

    let mut watcher = FileWatcher::new(src_dir, working_dir.clone(), move |paths: Vec<PathBuf>| {
        let start = Instant::now();

        // Send reload signal immediately
        let _ = watcher_tx.send(());

        // Show reload notification unless quiet
        if !quiet_watcher {
            // Format file paths for display
            let display = paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");

            // Create spinner
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!("  {{spinner:.cyan}} {} {{msg}}", tool_label))
                    .unwrap(),
            );
            pb.set_message(display.clone());
            pb.enable_steady_tick(Duration::from_millis(80));

            // Keep spinner for minimum 400ms
            let elapsed = start.elapsed();
            if elapsed < Duration::from_millis(400) {
                std::thread::sleep(Duration::from_millis(400) - elapsed);
            }

            // Show completion with timing
            let total_ms = start.elapsed().as_millis();
            pb.finish_with_message(format!(
                "{} {} {}",
                style("✓").green(),
                style(&display).dim(),
                style(format!("{}ms", total_ms)).dim()
            ));
        }
    })?;

    watcher.start()?;

    // Start HTTP server
    let addr = format!("{}:{}", host, port);
    if !quiet {
        println!(
            "{} {}",
            style("Server:").cyan(),
            style(format!("http://{}", addr)).green().bold()
        );
        println!(
            "{} {}",
            style("Status:").cyan(),
            style("Watching for changes...").dim()
        );
        println!();
    }

    create_server(&addr, &config, reload_tx).await?;

    Ok(())
}
