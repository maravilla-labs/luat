// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Error types for the LUAT templating engine.
//!
//! This module defines [`LuatError`], the main error enum, and helper types
//! for rich error reporting with source context.
//!
//! # Error Categories
//!
//! - **Parse errors**: Invalid template syntax
//! - **Transform errors**: AST to IR conversion failures
//! - **Codegen errors**: Lua code generation failures
//! - **Lua errors**: Runtime execution failures
//! - **Resolution errors**: Template file not found
//! - **Cache errors**: Caching operation failures
//!
//! # Source Context
//!
//! Parse errors include [`SourceContext`] for rich error messages
//! showing the problematic code with line numbers and caret pointing
//! to the exact error location.

use thiserror::Error;
use std::fmt;

/// Source context for enhanced error messages.
///
/// Captures a snippet of source code around an error location,
/// enabling rich error messages with line numbers and visual indicators.
#[derive(Debug, Clone)]
pub struct SourceContext {
    /// All lines from the source file.
    pub lines: Vec<String>,
    /// The line number where the error occurred (1-indexed).
    pub error_line: usize,
    /// The column number where the error occurred (1-indexed).
    pub error_column: usize,
    /// First line number of the snippet (1-indexed).
    pub snippet_start: usize,
    /// Last line number of the snippet (1-indexed).
    pub snippet_end: usize,
}

impl SourceContext {
    /// Creates a source context from source code and error location.
    ///
    /// Captures 3 lines before and after the error line for context.
    pub fn from_source(source: &str, line: usize, column: usize) -> Self {
        let lines: Vec<String> = source.lines().map(|l| l.to_string()).collect();
        let snippet_start = line.saturating_sub(3).max(1);
        let snippet_end = (line + 3).min(lines.len());
        
        Self {
            lines,
            error_line: line,
            error_column: column,
            snippet_start,
            snippet_end,
        }
    }
    
    /// Formats the source snippet with line numbers and error indicator.
    ///
    /// Returns a string like:
    /// ```text
    ///    4 | <div class="container">
    ///    5 |   {invalid.syntax}
    ///      |   ^
    ///    6 | </div>
    /// ```
    pub fn format_snippet(&self) -> String {
        let mut result = String::new();
        
        for line_num in self.snippet_start..=self.snippet_end {
            if line_num > self.lines.len() {
                break;
            }
            
            let line = &self.lines[line_num - 1];
            let is_error_line = line_num == self.error_line;
            
            result.push_str(&format!("{:4} | {}\n", line_num, line));
            
            if is_error_line {
                result.push_str(&format!("     | {}^\n", " ".repeat(self.error_column.saturating_sub(1))));
            }
        }
        
        result
    }
}

impl fmt::Display for SourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_snippet())
    }
}

/// Helper struct for displaying optional source context.
pub struct OptSourceContextDisplay<'a>(pub &'a Option<SourceContext>);

impl<'a> fmt::Display for OptSourceContextDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(ctx) => write!(f, "{}", ctx),
            None => write!(f, ""),
        }
    }
}

/// Helper trait for formatting optional source context.
pub trait AsDisplay<'a> {
    /// Wraps self for Display formatting.
    fn as_display(&'a self) -> OptSourceContextDisplay<'a>;
}

impl<'a> AsDisplay<'a> for Option<SourceContext> {
    fn as_display(&'a self) -> OptSourceContextDisplay<'a> {
        OptSourceContextDisplay(self)
    }
}

/// The main error type for LUAT operations.
///
/// All LUAT functions return `Result<T, LuatError>` to provide
/// detailed error information for debugging and user feedback.
#[derive(Error, Debug)]
pub enum LuatError {
    /// Template parsing failed due to invalid syntax.
    #[error("Parse error in {file:?}: {message} at line {line}, column {column}\n{}", source_context.as_display())]
    ParseError {
        /// Description of the parse error.
        message: String,
        /// Line number where the error occurred.
        line: usize,
        /// Column number where the error occurred.
        column: usize,
        /// The file path, if known.
        file: Option<String>,
        /// Source context for rich error display.
        source_context: Option<SourceContext>,
    },

    /// AST to IR transformation failed.
    #[error("Transform error: {0}")]
    TransformError(String),

    /// Lua code generation failed.
    #[error("Code generation error: {0}")]
    CodegenError(String),

    /// Lua runtime execution error.
    #[error("Lua execution error: {0}")]
    LuaError(#[from] mlua::Error),

    /// File I/O error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Template file could not be found or resolved.
    #[error("Resolution error: {0}")]
    ResolutionError(String),

    /// Cache operation failed.
    #[error("Cache error: {0}")]
    CacheError(String),

    /// A required module could not be found.
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// The template structure is invalid.
    #[error("Invalid template: {0}")]
    InvalidTemplate(String),

    /// Multiple `<script context="module">` blocks found.
    #[error("Multiple module scripts found")]
    MultipleModuleScripts,

    /// Multiple `<script>` blocks found (only one allowed).
    #[error("Multiple regular scripts found")]
    MultipleRegularScripts,

    /// Module script must appear before any template content.
    #[error("Module script must be first")]
    ModuleScriptNotFirst,

    /// Runtime error during template rendering.
    #[error("Template runtime error in {template}: {message}\n{}", source_context.as_display())]
    TemplateRuntimeError {
        /// The template where the error occurred.
        template: String,
        /// Error message.
        message: String,
        /// Lua stack trace, if available.
        lua_traceback: Option<String>,
        /// Source context for error display.
        source_context: Option<SourceContext>,
    },

    /// Error occurred while processing a module in a bundle.
    #[error("Bundle module error: {module} - {message}")]
    BundleModuleError {
        /// The module that caused the error.
        module: String,
        /// Error message.
        message: String,
        /// The underlying error.
        original_error: Box<LuatError>,
    },
}

/// Convenience type alias for Results with [`LuatError`].
pub type Result<T> = std::result::Result<T, LuatError>;
