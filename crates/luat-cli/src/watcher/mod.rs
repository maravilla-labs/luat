// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! File system watching for hot reload.
//!
//! This module provides `FileWatcher` for monitoring file changes
//! and triggering rebuilds or live reloads.
//!
//! # Features
//!
//! - Debounced file change events (750ms)
//! - Filters for relevant file types (.luat, .lua)
//! - Recursive directory watching

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, Debouncer, FileIdMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Watches filesystem for changes to template files.
///
/// Uses debouncing to prevent multiple rapid rebuilds and filters
/// events to only trigger on relevant file types.
pub struct FileWatcher {
    #[allow(dead_code)]
    debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    #[allow(dead_code)]
    rx: mpsc::Receiver<Result<Vec<notify_debouncer_full::DebouncedEvent>, Vec<notify::Error>>>,
}

impl FileWatcher {
    /// Creates a new file watcher for the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - Directory path to watch recursively
    /// * `base_path` - Base path for computing relative paths
    /// * `on_change` - Callback invoked when relevant files change, receives relative paths
    ///
    /// # File Types
    ///
    /// Only `.luat` and `.lua` files trigger the callback.
    pub fn new<F>(path: String, base_path: PathBuf, on_change: F) -> anyhow::Result<Self>
    where
        F: Fn(Vec<PathBuf>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();

        let mut debouncer = new_debouncer(
            Duration::from_millis(750),
            None,
            move |result: Result<Vec<notify_debouncer_full::DebouncedEvent>, Vec<notify::Error>>| {
                if let Ok(events) = &result {
                    // Collect changed paths with relevant extensions
                    let changed_paths: Vec<PathBuf> = events
                        .iter()
                        .flat_map(|e| e.paths.iter())
                        .filter(|p| {
                            let ext = p.extension().and_then(|e| e.to_str());
                            matches!(ext, Some("luat") | Some("lua"))
                        })
                        .map(|p| p.strip_prefix(&base_path).unwrap_or(p).to_path_buf())
                        .collect();

                    if !changed_paths.is_empty() {
                        on_change(changed_paths);
                    }
                }
                let _ = tx.send(result);
            },
        )?;

        // In newer versions, Debouncer implements Watcher trait directly
        debouncer.watch(Path::new(&path), RecursiveMode::Recursive)?;

        Ok(Self { debouncer, rx })
    }

    /// Starts the file watcher (no-op as watcher runs after construction).
    pub fn start(&mut self) -> anyhow::Result<()> {
        // The watcher is already running after new()
        // This method exists for API consistency
        Ok(())
    }
}
