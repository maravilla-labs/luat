// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! File watcher command for monitoring template changes.

use crate::config::Config;
use crate::watcher::FileWatcher;
use std::path::PathBuf;
use tokio::signal;

/// Runs the file watcher to monitor template changes.
pub async fn run() -> anyhow::Result<()> {
    let config = Config::load()?;
    let templates_dir = config.dev.templates_dir.clone();
    let working_dir = std::env::current_dir()?;

    println!("Watching for changes in: {}", templates_dir);
    println!("Press Ctrl+C to stop...");
    println!();

    let mut watcher = FileWatcher::new(templates_dir, working_dir, |paths: Vec<PathBuf>| {
        let files = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("  File changed: {} - rebuild triggered", files);
    })?;

    watcher.start()?;

    // Wait for Ctrl+C
    signal::ctrl_c().await?;

    println!("\nStopping file watcher...");
    Ok(())
}
