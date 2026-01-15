// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Abstract Syntax Tree (AST) types for LUAT templates.
//!
//! This module defines the data structures that represent a parsed LUAT template.
//! The AST is produced by the parser and consumed by the transform and codegen stages.
//!
//! # Structure
//!
//! A LUAT template is represented as a [`TemplateAST`] containing:
//! - Optional module-level script (`<script context="module">`)
//! - Optional component script (`<script>`)
//! - A list of body [`Node`]s representing the template markup
//! - Discovered component imports
//!
//! # Node Types
//!
//! The [`Node`] enum represents all possible template constructs:
//! - HTML elements and components
//! - Text content and mustache expressions (`{expr}`)
//! - Control flow blocks (`{#if}`, `{#each}`)
//! - Special directives (`{@html}`, `{@local}`, `{@render}`)

use serde::{Deserialize, Serialize};

/// AST node types representing template structure.
///
/// Each variant corresponds to a different syntactic construct in LUAT templates.
/// Nodes form a tree structure where elements and control blocks can contain children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// An HTML element like `<div>`, `<span>`, etc.
    ElementNode {
        /// The tag name (e.g., "div", "span", "button").
        tag: String,
        /// Element attributes and their values.
        attributes: Vec<Attribute>,
        /// Child nodes nested within this element.
        children: Vec<Node>,
    },
    /// Plain text content between elements or expressions.
    TextNode {
        /// The text content, preserving whitespace.
        content: String,
    },
    /// A mustache expression `{expression}` that outputs escaped HTML.
    MustacheNode {
        /// The Lua expression to evaluate and render.
        expression: Expression,
    },
    /// HTML comment that may contain dynamic expressions.
    ///
    /// Comments like `<!-- {expr} -->` preserve expressions for output.
    HtmlComment {
        /// Child nodes within the comment (text and expressions).
        children: Vec<Node>,
    },
    /// LUAT-specific comment `{! comment !}` that is stripped from output.
    LuatComment,
    /// Conditional block `{#if condition}...{/if}`.
    IfBlock {
        /// The Lua expression to evaluate as a boolean.
        condition: Expression,
        /// Nodes to render when condition is truthy.
        then_branch: Vec<Node>,
        /// Optional nodes to render when condition is falsy (from `{:else}`).
        else_branch: Option<Vec<Node>>,
    },
    /// Iteration block `{#each list as item}...{/each}`.
    EachBlock {
        /// The Lua expression that evaluates to an iterable.
        list_expr: Expression,
        /// Variable name for the current item.
        item_id: String,
        /// Optional variable name for the current index.
        index_id: Option<String>,
        /// Nodes to render for each item.
        body: Vec<Node>,
        /// Optional nodes to render when the list is empty (`{:empty}`).
        empty: Option<Vec<Node>>,
    },
    /// Whitespace-sensitive conditional block `{#sif condition}...{/sif}`.
    ///
    /// Like `IfBlock` but preserves exact whitespace in output.
    SensitiveIfBlock {
        /// The Lua expression to evaluate as a boolean.
        condition: Expression,
        /// Nodes to render when condition is truthy.
        then_branch: Vec<Node>,
        /// Optional nodes to render when condition is falsy.
        else_branch: Option<Vec<Node>>,
    },
    /// Whitespace-sensitive iteration block `{#seach list as item}...{/seach}`.
    ///
    /// Like `EachBlock` but preserves exact whitespace in output.
    SensitiveEachBlock {
        /// The Lua expression that evaluates to an iterable.
        list_expr: Expression,
        /// Variable name for the current item.
        item_id: String,
        /// Optional variable name for the current index.
        index_id: Option<String>,
        /// Nodes to render for each item.
        body: Vec<Node>,
        /// Optional nodes to render when the list is empty.
        empty: Option<Vec<Node>>,
    },
    /// Local constant declaration `{@local name = expression}`.
    ///
    /// Declares a local variable available in subsequent template expressions.
    LocalConst {
        /// The variable name to bind.
        name: String,
        /// The Lua expression to evaluate and assign.
        expression: Expression,
    },
    /// Raw HTML output `{@html expression}`.
    ///
    /// Outputs the expression result without HTML escaping. Use with caution
    /// as this can introduce XSS vulnerabilities with untrusted content.
    RawHtml {
        /// The Lua expression that should return an HTML string.
        expression: Expression,
    },
    /// A component invocation like `<Button>` or `<Card>`.
    ///
    /// Components are distinguished from elements by having a capitalized name.
    ComponentNode {
        /// The component name (e.g., "Button", "Card").
        name: String,
        /// Props passed to the component.
        attributes: Vec<Attribute>,
        /// Child nodes passed as the component's children slot.
        children: Vec<Node>,
    },
    /// Children slot render directive `{@render children()}` or `{@render children?()}`.
    ///
    /// Used inside components to render passed children content.
    RenderChildren {
        /// If true, renders nothing when children is nil (uses `?()` syntax).
        optional: bool,
    },
    /// Pass-through script tag that isn't processed by LUAT.
    ///
    /// Used for `<script type="application/json">` or similar non-Lua scripts.
    ScriptAny {
        /// The full opening tag.
        tag: String,
        /// The script content.
        content: String,
    },
}

/// Attribute on an element or component.
///
/// Attributes can be named key-value pairs or spread operators that expand
/// a Lua table into individual attributes/props.
///
/// # Examples
///
/// ```text
/// <div class="container">     <!-- Named with static value -->
/// <div class="{className}">   <!-- Named with dynamic value -->
/// <div {class}>               <!-- Shorthand (name = value) -->
/// <Button {...props}>         <!-- Spread operator -->
/// <input disabled>            <!-- Boolean attribute -->
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Attribute {
    /// A named attribute like `class="foo"` or `onclick={handler}`.
    Named {
        /// The attribute name.
        name: String,
        /// The attribute value (static, dynamic, or boolean).
        value: AttributeValue,
    },
    /// A spread attribute `{...expr}` that expands a table into attributes.
    Spread(Expression),
}

/// The value portion of a named attribute.
///
/// Supports various syntaxes for static and dynamic attribute values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// A static string value: `foo="bar"`.
    Static(String),
    /// A dynamic expression value: `foo="{expr}"`.
    Dynamic(Expression),
    /// Shorthand syntax where name equals value: `{foo}` expands to `foo={foo}`.
    Shorthand(Expression),
    /// Raw HTML attribute value: `foo={@html expr}`.
    RawHtml(Expression),
    /// Boolean attribute with no value: `disabled`, `checked`.
    BooleanTrue,
}

/// A Lua expression extracted from the template.
///
/// Expressions appear in mustache tags (`{expr}`), attribute values,
/// control flow conditions, and other dynamic contexts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// The Lua expression text.
    pub content: String,
    /// Source location for error reporting.
    pub span: Span,
}

/// Source location information for error reporting and debugging.
///
/// Tracks the position of a syntax element within the source template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Byte offset from the start of the source.
    pub start: usize,
    /// Byte offset of the end (exclusive).
    pub end: usize,
    /// 1-indexed line number.
    pub line: usize,
    /// 1-indexed column number.
    pub column: usize,
}

/// Type of script block in a LUAT template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptType {
    /// Component-level script `<script>` that runs for each render.
    Regular,
    /// Module-level script `<script context="module">` that runs once when loaded.
    Module,
}

/// A script block containing Lua code.
///
/// Templates can have up to one module script and one regular script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptBlock {
    /// Whether this is a module or regular script.
    pub script_type: ScriptType,
    /// The Lua source code.
    pub content: String,
    /// Source location of the script block.
    pub span: Span,
}

/// Complete AST representation of a parsed LUAT template.
///
/// This is the root structure produced by [`crate::parser::parse_template`]
/// and consumed by the transform and codegen stages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateAST {
    /// Module-level script that runs once when the component is loaded.
    pub module_script: Option<ScriptBlock>,
    /// Component script that runs on each render.
    pub regular_script: Option<ScriptBlock>,
    /// The template body nodes (markup, expressions, control flow).
    pub body: Vec<Node>,
    /// Component paths discovered from `require()` calls in scripts.
    pub imports: Vec<String>,
    /// Canonical file path, set by the engine after resolution.
    pub path: Option<String>,
}

impl TemplateAST {
    /// Creates a new empty template AST.
    pub fn new() -> Self {
        Self {
            module_script: None,
            regular_script: None,
            body: Vec::new(),
            imports: Vec::new(),
            path: None,
        }
    }
}

impl Default for TemplateAST {
    fn default() -> Self {
        Self::new()
    }
}

/// LUAT magic function like `$state()` or `$derived()`.
///
/// Magic functions provide Svelte 5-style rune-like syntax for reactivity.
/// These are placeholders for future reactivity features.
///
/// # Examples
///
/// ```text
/// let count = $state(0)
/// let doubled = $derived(function() return count * 2 end)
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LuatMagicFunction {
    /// Function name without the `$` prefix (e.g., "state", "derived").
    pub name: String,
    /// Arguments passed to the magic function.
    pub args: Vec<Expression>,
    /// Optional default value (second argument in `$state(value, default)`).
    pub default_value: Option<Expression>,
    /// Source location for error reporting.
    pub span: Span,
}

impl LuatMagicFunction {
    /// Creates a new magic function representation.
    pub fn new(name: impl Into<String>, args: Vec<Expression>, default_value: Option<Expression>, span: Span) -> Self {
        Self {
            name: name.into(),
            args,
            default_value,
            span,
        }
    }

    /// Generates Lua-compatible code for this magic function.
    ///
    /// Currently outputs placeholder comments as magic functions are
    /// reserved for future reactivity features.
    pub fn to_lua(&self) -> String {
        // Special handling for different magic functions
        match self.name.as_str() {
            "derived" => {
                // Handle special case for function arguments which need proper Lua syntax
                let content = if self.args.is_empty() { 
                    "nil".to_string() 
                } else {
                    // Fix Lua function syntax if it's a function declaration
                    let arg_content = &self.args[0].content;
                    if arg_content.trim_start().starts_with("(function") && arg_content.contains("return") {
                        // Already wrapped in parentheses - good Lua syntax
                        arg_content.clone()
                    } else if arg_content.trim_start().starts_with("function") && arg_content.contains("return") {
                        // Need to wrap in parentheses for proper syntax
                        format!("({})", arg_content)
                    } else {
                        arg_content.clone()
                    }
                };
                
                format!(
                    "-- LUAT magic function $derived will be implemented in future\n{}",
                    content
                )
            },
            "state" => {
                match &self.default_value {
                    Some(default_val) => {
                        format!(
                            "-- LUAT magic function $state will be implemented in future\n{} or {}",
                            if self.args.is_empty() { "nil" } else { &self.args[0].content },
                            default_val.content
                        )
                    },
                    None => {
                        format!(
                            "-- LUAT magic function $state will be implemented in future\n{}",
                            if self.args.is_empty() { "nil" } else { &self.args[0].content }
                        )
                    }
                }
            },
            _ => {
                // Generic handling for other magic functions
                // The Some/None branches produce identical output - this is intentional
                // as the default value will be used in future implementations
                format!(
                    "-- LUAT magic function ${} will be implemented in future\n{}",
                    self.name,
                    if self.args.is_empty() { "nil" } else { &self.args[0].content }
                )
            }
        }
    }
}

impl Expression {
    /// Creates a new expression with the given content and source location.
    pub fn new(content: impl Into<String>, span: Span) -> Self {
        Self {
            content: content.into(),
            span,
        }
    }
}

impl Span {
    /// Creates a new source span.
    ///
    /// # Arguments
    ///
    /// * `start` - Byte offset from the beginning of the source
    /// * `end` - Byte offset of the end (exclusive)
    /// * `line` - 1-indexed line number
    /// * `column` - 1-indexed column number
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self { start, end, line, column }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_creation() {
        let mut ast = TemplateAST::new();
        
        let text_node = Node::TextNode {
            content: "Hello World".to_string(),
        };
        
        ast.body.push(text_node);
        assert_eq!(ast.body.len(), 1);
    }

    #[test]
    fn test_expression_creation() {
        let span = Span::new(0, 5, 1, 1);
        let expr = Expression::new("hello", span.clone());
        
        assert_eq!(expr.content, "hello");
        assert_eq!(expr.span, span);
    }
}
