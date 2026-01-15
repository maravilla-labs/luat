// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! AST to IR transformation.
//!
//! This module transforms the raw [`TemplateAST`] from the parser into
//! an Intermediate Representation ([`IR`]) optimized for code generation.
//!
//! # Transformation Steps
//!
//! 1. **Whitespace normalization**: Empty text nodes are removed
//! 2. **Component collection**: All used components are tracked
//! 3. **Control flow flattening**: Nested blocks are preserved with metadata
//! 4. **Attribute processing**: Dynamic vs static attributes are distinguished
//!
//! # Usage
//!
//! ```rust,ignore
//! use luat::{parse_template, transform_ast, validate_ir};
//!
//! let ast = parse_template(source)?;
//! let ir = transform_ast(ast)?;
//! validate_ir(&ir)?;
//! ```

use crate::ast::*;
use crate::error::Result;
use std::collections::HashSet;

/// Intermediate Representation of a transformed template.
///
/// The IR is a simplified form of the AST that is ready for code generation.
/// It tracks script blocks, template body, and component dependencies.
#[derive(Debug, Clone)]
pub struct IR {
    /// Module-level script (runs once when loaded).
    pub module_script: Option<ScriptBlock>,
    /// Component script (runs on each render).
    pub regular_script: Option<ScriptBlock>,
    /// The transformed template body.
    pub body: Vec<IRNode>,
    /// Set of component names used in this template.
    pub components: HashSet<String>,
}

/// A node in the transformed intermediate representation.
#[derive(Debug, Clone)]
pub enum IRNode {
    /// Plain text content.
    TextNode {
        /// The text content.
        content: String,
    },
    /// An expression to be output.
    MustacheNode {
        /// The Lua expression to evaluate.
        expression: Expression,
        /// If true, HTML-escape the output.
        escaped: bool,
    },
    /// A conditional block.
    IfNode {
        /// The condition expression.
        condition: Expression,
        /// Nodes to render when truthy.
        then_branch: Vec<IRNode>,
        /// Nodes to render when falsy.
        else_branch: Option<Vec<IRNode>>,
        /// If true, preserve whitespace.
        sensitive: bool,
    },
    /// An iteration block.
    EachNode {
        /// Expression yielding the list to iterate.
        list_expr: Expression,
        /// Variable name for current item.
        item_id: String,
        /// Optional variable name for index.
        index_id: Option<String>,
        /// Nodes to render for each item.
        body: Vec<IRNode>,
        /// Nodes to render when list is empty.
        empty: Option<Vec<IRNode>>,
        /// If true, preserve whitespace.
        sensitive: bool,
    },
    /// Local constant declaration `{@local}`.
    LocalConst {
        /// The variable name.
        name: String,
        /// The value expression.
        expression: Expression,
    },
    /// An HTML element.
    ElementNode {
        /// The tag name.
        tag: String,
        /// Element attributes.
        attributes: Vec<IRAttribute>,
        /// Child nodes.
        children: Vec<IRNode>,
    },
    /// A component invocation.
    ComponentNode {
        /// The component name.
        name: String,
        /// Props passed to the component.
        attributes: Vec<IRAttribute>,
        /// Children to pass (None if no children).
        children: Option<Vec<IRNode>>,
    },
    /// Children slot render directive.
    RenderChildren {
        /// If true, no error when children is nil.
        optional: bool,
    },
    /// Pass-through script content.
    ScriptAny {
        /// The script content.
        content: String,
    },
    /// HTML comment node.
    HtmlComment {
        /// Comment content (may include expressions).
        children: Vec<IRNode>,
    },
}

/// An attribute in the IR.
#[derive(Debug, Clone)]
pub enum IRAttribute {
    /// A named attribute.
    Named {
        /// The attribute name.
        name: String,
        /// The attribute value.
        value: IRAttributeValue,
    },
    /// A spread operator `{...expr}`.
    Spread(Expression),
}

/// The value of an IR attribute.
#[derive(Debug, Clone)]
pub enum IRAttributeValue {
    /// A static string value.
    Static(String),
    /// A dynamic expression value.
    Dynamic(Expression),
    /// Raw HTML expression (not escaped).
    RawHtml(Expression),
    /// Boolean true (attribute present without value).
    BooleanTrue,
}

/// Transforms a [`TemplateAST`] into an [`IR`].
///
/// This function processes the AST to create an IR suitable for code generation,
/// collecting component dependencies and normalizing the structure.
///
/// # Errors
///
/// Returns an error if transformation fails (e.g., invalid `{@local}` placement).
pub fn transform_ast(ast: TemplateAST) -> Result<IR> {
    let mut components = HashSet::new();
    let body = transform_nodes(ast.body, &mut components, false)?;

    Ok(IR {
        module_script: ast.module_script,
        regular_script: ast.regular_script,
        body,
        components,
    })
}

fn transform_nodes(
    nodes: Vec<Node>,
    components: &mut HashSet<String>,
    in_block: bool,
) -> Result<Vec<IRNode>> {
    let mut ir_nodes = Vec::new();
    
    for node in nodes {
        if let Some(ir_node) = transform_node(node, components, in_block)? {
            ir_nodes.push(ir_node);
        }
        // Skip empty nodes (None case)
    }
    
    Ok(ir_nodes)
}

fn transform_node(
    node: Node,
    components: &mut HashSet<String>,
    in_block: bool,
) -> Result<Option<IRNode>> {
    match node {
        Node::TextNode { content } => {
            if content.trim().is_empty() {
                Ok(None) // Skip whitespace-only text nodes
            } else {
                Ok(Some(IRNode::TextNode { content }))
            }
        }
        
        Node::MustacheNode { expression } => {
            Ok(Some(IRNode::MustacheNode {
                expression,
                escaped: true,
            }))
        }
        
        Node::RawHtml { expression } => {
            Ok(Some(IRNode::MustacheNode {
                expression,
                escaped: false,
            }))
        }

        Node::LocalConst { name, expression } => {
            if !in_block {
                return Err(crate::error::LuatError::TransformError(
                    "{@local} is only allowed as an immediate child of a block".to_string(),
                ));
            }
            Ok(Some(IRNode::LocalConst { name, expression }))
        }
        
        Node::IfBlock { condition, then_branch, else_branch } => {
            let then_ir = transform_nodes(then_branch, components, true)?;
            let else_ir = match else_branch {
                Some(else_nodes) => Some(transform_nodes(else_nodes, components, true)?),
                None => None,
            };
            
            Ok(Some(IRNode::IfNode {
                condition,
                then_branch: then_ir,
                else_branch: else_ir,
                sensitive: false,
            }))
        }
        
        Node::SensitiveIfBlock { condition, then_branch, else_branch } => {
            let then_ir = transform_nodes(then_branch, components, true)?;
            let else_ir = match else_branch {
                Some(else_nodes) => Some(transform_nodes(else_nodes, components, true)?),
                None => None,
            };
            
            Ok(Some(IRNode::IfNode {
                condition,
                then_branch: then_ir,
                else_branch: else_ir,
                sensitive: true,
            }))
        }
        
        Node::EachBlock { list_expr, item_id, index_id, body, empty } => {
            let body_ir = transform_nodes(body, components, true)?;
            let empty_ir = match empty {
                Some(empty_nodes) => Some(transform_nodes(empty_nodes, components, true)?),
                None => None,
            };
            
            Ok(Some(IRNode::EachNode {
                list_expr,
                item_id,
                index_id,
                body: body_ir,
                empty: empty_ir,
                sensitive: false,
            }))
        }
        
        Node::SensitiveEachBlock { list_expr, item_id, index_id, body, empty } => {
            let body_ir = transform_nodes(body, components, true)?;
            let empty_ir = match empty {
                Some(empty_nodes) => Some(transform_nodes(empty_nodes, components, true)?),
                None => None,
            };
            
            Ok(Some(IRNode::EachNode {
                list_expr,
                item_id,
                index_id,
                body: body_ir,
                empty: empty_ir,
                sensitive: true,
            }))
        }
        
        Node::ElementNode { tag, attributes, children } => {
            let ir_attributes = transform_attributes(attributes)?;
            let ir_children = transform_nodes(children, components, false)?;
            
            Ok(Some(IRNode::ElementNode {
                tag,
                attributes: ir_attributes,
                children: ir_children,
            }))
        }
        
        Node::ComponentNode { name, attributes, children } => {
            // Insert the full component name to preserve path information
            components.insert(name.clone());

            let ir_attributes = transform_attributes(attributes)?;
            let ir_children = if children.is_empty() {
                None
            } else {
                Some(transform_nodes(children, components, true)?)
            };
            
            Ok(Some(IRNode::ComponentNode {
                name,
                attributes: ir_attributes,
                children: ir_children,
            }))
        }

        Node::HtmlComment { children } => {
            let ir_children = transform_nodes(children, components, false)?;
            Ok(Some(IRNode::HtmlComment { children: ir_children }))
        }

        Node::LuatComment => {
            // Ignore LUAT comments in IR
            Ok(None)
        }

        Node::RenderChildren { optional } => {
            Ok(Some(IRNode::RenderChildren { optional }))
        }
        
        Node::ScriptAny { tag: _, content } => {
            Ok(Some(IRNode::ScriptAny { content }))
        },
    }
}

fn transform_attributes(attributes: Vec<Attribute>) -> Result<Vec<IRAttribute>> {
    let mut ir_attributes = Vec::new();

    for attr in attributes {
        match attr {
            Attribute::Named { name, value } => {
                let ir_value = match value {
                    AttributeValue::Static(s) => IRAttributeValue::Static(s),
                    AttributeValue::Dynamic(expr) => IRAttributeValue::Dynamic(expr),
                    AttributeValue::RawHtml(expr) => IRAttributeValue::RawHtml(expr),
                    AttributeValue::Shorthand(expr) => IRAttributeValue::Dynamic(expr),
                    AttributeValue::BooleanTrue => IRAttributeValue::BooleanTrue,
                };

                ir_attributes.push(IRAttribute::Named { name, value: ir_value });
            }
            Attribute::Spread(expr) => {
                ir_attributes.push(IRAttribute::Spread(expr));
            }
        }
    }

    Ok(ir_attributes)
}

/// Validate the IR for common errors
pub fn validate_ir(ir: &IR) -> Result<()> {
    // Check for recursive component dependencies
    // In a full implementation, you'd build a dependency graph and check for cycles
    
    // Validate expressions (basic check)
    validate_ir_nodes(&ir.body)?;
    
    Ok(())
}

fn validate_ir_nodes(nodes: &[IRNode]) -> Result<()> {
    for node in nodes {
        match node {
            IRNode::IfNode { then_branch, else_branch, .. } => {
                validate_ir_nodes(then_branch)?;
                if let Some(else_nodes) = else_branch {
                    validate_ir_nodes(else_nodes)?;
                }
            }
            IRNode::EachNode { body, empty, .. } => {
                validate_ir_nodes(body)?;
                if let Some(empty_nodes) = empty {
                    validate_ir_nodes(empty_nodes)?;
                }
            }
            IRNode::ElementNode { children, .. } => {
                validate_ir_nodes(children)?;
            }
            IRNode::ComponentNode { children: Some(child_nodes), .. } => {
                validate_ir_nodes(child_nodes)?;
            }
            IRNode::ComponentNode { children: None, .. } => {}
            _ => {}
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_template;

    #[test]
    fn test_transform_simple_template() {
        let source = r#"<div>Hello {name}</div>"#;
        let ast = parse_template(source).unwrap();
        
        let ir = transform_ast(ast).unwrap();
        assert_eq!(ir.body.len(), 1);
        
        match &ir.body[0] {
            IRNode::ElementNode { tag, children, .. } => {
                assert_eq!(tag, "div");
                assert_eq!(children.len(), 2); // "Hello " and {name}
            }
            _ => panic!("Expected ElementNode"),
        }
    }

    #[test]
    fn test_transform_component() {
        let source = r#"<Card>Hello</Card>"#;
        let ast = parse_template(source).unwrap();
        
        let ir = transform_ast(ast).unwrap();
        assert!(ir.components.contains("Card"));
        
        match &ir.body[0] {
            IRNode::ComponentNode { name, .. } => {
                assert_eq!(name, "Card");
            }
            _ => panic!("Expected ComponentNode"),
        }
    }

    #[test]
    fn test_transform_if_block() {
        let source = r#"{#if condition}Hello{/if}"#;
        let ast = parse_template(source).unwrap();
        
        let ir = transform_ast(ast).unwrap();
        assert_eq!(ir.body.len(), 1);
        
        match &ir.body[0] {
            IRNode::IfNode { condition, then_branch, sensitive, .. } => {
                assert_eq!(condition.content, "condition");
                assert_eq!(then_branch.len(), 1);
                assert!(!sensitive);
            }
            _ => panic!("Expected IfNode"),
        }
    }

    #[test]
    fn test_transform_sensitive_if_block() {
        let source = r#"{!if condition}Hello{/if}"#;
        let ast = parse_template(source).unwrap();
        
        let ir = transform_ast(ast).unwrap();
        assert_eq!(ir.body.len(), 1);
        
        match &ir.body[0] {
            IRNode::IfNode { sensitive, .. } => {
                assert!(sensitive);
            }
            _ => panic!("Expected IfNode"),
        }
    }
}
