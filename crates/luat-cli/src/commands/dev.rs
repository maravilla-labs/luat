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
    crate::commands::packages::ensure_installed(&working_dir).await?;

    // Client entries: built with hashed names, rebuilt on every change.
    let assets = if config.frontend.entries.is_empty() {
        None
    } else {
        Some(DevAssetBuilder::start(&config, &working_dir, quiet).await?)
    };

    // Fixed-path frontend tools (projects without entries)
    let enabled_tools = config.frontend.get_enabled_tools();
    if assets.is_none() && !enabled_tools.is_empty() {
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

    let watcher_assets = assets.clone();
    let mut watcher = FileWatcher::new(src_dir, working_dir.clone(), move |paths: Vec<PathBuf>| {
        let start = Instant::now();

        // Templates feed Tailwind's class scan, so any change rebuilds.
        if let Some(assets) = &watcher_assets {
            assets.rebuild();
        }

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
    // Keep path dependencies' copies in .luat/packages current.
    let _package_watchers = watch_path_packages(&working_dir, reload_tx.clone(), quiet)?;

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

    create_server(&addr, &config, reload_tx, assets.map(|a| a.manifest)).await?;

    Ok(())
}

/// Watches every path dependency's directory and re-installs (re-copies)
/// path packages when one of their `.lua`/`.luat` files changes.
fn watch_path_packages(
    working_dir: &std::path::Path,
    reload_tx: Arc<broadcast::Sender<()>>,
    quiet: bool,
) -> anyhow::Result<Vec<FileWatcher>> {
    let handle = tokio::runtime::Handle::current();
    let mut watchers = Vec::new();
    for dir in crate::commands::packages::path_dependency_dirs(working_dir) {
        let root = working_dir.to_path_buf();
        let tx = reload_tx.clone();
        let handle = handle.clone();
        let watcher = FileWatcher::new(dir.to_string_lossy().into_owned(), dir.clone(), move |paths| {
            let project = luat::packages::Project::new(&root);
            match handle.block_on(project.install(false)) {
                Ok(_) => {
                    if !quiet {
                        println!("  {} re-installed path packages ({} changed)", style("✓").green(), paths.len());
                    }
                    let _ = tx.send(());
                }
                Err(e) => eprintln!("  {} {}", style("✗").red(), style(format!("package install failed: {e}")).red()),
            }
        })?;
        watchers.push(watcher);
    }
    Ok(watchers)
}

/// Builds `[frontend] entries` for the dev server and keeps the result
/// current.
#[derive(Clone)]
struct DevAssetBuilder {
    options: luat::assets::AssetOptions,
    manifest: crate::server::http::DevAssets,
}

impl DevAssetBuilder {
    async fn start(config: &Config, working_dir: &std::path::Path, quiet: bool) -> anyhow::Result<Self> {
        let (esbuild, tailwind) = crate::toolchain::asset_tools(&config.frontend, working_dir).await?;
        let options = luat::assets::AssetOptions {
            project_dir: working_dir.to_path_buf(),
            entries: config.frontend.entries.clone(),
            out_dir: crate::server::http::dev_assets_dir(working_dir),
            esbuild,
            tailwind,
            production: false,
        };
        let start = Instant::now();
        let manifest = luat::assets::build_assets(&options)?;
        if !quiet {
            println!(
                "  {} {} {}",
                style("✓").green(),
                style(format!("Client assets ({} entries)", manifest.entries().len())).dim(),
                style(format!("{}ms", start.elapsed().as_millis())).dim()
            );
        }
        Ok(Self {
            options,
            manifest: Arc::new(std::sync::RwLock::new(manifest)),
        })
    }

    /// Rebuilds; on failure the previous build stays in place.
    fn rebuild(&self) {
        match luat::assets::build_assets(&self.options) {
            Ok(manifest) => {
                if let Ok(mut current) = self.manifest.write() {
                    *current = manifest;
                }
            }
            Err(e) => eprintln!("  {} {}", style("✗").red(), style(format!("Client assets: {e}")).red()),
        }
    }
}
