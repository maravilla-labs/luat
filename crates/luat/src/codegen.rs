// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Lua code generation from IR.
//!
//! This module generates executable Lua code from the intermediate
//! representation ([`IR`]) produced by the transform stage.
//!
//! # Generated Code Structure
//!
//! The generated Lua module has this structure:
//!
//! ```lua
//! -- Helper functions (escape, write, etc.)
//! -- Module script (if present)
//!
//! local function render(props, runtime)
//!     -- Regular script (if present)
//!     -- Template body
//!     return table.concat(__output)
//! end
//!
//! exports.render = render
//! exports.moduleName = "ComponentName"
//! return exports
//! ```
//!
//! # Features
//!
//! - HTML escaping for security
//! - Context API (setContext/getContext)
//! - Component rendering with children
//! - Control flow (if/each) with sensitivity options

use crate::ast::*;
use crate::error::Result;
use crate::transform::*;
use std::collections::BTreeMap;

/// Source map that maps Lua line numbers to original .luat source lines.
#[derive(Debug, Clone, Default)]
pub struct LuaSourceMap {
    /// Maps Lua output line number -> .luat source line number.
    /// Only significant lines are recorded (those with expressions).
    mappings: BTreeMap<usize, usize>,
}

impl LuaSourceMap {
    /// Creates a new empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a mapping from a Lua line to a source line.
    pub fn record(&mut self, lua_line: usize, source_line: usize) {
        self.mappings.insert(lua_line, source_line);
    }

    /// Finds the most likely source line for a given Lua line.
    ///
    /// If no exact match, returns the closest preceding mapping.
    pub fn lookup(&self, lua_line: usize) -> Option<usize> {
        // Try exact match first
        if let Some(&source_line) = self.mappings.get(&lua_line) {
            return Some(source_line);
        }

        // Find the closest preceding mapping
        self.mappings
            .range(..=lua_line)
            .next_back()
            .map(|(_, &source_line)| source_line)
    }

    /// Generates an encoded source map comment that can be embedded in Lua code.
    pub fn to_comment(&self) -> String {
        if self.mappings.is_empty() {
            return String::new();
        }

        // Format: --[[SRCMAP:lua1=src1,lua2=src2,...]]
        let pairs: Vec<String> = self
            .mappings
            .iter()
            .map(|(lua, src)| format!("{}={}", lua, src))
            .collect();

        format!("--[[SRCMAP:{}]]", pairs.join(","))
    }

    /// Parses a source map from an encoded comment.
    pub fn from_comment(comment: &str) -> Option<Self> {
        let start = comment.find("--[[SRCMAP:")?;
        let end = comment[start..].find("]]")?;
        let content = &comment[start + 11..start + end];

        let mut map = Self::new();
        for pair in content.split(',') {
            let parts: Vec<&str> = pair.split('=').collect();
            if parts.len() == 2 {
                if let (Ok(lua), Ok(src)) = (parts[0].parse(), parts[1].parse()) {
                    map.record(lua, src);
                }
            }
        }
        Some(map)
    }

    /// Returns true if no mappings are recorded.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Translates a Lua error message to show the original .luat source line.
    ///
    /// Looks for line number patterns like `:123:` in the error message
    /// and replaces the Lua line number with the mapped .luat source line.
    pub fn translate_error(&self, error_msg: &str) -> String {
        use std::borrow::Cow;

        // Pattern: `:LINE:` where LINE is a number
        let re = regex::Regex::new(r":(\d+):").unwrap();

        let result: Cow<str> = re.replace_all(error_msg, |caps: &regex::Captures| {
            if let Ok(lua_line) = caps[1].parse::<usize>() {
                if let Some(source_line) = self.lookup(lua_line) {
                    return format!(":{}:", source_line);
                }
            }
            caps[0].to_string()
        });

        result.into_owned()
    }
}

/// Generates Lua code from an IR.
///
/// # Arguments
///
/// * `ir` - The intermediate representation to generate code from
/// * `module_name` - Name to use in the generated module metadata
///
/// # Returns
///
/// A string containing the generated Lua source code.
///
/// # Examples
///
/// ```rust,ignore
/// let ir = transform_ast(ast)?;
/// let lua_code = generate_lua_code(ir, "Button")?;
/// ```
pub fn generate_lua_code(ir: IR, module_name: &str) -> Result<String> {
    let mut generator = LuaCodeGenerator::new(module_name);
    generator.generate(ir)
}

/// Generates Lua code with source map for error line mapping.
///
/// This function returns both the generated Lua code and a source map
/// that can be used to convert Lua line numbers back to .luat source lines.
///
/// The source map is also embedded in the Lua code as a comment.
pub fn generate_lua_code_with_sourcemap(ir: IR, module_name: &str) -> Result<(String, LuaSourceMap)> {
    let mut generator = LuaCodeGenerator::new(module_name);
    generator.generate_with_sourcemap(ir)
}

struct LuaCodeGenerator {
    module_name: String,
    output: String,
    indent_level: usize,
    local_vars: std::collections::HashSet<String>,
    /// Current output line number (1-indexed).
    current_line: usize,
    /// Source map being built.
    source_map: LuaSourceMap,
}

impl LuaCodeGenerator {
    fn new(module_name: &str) -> Self {
        Self {
            module_name: module_name.to_string(),
            output: String::new(),
            indent_level: 0,
            local_vars: std::collections::HashSet::new(),
            current_line: 1,
            source_map: LuaSourceMap::new(),
        }
    }

    /// Records a source mapping for the current output line.
    fn record_source_line(&mut self, source_line: usize) {
        if source_line > 0 {
            self.source_map.record(self.current_line, source_line);
        }
    }

    fn parse_local_vars(script: &str) -> std::collections::HashSet<String> {
        // Very simple: look for lines like 'local foo' or 'local foo = ...'
        let mut set = std::collections::HashSet::new();
        for line in script.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("local ") {
                let ident: String = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !ident.is_empty() {
                    set.insert(ident);
                }
            }
        }
        set
    }

    fn generate(&mut self, ir: IR) -> Result<String> {
        self.write_line("-- Generated Lua template module");
        self.write_line(&format!("-- Module: {}", self.module_name));
        self.write_line("");

        // Generate helper functions
        self.generate_helpers()?;

        // Generate module script (hoisted, executed once)
        if let Some(module_script) = ir.module_script {
            self.write_line("-- Module script (hoisted)");
            self.write_line(&module_script.content);
            self.write_line("");
        }

        self.write_line("local function render(props, runtime)");
        self.indent();
        self.write_line("runtime = runtime or {}");
        self.write_line("props = props or {}");
        self.write_line("local __output = {}");

        self.write_line("local function __write(content)");
        self.indent();
        self.write_line("table.insert(__output, tostring(content))");
        self.dedent();
        self.write_line("end");
        self.write_line("");
        // generate context api inside render function        
        self.write_line("runtime.context_stack = runtime.context_stack or {}");
        self.write_line("table.insert(runtime.context_stack, {})");

        self.genrate_context_inline_helper()?;

        // Generate regular script (executed on each render)
        if let Some(regular_script) = ir.regular_script {
            self.write_line("-- Regular script (executed on each render)");
            self.write_line(&regular_script.content);
            // Parse local vars from script
            self.local_vars = Self::parse_local_vars(&regular_script.content);
            self.write_line("");
        } else {
            self.local_vars.clear();
        }

        // Generate template body
        self.generate_nodes(&ir.body)?;

        self.write_line("");
        self.write_line("-- Pop the context scope after rendering");
        self.write_line("table.remove(runtime.context_stack)");
        self.write_line("return table.concat(__output)");
        self.dedent();
        self.write_line("end");

        // Generate module exports
        // Keep this that was the first version we created
        // self.write_line("");
        // self.write_line("return {");
        // self.indent();
        // self.write_line("render = render,");
        // self.write_line(&format!("type = \"{}\"", self.module_name));
        // self.dedent();
        // self.write_line("}");

        self.write_line("");
        self.write_line("-- Exported module");
        self.write_line("exports.render = render");
        self.write_line(&format!("exports.moduleName = \"{}\"", escape_lua_string(&self.module_name)));
        self.write_line("");
        self.write_line("return exports");

        Ok(std::mem::take(&mut self.output))
    }

    fn genrate_context_inline_helper(&mut self) -> Result<()> {
        // Context API (like Svelte's setContext/getContext)
        self.write_line("-- Context API (like Svelte's setContext/getContext)");

        self.write_line("local function setContext(key, value)");
        self.indent();
        self.write_line("local current = runtime.context_stack[#runtime.context_stack]");
        self.write_line("if current then");
        self.indent();
        self.write_line("current[key] = value");
        self.dedent();
        self.write_line("else");
        self.indent();
        self.write_line("error(\"No current context scope to set key: \" .. key)");
        self.dedent();
        self.write_line("end");
        self.dedent();
        self.write_line("end");
        self.write_line("");

        self.write_line("local function getContext(key)");
        self.indent();
        self.write_line("-- Lookup from top to bottom");
        self.write_line("for i = #runtime.context_stack, 1, -1 do");
        self.indent();
        self.write_line("local scope = runtime.context_stack[i]");
        self.write_line("if scope[key] ~= nil then");
        self.indent();
        self.write_line("return scope[key]");
        self.dedent();
        self.write_line("end");
        self.dedent();
        self.write_line("end");
        self.write_line("return nil");
        self.dedent();
        self.write_line("end");
        self.write_line("");

        // Page context API (non-scoped, persists across entire request)
        self.write_line("-- Page context API (persists across entire request, not scoped)");

        self.write_line("local function setPageContext(key, value)");
        self.indent();
        self.write_line("runtime.page_context = runtime.page_context or {}");
        self.write_line("runtime.page_context[key] = value");
        self.dedent();
        self.write_line("end");
        self.write_line("");

        self.write_line("local function getPageContext(key)");
        self.indent();
        self.write_line("return runtime.page_context and runtime.page_context[key]");
        self.dedent();
        self.write_line("end");
        self.write_line("");

        Ok(())
    }

    fn generate_helpers(&mut self) -> Result<()> {
        self.write_line("-- Helper functions");

        // HTML escaping function
        self.write_line("local function html_escape(str)");
        self.indent();
        self.write_line("if str == nil then return '' end");
        self.write_line("str = tostring(str)");
        self.write_line("str = string.gsub(str, '&', '&amp;')");
        self.write_line("str = string.gsub(str, '<', '&lt;')");
        self.write_line("str = string.gsub(str, '>', '&gt;')");
        self.write_line("str = string.gsub(str, '\"', '&quot;')");
        self.write_line("str = string.gsub(str, \"'\", '&#39;')");
        self.write_line("return str");
        self.dedent();
        self.write_line("end");
        self.write_line("");

        // Smart tostring function (fixed: only tables get JSON-like, others are plain tostring)
        self.write_line("local function smart_tostring(val)");
        self.indent();
        self.write_line("if val == nil then return '' end");
        self.write_line("if type(val) == 'table' then");
        self.indent();
        self.write_line("local parts = {}");
        self.write_line("for k, v in pairs(val) do");
        self.indent();
        self.write_line("table.insert(parts, '\"' .. k .. '\": ' .. smart_tostring(v))");
        self.dedent();
        self.write_line("end");
        self.write_line("return '{ ' .. table.concat(parts, ', ') .. ' }'");
        self.dedent();
        self.write_line("else");
        self.indent();
        self.write_line("return tostring(val)");
        self.dedent();
        self.write_line("end");
        self.dedent();
        self.write_line("end");
        self.write_line("");
        self.write_line("local exports = {}");

        Ok(())
    }

    fn generate_nodes(&mut self, nodes: &[IRNode]) -> Result<()> {
        for node in nodes {
            self.generate_node(node)?;
        }
        Ok(())
    }

    fn generate_node(&mut self, node: &IRNode) -> Result<()> {
        match node {
            IRNode::TextNode { content } => self.generate_text_node(content),
            IRNode::MustacheNode {
                expression,
                escaped,
            } => self.generate_mustache_node(expression, *escaped),
            IRNode::IfNode {
                condition,
                then_branch,
                else_branch,
                sensitive,
            } => self.generate_if_node(condition, then_branch, else_branch.as_ref(), *sensitive),
            IRNode::EachNode {
                list_expr,
                item_id,
                index_id,
                body,
                empty,
                sensitive,
            } => self.generate_each_node(
                list_expr,
                item_id,
                index_id.as_ref(),
                body,
                empty.as_ref(),
                *sensitive,
            ),
            IRNode::ElementNode {
                tag,
                attributes,
                children,
            } => self.generate_element_node(tag, attributes, children),
            IRNode::ComponentNode {
                name,
                attributes,
                children,
            } => self.generate_component_node(name, attributes, children.as_ref()),
            IRNode::LocalConst { name, expression } => {
                self.generate_local_const(name, expression)
            }
            IRNode::RenderChildren { optional } => self.generate_render_children(*optional),
            IRNode::ScriptAny { content } => {
                // Process dynamic expressions in script tags
                let processed_content = content.clone();
                
                // Look for mustache expressions in the script tag: {expression}
                let mut offset = 0;
                while let Some(start) = processed_content[offset..].find('{') {
                    let real_start = offset + start;
                    if let Some(end) = processed_content[real_start..].find('}') {
                        let real_end = real_start + end + 1;
                        let expr = &processed_content[real_start + 1..real_end - 1].trim();
                        
                        // Skip if this isn't a mustache expression (could be part of an HTML attribute)
                        if !expr.is_empty() && !expr.contains('<') && !expr.contains('>') {
                            // Get the value from the context
                            self.write_line(&format!(
                                "__write(\"{}\")",
                                processed_content[offset..real_start].replace("\\", "\\\\").replace("\"", "\\\"")
                            ));
                            self.write_line(&format!("__write(smart_tostring({}))", expr));
                            offset = real_end;
                        } else {
                            offset = real_start + 1; // Skip this { and continue
                        }
                    } else {
                        // No closing }, just output the rest
                        self.write_line(&format!(
                            "__write(\"{}\")",
                            processed_content[offset..].replace("\\", "\\\\").replace("\"", "\\\"")
                        ));
                        break;
                    }
                }
                
                // Output any remaining content
                if offset < processed_content.len() {
                    self.write_line(&format!(
                        "__write(\"{}\")",
                        processed_content[offset..].replace("\\", "\\\\").replace("\"", "\\\"")
                    ));
                }
                
                Ok(())
            }
            IRNode::HtmlComment { children } => self.generate_html_comment(children),
        }
    }

    fn generate_text_node(&mut self, content: &str) -> Result<()> {
        let escaped_content = content
            .replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t");

        self.write_line(&format!("__write(\"{}\")", escaped_content));
        Ok(())
    }

    fn generate_mustache_node(&mut self, expression: &Expression, escaped: bool) -> Result<()> {
        let expr = expression.content.trim();
        let source_line = expression.span.line;

        if escaped {
            self.write_line_with_source(
                &format!("__write(html_escape(smart_tostring({})))", expr),
                source_line,
            );
        } else {
            self.write_line_with_source(
                &format!("__write(smart_tostring({}))", expr),
                source_line,
            );
        }
        Ok(())
    }

    fn generate_local_const(&mut self, name: &str, expression: &Expression) -> Result<()> {
        let expr = expression.content.trim();
        let source_line = expression.span.line;
        self.write_line_with_source(&format!("local {} = {}", name, expr), source_line);
        Ok(())
    }

    fn generate_if_node(
        &mut self,
        condition: &Expression,
        then_branch: &[IRNode],
        else_branch: Option<&Vec<IRNode>>,
        sensitive: bool,
    ) -> Result<()> {
        let source_line = condition.span.line;

        if sensitive {
            self.write_line("__write(\"<!-- sensitive -->\")");
            self.write_line("-- sensitive");
        }

        self.write_line_with_source(&format!("if {} then", condition.content.trim()), source_line);
        self.indent();
        self.generate_nodes(then_branch)?;
        self.dedent();

        if let Some(else_nodes) = else_branch {
            self.write_line("else");
            self.indent();
            self.generate_nodes(else_nodes)?;
            self.dedent();
        }

        self.write_line("end");
        Ok(())
    }

    fn generate_each_node(
        &mut self,
        list_expr: &Expression,
        item_id: &str,
        index_id: Option<&String>,
        body: &[IRNode],
        empty: Option<&Vec<IRNode>>,
        sensitive: bool,
    ) -> Result<()> {
        let source_line = list_expr.span.line;

        if sensitive {
            self.write_line("__write(\"<!-- sensitive -->\")");
            self.write_line("-- sensitive");
        }

        self.write_line_with_source(
            &format!("local __list = {} or {{}}", list_expr.content.trim()),
            source_line,
        );

        if let Some(empty_nodes) = empty {
            self.write_line("if #__list == 0 then");
            self.indent();
            self.generate_nodes(empty_nodes)?;
            self.dedent();
            self.write_line("else");
            self.indent();
        }

        let index_var = index_id.as_ref().map(|s| s.as_str()).unwrap_or("__i");
        self.write_line(&format!(
            "for {}, {} in ipairs(__list) do",
            index_var, item_id
        ));
        self.indent();

        // Add loop variables to local_vars
        self.local_vars.insert(item_id.to_string());
        if let Some(idx_id) = index_id {
            self.local_vars.insert(idx_id.clone());
        } else {
            self.local_vars.insert("__i".to_string());
        }

        // Create local context for loop variables
        self.write_line("local __loop_props = setmetatable({");
        self.indent();
        self.write_line(&format!("{} = {},", item_id, item_id));
        if let Some(idx_id) = index_id {
            self.write_line(&format!("{} = {},", idx_id, index_var));
        }
        self.dedent();
        self.write_line("}, {__index = props})");

        // Temporarily override props for loop body
        self.write_line("local __old_props = props");
        self.write_line("props = __loop_props");

        self.generate_nodes(body)?;

        self.write_line("props = __old_props");

        // Remove loop variables from local_vars after loop
        self.local_vars.remove(item_id);
        if let Some(idx_id) = index_id {
            self.local_vars.remove(idx_id);
        } else {
            self.local_vars.remove("__i");
        }

        self.dedent();
        self.write_line("end");

        if empty.is_some() {
            self.dedent();
            self.write_line("end");
        }

        Ok(())
    }

    fn generate_element_node(
        &mut self,
        tag: &str,
        attributes: &[IRAttribute],
        children: &[IRNode],
    ) -> Result<()> {
        // Opening tag
        self.write_line(&format!("__write(\"<{}\")", tag)); // Removed trailing space here

        // Attributes
        for attr in attributes {
            self.generate_attribute(attr)?;
        }

        if children.is_empty() && is_void_element(tag) {
            // Check for HTML void elements
            self.write_line("__write(\" />\")");
        } else {
            self.write_line("__write(\">\")");
            self.generate_nodes(children)?;
            self.write_line(&format!("__write(\"</{}>\")", tag));
        }

        Ok(())
    }

    fn generate_attribute(&mut self, attr: &IRAttribute) -> Result<()> {
        match attr {
            IRAttribute::Named { name, value } => match value {
                IRAttributeValue::Static(val) => {
                    self.write_line(&format!(
                        "__write(\" {}=\\\"{}\\\"\")",
                        name,
                        escape_lua_string(val)
                    ));
                }
                IRAttributeValue::Dynamic(expr) => {
                    let source_line = expr.span.line;
                    if name == "class" {
                        self.write_line_with_source(
                            &format!("local __val = {}", expr.content.trim()),
                            source_line,
                        );
                        self.write_line("if type(__val) == 'table' then");
                        self.indent();
                        self.write_line("local __classes = {}");
                        self.write_line("for k, v in pairs(__val) do if v then table.insert(__classes, k) end end");
                        self.write_line("__write(\" class=\\\"\" .. table.concat(__classes, ' ') .. \"\\\"\")");
                        self.dedent();
                        self.write_line("else");
                        self.indent();
                        self.write_line("__write(\" class=\\\"\" .. html_escape(tostring(__val)) .. \"\\\"\")");
                        self.dedent();
                        self.write_line("end");
                    } else {
                        self.write_line_with_source(
                            &format!(
                                "__write(\" {}=\\\"\" .. html_escape(tostring({})) .. \"\\\"\")",
                                name,
                                expr.content.trim()
                            ),
                            source_line,
                        );
                    }
                }
                IRAttributeValue::RawHtml(expr) => {
                    let source_line = expr.span.line;
                    self.write_line_with_source(
                        &format!(
                            "__write(\" {}=\\\"\" .. tostring({}) .. \"\\\"\")",
                            name,
                            expr.content.trim()
                        ),
                        source_line,
                    );
                }
                IRAttributeValue::BooleanTrue => {
                    self.write_line(&format!("__write(\" {}\")", name));
                }
            },
            IRAttribute::Spread(expr) => {
                let source_line = expr.span.line;
                // For spreading into HTML elements, iterate and append attributes
                self.write_line_with_source(
                    &format!("for __k, __v in pairs({}) do", expr.content.trim()),
                    source_line,
                );
                self.indent();
                self.write_line("__write(\" \" .. __k .. \"=\\\"\" .. html_escape(tostring(__v)) .. \"\\\"\")");
                self.dedent();
                self.write_line("end");
            }
        }
        Ok(())
    }

    fn generate_component_node(
        &mut self,
        name: &str,
        attributes: &[IRAttribute],
        children: Option<&Vec<IRNode>>,
    ) -> Result<()> {
        // Build props table for component ensuring order of spreads/named attrs
        self.write_line("local __component_props = {}");

        for attr in attributes {
            match attr {
                IRAttribute::Named { name, value } => match value {
                    IRAttributeValue::Static(val) => {
                        self.write_line(&format!("{} = \"{}\"", component_prop_setter(name), escape_lua_string(val)));
                    }
                    IRAttributeValue::Dynamic(expr) => {
                        let source_line = expr.span.line;
                        self.write_line_with_source(
                            &format!("{} = {}", component_prop_setter(name), expr.content.trim()),
                            source_line,
                        );
                    }
                    IRAttributeValue::RawHtml(expr) => {
                        let source_line = expr.span.line;
                        self.write_line_with_source(
                            &format!("{} = {}", component_prop_setter(name), expr.content.trim()),
                            source_line,
                        );
                    }
                    IRAttributeValue::BooleanTrue => {
                        self.write_line(&format!("{} = true", component_prop_setter(name)));
                    }
                },
                IRAttribute::Spread(expr) => {
                    let source_line = expr.span.line;
                    self.write_line_with_source(
                        &format!("for __k, __v in pairs({}) do __component_props[__k] = __v end", expr.content.trim()),
                        source_line,
                    );
                }
            }
        }

        // Add children function if present
        if let Some(child_nodes) = children {
            self.write_line("__component_props.children = function(__write)");
            self.indent();
            self.generate_nodes(child_nodes)?;
            self.dedent();
            self.write_line("end");
        }

        // Call component render function
        // self.write_line(&format!("__write({}.render(__component_props))", name));
        self.write_line(&format!(
            "__write({}.render(__component_props, runtime))",
            name
        ));

        Ok(())
    }

    fn generate_render_children(&mut self, optional: bool) -> Result<()> {
        if optional {
            self.write_line("if props.children then");
            self.indent();
            self.write_line("props.children(__write)");
            self.dedent();
            self.write_line("end");
        } else {
            self.write_line("if props.children then");
            self.indent();
            self.write_line("props.children(__write)");
            self.dedent();
            self.write_line("else");
            self.indent();
            self.write_line("error('children is required but not provided')");
            self.dedent();
            self.write_line("end");
        }
        Ok(())
    }

    fn generate_html_comment(&mut self, children: &[IRNode]) -> Result<()> {
        self.write_line("__write(\"<!--\")");
        self.generate_nodes(children)?;
        self.write_line("__write(\"-->\")");
        Ok(())
    }

    fn write_line(&mut self, line: &str) {
        if !line.is_empty() {
            self.output.push_str(&"  ".repeat(self.indent_level));
        }
        self.output.push_str(line);
        self.output.push('\n');
        self.current_line += 1;
    }

    /// Writes a line and records the source mapping.
    fn write_line_with_source(&mut self, line: &str, source_line: usize) {
        self.record_source_line(source_line);
        self.write_line(line);
    }

    fn indent(&mut self) {
        self.indent_level += 1;
    }

    fn dedent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    /// Generates code with source map and returns both.
    fn generate_with_sourcemap(&mut self, ir: IR) -> Result<(String, LuaSourceMap)> {
        let code = self.generate(ir)?;

        // Prepend source map comment to the code
        let source_map_comment = self.source_map.to_comment();
        let final_code = if !source_map_comment.is_empty() {
            format!("{}\n{}", source_map_comment, code)
        } else {
            code
        };

        Ok((final_code, self.source_map.clone()))
    }
}

// Helper function to identify HTML void elements
fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Escapes a string for use in a Lua string literal.
pub fn escape_lua_string(s: &str) -> String {
    s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
}

fn component_prop_setter(name: &str) -> String {
    if is_valid_lua_identifier(name) {
        format!("__component_props.{}", name)
    } else {
        format!("__component_props[\"{}\"]", name)
    }
}

fn is_valid_lua_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {},
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Bundle multiple Lua modules into a single file
pub fn bundle_sources<F>(sources: Vec<(String, String)>, mut progress: F) -> Result<(String, crate::sourcemap::BundleSourceMap)>
where
    F: FnMut(usize, usize),
{
    let mut bundle = String::new();
    let mut source_map = crate::sourcemap::BundleSourceMap::new();

    bundle.push_str("-- Bundled Lua template modules\n");
    bundle.push_str("-- Use internal load function preserved by sandbox\n");
    bundle.push_str("local __load = __luat_internal_load\n");
    bundle.push_str("local __original_require = require\n");
    bundle.push_str("local __module_loaders = {}\n");
    bundle.push_str("local __modules = {}\n\n");

    bundle.push_str("local function __normalize_path(path)\n");
    bundle.push_str("  path = string.gsub(path, \"\\\\\", \"/\")\n");
    bundle.push_str("  local parts = {}\n");
    bundle.push_str("  for part in string.gmatch(path, \"[^/]+\") do\n");
    bundle.push_str("    if part == \"..\" then\n");
    bundle.push_str("      if #parts > 0 then table.remove(parts) end\n");
    bundle.push_str("    elseif part ~= \".\" and part ~= \"\" then\n");
    bundle.push_str("      table.insert(parts, part)\n");
    bundle.push_str("    end\n");
    bundle.push_str("  end\n");
    bundle.push_str("  return table.concat(parts, \"/\")\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __dirname(path)\n");
    bundle.push_str("  if not path or path == \"\" then return \"\" end\n");
    bundle.push_str("  local dir = string.match(path, \"^(.*)/\")\n");
    bundle.push_str("  if not dir then return \"\" end\n");
    bundle.push_str("  return dir\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __expand_alias(name)\n");
    bundle.push_str("  if string.sub(name, 1, 5) == \"$lib/\" then\n");
    bundle.push_str("    return \"lib/\" .. string.sub(name, 6)\n");
    bundle.push_str("  end\n");
    bundle.push_str("  return name\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __add_candidates(candidates, base)\n");
    bundle.push_str("  if base == \"\" then return end\n");
    bundle.push_str("  table.insert(candidates, base)\n");
    bundle.push_str("  if not string.match(base, \"%.luat$\") and not string.match(base, \"%.lua$\") then\n");
    bundle.push_str("    table.insert(candidates, base .. \".luat\")\n");
    bundle.push_str("    table.insert(candidates, base .. \".lua\")\n");
    bundle.push_str("  end\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __resolve_module(name, importer)\n");
    bundle.push_str("  local require_map = rawget(_G, \"__require_map\")\n");
    bundle.push_str("  if require_map ~= nil then\n");
    bundle.push_str("    local importer_map = require_map[importer] or require_map[\"\"]\n");
    bundle.push_str("    if importer_map and importer_map[name] then\n");
    bundle.push_str("      local resolved = importer_map[name]\n");
    bundle.push_str("      if __modules[resolved] ~= nil then\n");
    bundle.push_str("        return \"module\", resolved\n");
    bundle.push_str("      end\n");
    bundle.push_str("      if __module_loaders[resolved] ~= nil then\n");
    bundle.push_str("        return \"loader\", resolved\n");
    bundle.push_str("      end\n");
    bundle.push_str("      if __server_sources and __server_sources[resolved] ~= nil then\n");
    bundle.push_str("        return \"server\", resolved\n");
    bundle.push_str("      end\n");
    bundle.push_str("    end\n");
    bundle.push_str("  end\n");
    bundle.push_str("  local candidates = {}\n");
    bundle.push_str("  local expanded = __expand_alias(name)\n");
    bundle.push_str("  local base_dir = \"\"\n");
    bundle.push_str("  if importer and importer ~= \"\" then\n");
    bundle.push_str("    base_dir = __dirname(importer)\n");
    bundle.push_str("  end\n");
    bundle.push_str("  if string.sub(expanded, 1, 1) == \"/\" then\n");
    bundle.push_str("    __add_candidates(candidates, __normalize_path(string.sub(expanded, 2)))\n");
    bundle.push_str("  elseif string.sub(expanded, 1, 2) == \"./\" or string.sub(expanded, 1, 3) == \"../\" then\n");
    bundle.push_str("    local joined = expanded\n");
    bundle.push_str("    if base_dir ~= \"\" then joined = base_dir .. \"/\" .. expanded end\n");
    bundle.push_str("    __add_candidates(candidates, __normalize_path(joined))\n");
    bundle.push_str("  else\n");
    bundle.push_str("    if base_dir ~= \"\" then\n");
    bundle.push_str("      __add_candidates(candidates, __normalize_path(base_dir .. \"/\" .. expanded))\n");
    bundle.push_str("    end\n");
    bundle.push_str("    __add_candidates(candidates, __normalize_path(expanded))\n");
    bundle.push_str("  end\n");
    bundle.push_str("  local basename = string.match(expanded, \"([^/]+)$\") or expanded\n");
    bundle.push_str("  if basename ~= expanded then\n");
    bundle.push_str("    if base_dir ~= \"\" then\n");
    bundle.push_str("      __add_candidates(candidates, __normalize_path(base_dir .. \"/\" .. basename))\n");
    bundle.push_str("    end\n");
    bundle.push_str("    __add_candidates(candidates, __normalize_path(basename))\n");
    bundle.push_str("  end\n");
    bundle.push_str("  for _, key in ipairs(candidates) do\n");
    bundle.push_str("    if __modules[key] ~= nil then\n");
    bundle.push_str("      return \"module\", key\n");
    bundle.push_str("    end\n");
    bundle.push_str("    if __module_loaders[key] ~= nil then\n");
    bundle.push_str("      return \"loader\", key\n");
    bundle.push_str("    end\n");
    bundle.push_str("    if __server_sources and __server_sources[key] ~= nil then\n");
    bundle.push_str("      return \"server\", key\n");
    bundle.push_str("    end\n");
    bundle.push_str("  end\n");
    bundle.push_str("  return nil\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __load_server_module(key)\n");
    bundle.push_str("  if package.loaded[key] ~= nil then return package.loaded[key] end\n");
    bundle.push_str("  if not __server_sources then return nil end\n");
    bundle.push_str("  local source = __server_sources[key]\n");
    bundle.push_str("  if not source then return nil end\n");
    bundle.push_str("  local prev = _G.__luat_current_module\n");
    bundle.push_str("  _G.__luat_current_module = key\n");
    bundle.push_str("  local fn = __load(source, \"@\" .. key)\n");
    bundle.push_str("  local ok, result = pcall(fn)\n");
    bundle.push_str("  _G.__luat_current_module = prev\n");
    bundle.push_str("  if not ok then error(result, 2) end\n");
    bundle.push_str("  if result == nil then result = true end\n");
    bundle.push_str("  package.loaded[key] = result\n");
    bundle.push_str("  return result\n");
    bundle.push_str("end\n\n");

    bundle.push_str("local function __require(name)\n");
    bundle.push_str("  local importer = _G.__luat_current_module or \"\"\n");
    bundle.push_str("  local kind, key = __resolve_module(name, importer)\n");
    bundle.push_str("  if kind == \"module\" then return __modules[key] end\n");
    bundle.push_str("  if kind == \"loader\" then\n");
    bundle.push_str("    local result = __module_loaders[key]()\n");
    bundle.push_str("    if result == nil then result = true end\n");
    bundle.push_str("    __modules[key] = result\n");
    bundle.push_str("    package.loaded[key] = result\n");
    bundle.push_str("    return result\n");
    bundle.push_str("  end\n");
    bundle.push_str("  if kind == \"server\" then\n");
    bundle.push_str("    local result = __load_server_module(key)\n");
    bundle.push_str("    if result ~= nil then return result end\n");
    bundle.push_str("  end\n");
    bundle.push_str("  return __original_require(name)\n");
    bundle.push_str("end\n\n");

    bundle.push_str("_G.require = __require\n\n");

    // Add helper to enhance error messages with module context
    bundle.push_str("local function __enhance_error(err, module_name)\n");
    bundle.push_str("  if type(err) ~= 'string' then return err end\n");
    bundle.push_str("  -- Check if error already has module context\n");
    bundle.push_str("  if string.find(err, module_name, 1, true) then return err end\n");
    bundle.push_str("  -- Try to extract line number from bundle error\n");
    bundle.push_str("  local line = string.match(err, ':(%d+):')\n");
    bundle.push_str("  if line then\n");
    bundle.push_str("    -- Replace bundle reference with module name\n");
    bundle.push_str("    local msg = string.match(err, ':%d+:%s*(.*)$') or err\n");
    bundle.push_str("    return module_name .. ':' .. line .. ': ' .. msg\n");
    bundle.push_str("  end\n");
    bundle.push_str("  return module_name .. ': ' .. err\n");
    bundle.push_str("end\n\n");

    // Generate all modules
    for (i, (name, source)) in sources.iter().enumerate() {
        progress(i, sources.len());

        // Calculate the line offset where this module's source starts
        // (current bundle lines + 5 wrapper lines: comment, function, prev, current_module, pcall)
        let bundle_lines_so_far = bundle.lines().count();
        let module_source_start_line = bundle_lines_so_far + 6; // 5 wrapper lines + 1 for 1-indexing

        let escaped_name = escape_lua_string(name);
        bundle.push_str(&format!("-- Module: {}\n", name));
        bundle.push_str(&format!("__module_loaders[\"{}\"] = function()\n", escaped_name));
        bundle.push_str("  local __prev = _G.__luat_current_module\n");
        bundle.push_str(&format!("  _G.__luat_current_module = \"{}\"\n", escaped_name));
        bundle.push_str("  local __ok, __result = pcall(function()\n");

        // Indent the source code
        for line in source.lines() {
            bundle.push_str("    ");
            bundle.push_str(line);
            bundle.push('\n');
        }

        bundle.push_str("  end)\n");
        bundle.push_str("  _G.__luat_current_module = __prev\n");
        bundle.push_str(&format!(
            "  if not __ok then error(__enhance_error(__result, \"{}\"), 2) end\n",
            escaped_name
        ));
        bundle.push_str("  return __result\n");
        bundle.push_str("end\n\n");

        // Add module to source map
        source_map.add_module(name, name, module_source_start_line, source);
    }

    // Export modules table
    bundle.push_str("return __modules\n");

    progress(sources.len(), sources.len());
    Ok((bundle, source_map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_template;
    use crate::transform::transform_ast;

    #[test]
    fn test_generate_simple_template() {
        let source = r#"<div>Hello {name}</div>"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();

        let lua_code = generate_lua_code(ir, "test").unwrap();

        println!("Generated Lua code:\n{}", lua_code);

        assert!(lua_code.contains("function render(props, runtime)"));
        assert!(lua_code.contains("__write(\"<div\")")); // Fixed: the tag is split
        assert!(lua_code.contains("__write(html_escape(smart_tostring(name)))"));
        assert!(lua_code.contains("</div>"));
    }

    #[test]
    fn test_generate_component() {
        let source = r#"        
        <Card title="Hello">World</Card>
        "#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();

        let lua_code = generate_lua_code(ir, "test").unwrap();
        println!("Generated Lua code:\n{}", lua_code);
        assert!(lua_code.contains("Card.render"));
        assert!(lua_code.contains("title = \"Hello\""));
        assert!(lua_code.contains("children = function"));
    }

    #[test]
    fn test_generate_if_block() {
        let source = r#"{#if show}Hello{/if}"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();

        let lua_code = generate_lua_code(ir, "test").unwrap();
        println!("Generated Lua code:\n{}", lua_code);
        assert!(lua_code.contains("if show then"));
        assert!(lua_code.contains("end"));
    }

    #[test]
    fn test_generate_each_block() {
        let source = r#"{#each items as item}Hello {item}{/each}"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();

        let lua_code = generate_lua_code(ir, "test").unwrap();

        assert!(lua_code.contains("for __i, item in ipairs(__list) do"));
        assert!(lua_code.contains("local __loop_props"));
    }

    #[test]
    fn test_bundle_sources() {
        let sources = vec![
            (
                "Card".to_string(),
                "return { render = function() end }".to_string(),
            ),
            (
                "Button".to_string(),
                "return { render = function() end }".to_string(),
            ),
        ];

        let mut progress_calls = 0;
        let (bundle, source_map) = bundle_sources(sources, |current, total| {
            progress_calls += 1;
            assert!(current <= total);
        })
        .unwrap();

        assert!(bundle.contains("__module_loaders[\"Card\"]"));
        assert!(bundle.contains("__module_loaders[\"Button\"]"));
        assert!(bundle.contains("local function __require"));
        assert!(progress_calls > 0);

        // Verify source map has module entries
        assert!(source_map.modules.contains_key("Card"));
        assert!(source_map.modules.contains_key("Button"));
    }

    #[test]
    fn test_source_map_basic() {
        let mut source_map = LuaSourceMap::new();
        source_map.record(10, 5);  // Lua line 10 -> source line 5
        source_map.record(20, 15); // Lua line 20 -> source line 15

        // Exact match
        assert_eq!(source_map.lookup(10), Some(5));
        assert_eq!(source_map.lookup(20), Some(15));

        // Closest preceding
        assert_eq!(source_map.lookup(12), Some(5));  // Between 10 and 20, closest is 10
        assert_eq!(source_map.lookup(25), Some(15)); // After 20, closest is 20
    }

    #[test]
    fn test_source_map_error_translation() {
        let mut source_map = LuaSourceMap::new();
        source_map.record(72, 10);  // Lua line 72 -> source line 10

        let error = "syntax error: src/routes/blog/new/+page.luat:72: unexpected symbol near '{'";
        let translated = source_map.translate_error(error);

        assert!(translated.contains(":10:"), "Expected :10: but got: {}", translated);
        assert!(!translated.contains(":72:"), "Should not contain :72:");
    }

    #[test]
    fn test_source_map_to_comment() {
        let mut source_map = LuaSourceMap::new();
        source_map.record(10, 5);
        source_map.record(20, 15);

        let comment = source_map.to_comment();
        assert!(comment.contains("SRCMAP:"));
        assert!(comment.contains("10=5"));
        assert!(comment.contains("20=15"));
    }

    #[test]
    fn test_source_map_from_comment() {
        let comment = "--[[SRCMAP:10=5,20=15]]";
        let source_map = LuaSourceMap::from_comment(comment).unwrap();

        assert_eq!(source_map.lookup(10), Some(5));
        assert_eq!(source_map.lookup(20), Some(15));
    }

    #[test]
    fn test_generate_with_sourcemap() {
        let source = r#"<div>{name}</div>"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();

        let (lua_code, source_map) = generate_lua_code_with_sourcemap(ir, "test").unwrap();

        // The source map should have some mappings
        assert!(!source_map.is_empty(), "Source map should have mappings for expressions");

        // The generated code should work
        assert!(lua_code.contains("function render"));
    }
}
