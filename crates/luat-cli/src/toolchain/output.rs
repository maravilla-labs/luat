// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Output filtering and formatting for external build tools.
//!
//! This module captures and filters output from external tools (Tailwind, esbuild)
//! to provide a cleaner developer experience during watch mode.

use console::style;
use regex::Regex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStderr;

/// Output filter for external tool processes.
///
/// Filters redundant messages like repeated "Done in Xms" during watch mode
/// while preserving important warnings and errors.
pub struct OutputFilter {
    /// Tool name for prefixing messages
    tool_name: String,
    /// Whether this is the first build (show more info)
    first_build: Arc<AtomicBool>,
    /// Regex for matching "Done in Xms" messages
    done_pattern: Regex,
    /// Regex for matching timing values (to check if > threshold)
    timing_pattern: Regex,
    /// Minimum time in ms to show rebuild messages
    timing_threshold_ms: u64,
    /// Whether verbose mode is enabled
    verbose: bool,
    /// Suppress all non-error output after first build (for combined luat+css display)
    suppress_watch_rebuilds: bool,
}

impl OutputFilter {
    /// Creates a new output filter for the given tool.
    pub fn new(tool_name: &str, verbose: bool) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            first_build: Arc::new(AtomicBool::new(true)),
            done_pattern: Regex::new(r"(?i)done in \d+").unwrap(),
            timing_pattern: Regex::new(r"(\d+)\s*(ms|µs|μs|s)\b").unwrap(),
            timing_threshold_ms: 100,
            verbose,
            suppress_watch_rebuilds: false,
        }
    }

    /// Enable suppression of all non-error output after first build.
    /// Use when luat shows combined output (luat+css).
    pub fn with_suppress_watch_rebuilds(mut self, suppress: bool) -> Self {
        self.suppress_watch_rebuilds = suppress;
        self
    }

    /// Determines if a line should be suppressed (not shown to user).
    pub fn should_suppress(&self, line: &str) -> bool {
        // Never suppress errors or warnings
        let line_lower = line.to_lowercase();
        if line_lower.contains("error")
            || line_lower.contains("warning")
            || line_lower.contains("warn:")
            || line_lower.contains("err:")
            || line_lower.contains("failed")
        {
            return false;
        }

        // Don't suppress first build output
        if self.first_build.load(Ordering::Relaxed) {
            return false;
        }

        // If suppress_watch_rebuilds is enabled, suppress all non-error output after first build
        if self.suppress_watch_rebuilds {
            return true;
        }

        // Never suppress in verbose mode (but after first_build and suppress_watch_rebuilds checks)
        if self.verbose {
            return false;
        }

        // Suppress repeated "Done in Xms" messages unless time > threshold
        if self.done_pattern.is_match(line) {
            if let Some(ms) = self.extract_timing_ms(line) {
                return ms < self.timing_threshold_ms;
            }
            return true;
        }

        // Suppress "[watch] build finished" messages
        if line.contains("[watch] build finished") {
            return true;
        }

        // Suppress version announcements on rebuilds
        if line.contains("tailwindcss v") || line.starts_with("≈") {
            return true;
        }

        // Suppress empty lines
        if line.trim().is_empty() {
            return true;
        }

        false
    }

    /// Extracts timing in milliseconds from a line like "Done in 67ms" or "Done in 491µs"
    fn extract_timing_ms(&self, line: &str) -> Option<u64> {
        if let Some(caps) = self.timing_pattern.captures(line) {
            let value: u64 = caps.get(1)?.as_str().parse().ok()?;
            let unit = caps.get(2)?.as_str();

            let ms = match unit {
                "s" => value * 1000,
                "ms" => value,
                "µs" | "μs" => value / 1000, // microseconds to ms
                _ => value,
            };

            return Some(ms);
        }
        None
    }

    /// Formats a line for display with optional tool prefix.
    pub fn format_line(&self, line: &str) -> String {
        let line = line.trim();

        // Format errors in red
        let line_lower = line.to_lowercase();
        if line_lower.contains("error") || line_lower.contains("failed") {
            return format!(
                "  {} {}",
                style(&self.tool_name).cyan(),
                style(line).red()
            );
        }

        // Format warnings in yellow
        if line_lower.contains("warning") || line_lower.contains("warn:") {
            return format!(
                "  {} {}",
                style(&self.tool_name).cyan(),
                style(line).yellow()
            );
        }

        // Format success/timing messages
        if self.done_pattern.is_match(line) {
            if let Some(ms) = self.extract_timing_ms(line) {
                return format!(
                    "  {} {} {}",
                    style(&self.tool_name).cyan(),
                    style("✓").green(),
                    style(format!("{}ms", ms)).dim()
                );
            }
        }

        // Default: prefix with tool name
        format!("  {} {}", style(&self.tool_name).cyan(), style(line).dim())
    }

    /// Marks the first build as complete (subsequent builds will have filtered output).
    pub fn mark_first_build_complete(&self) {
        self.first_build.store(false, Ordering::Relaxed);
    }

    /// Returns whether this is still the first build.
    pub fn is_first_build(&self) -> bool {
        self.first_build.load(Ordering::Relaxed)
    }

    /// Gets a clone of the first_build flag for sharing across tasks.
    pub fn first_build_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.first_build)
    }
}

/// Spawns a task to read and filter stderr from a child process.
///
/// This function reads lines from the child's stderr, filters redundant messages,
/// and prints formatted output to the console.
pub fn spawn_stderr_filter(
    stderr: ChildStderr,
    filter: OutputFilter,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !filter.should_suppress(&line) {
                let formatted = filter.format_line(&line);
                println!("{}", formatted);
            }

            // After first "Done in" message, mark first build complete
            if filter.is_first_build() && filter.done_pattern.is_match(&line) {
                filter.mark_first_build_complete();
            }
        }
    })
}

/// Spawns a task to read stdout (usually less important) and optionally display it.
pub fn spawn_stdout_reader(
    stdout: tokio::process::ChildStdout,
    tool_name: String,
    verbose: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if verbose {
                println!("  {} {}", style(&tool_name).cyan(), style(line).dim());
            }
            // In non-verbose mode, stdout is mostly ignored (stderr has the important info)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_timing_ms() {
        let filter = OutputFilter::new("test", false);

        assert_eq!(filter.extract_timing_ms("Done in 67ms"), Some(67));
        assert_eq!(filter.extract_timing_ms("Done in 491µs"), Some(0)); // rounds down
        assert_eq!(filter.extract_timing_ms("Done in 2s"), Some(2000));
        assert_eq!(filter.extract_timing_ms("⚡ Done in 24ms"), Some(24));
    }

    #[test]
    fn test_should_suppress() {
        let filter = OutputFilter::new("test", false);
        filter.mark_first_build_complete();

        // Should suppress
        assert!(filter.should_suppress("Done in 50ms"));
        assert!(filter.should_suppress("[watch] build finished"));
        assert!(filter.should_suppress("≈ tailwindcss v4.0.5"));
        assert!(filter.should_suppress(""));

        // Should NOT suppress
        assert!(!filter.should_suppress("Error: something failed"));
        assert!(!filter.should_suppress("Warning: deprecated feature"));
        assert!(!filter.should_suppress("Done in 150ms")); // > threshold
    }

    #[test]
    fn test_verbose_mode_shows_all() {
        let filter = OutputFilter::new("test", true);
        filter.mark_first_build_complete();

        // In verbose mode, nothing should be suppressed
        assert!(!filter.should_suppress("Done in 50ms"));
        assert!(!filter.should_suppress("[watch] build finished"));
    }
}
