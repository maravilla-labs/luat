// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::*;
use crate::parser::{parse_template, LuatParser, Rule};
use pest::Parser;

#[test]
fn test_div_with_class() {
    // First test simple div without attributes
    let source1 = r#"<div>Hello</div>"#;
    println!("Testing: {}", source1);
    
    let result1 = parse_template(source1);
    match result1 {
        Ok(_ast) => {
            println!("Simple div parsed successfully");
         println!("Input: render children with optional call");  }
        Err(e) => {
            println!("Simple div parse error: {:?}", e);
            panic!("Simple div should parse successfully");
        }
    }
    
    // Now test with attributes (try simple value first)
    let source2 = r#"<div class=container>Hello</div>"#;
    println!("Testing with unquoted attribute: {}", source2);
    
    let result2 = parse_template(source2);
    match result2 {
        Ok(_ast) => {
            println!("Div with unquoted class parsed successfully");
        }
        Err(e) => {
            println!("Div with unquoted class parse error: {:?}", e);
        }
    }
    
    // Now test with quoted attributes
    let source3 = r#"<div class="container">Hello</div>"#;
    println!("Testing with quoted attribute: {}", source3);
    
    let result3 = parse_template(source3);
    match result3 {
        Ok(ast) => {
            println!("Div with quoted class parsed successfully: {:?}", ast);
            assert_eq!(ast.body.len(), 1);
        }
        Err(e) => {
            println!("Div with quoted class parse error: {:?}", e);
            panic!("Should parse successfully");
        }
    }
}

#[test]
fn test_each_block() {
    let source = r#"{#each items as item}Hello {item}{/each}"#;
    println!("Testing each block: {}", source);
    
    let result = parse_template(source);
    match result {
        Ok(ast) => {
            println!("Each block parsed successfully: {:?}", ast);
            assert_eq!(ast.body.len(), 1);
        }
        Err(e) => {
            println!("Each block parse error: {:?}", e);
            panic!("Each block should parse successfully");
        }
    }
}

#[test]
fn test_each_block_as_template() {
    let input = r#"{#each items as item}Hello{/each}"#;
    match parse_template(input) {
        Ok(ast) => {
            println!("Each block parsed as template successfully: {:?}", ast);
            // Check that we have one node and it's an each block
            assert_eq!(ast.body.len(), 1);
            match &ast.body[0] {
                Node::EachBlock { .. } => println!("Successfully parsed as each block"),
                other => panic!("Expected each block, got: {:?}", other),
            }
        }
        Err(e) => {
            println!("Each block template parse error: {:?}", e);
            panic!("Should parse as template");
        }
    }
}

#[test]
fn test_simple_template_debug() {
    let input = r#"Hello"#;
    match parse_template(input) {
        Ok(ast) => {
            println!("Simple template parsed successfully: {:?}", ast);
        }
        Err(e) => {
            println!("Simple template parse error: {:?}", e);
        }
    }

    let input2 = r#"{item}"#;
    match parse_template(input2) {
        Ok(ast) => {
            println!("Mustache template parsed successfully: {:?}", ast);
        }
        Err(e) => {
            println!("Mustache template parse error: {:?}", e);
        }
    }
}

#[test] 
fn test_diagnose_each_issue() {
    // Test each_start only
    let input1 = r#"{#each items as item}"#;
    let result1 = LuatParser::parse(Rule::each_start, input1);
    match result1 {
        Ok(_) => println!("✓ each_start parses correctly"),
        Err(e) => println!("✗ each_start failed: {}", e),
    }
    
    // Test each_end only 
    let input2 = r#"{/each}"#;
    let result2 = LuatParser::parse(Rule::each_end, input2);
    match result2 {
        Ok(_) => println!("✓ each_end parses correctly"),
        Err(e) => println!("✗ each_end failed: {}", e),
    }
    
    // Test simple each_block
    let input3 = r#"{#each items as item}{/each}"#;
    let result3 = LuatParser::parse(Rule::each_block, input3);
    match result3 {
        Ok(_) => println!("✓ simple each_block parses correctly"),
        Err(e) => println!("✗ simple each_block failed: {}", e),
    }
    
    // Test template_node with each_block
    let input4 = r#"{#each items as item}{/each}"#;
    let result4 = LuatParser::parse(Rule::template_node, input4);
    match result4 {
        Ok(_) => println!("✓ template_node with each_block parses correctly"),
        Err(e) => println!("✗ template_node with each_block failed: {}", e),
    }
    
    // Test template_content with each_block
    let input5 = r#"{#each items as item}{/each}"#;
    let result5 = LuatParser::parse(Rule::template_content, input5);
    match result5 {
        Ok(_) => println!("✓ template_content with each_block parses correctly"),
        Err(e) => println!("✗ template_content with each_block failed: {}", e),
    }
}

#[test]
fn test_expr_and_ident() {
    // Test expr
    let result1 = LuatParser::parse(Rule::expr, "items");
    match result1 {
        Ok(_) => println!("✓ expr 'items' parses correctly"),
        Err(e) => println!("✗ expr 'items' failed: {}", e),
    }
    
    // Test ident  
    let result2 = LuatParser::parse(Rule::ident, "item");
    match result2 {
        Ok(_) => println!("✓ ident 'item' parses correctly"),
        Err(e) => println!("✗ ident 'item' failed: {}", e),
    }
    
    // Test manual each_start components
    println!("Testing each_start step by step...");        // Test just the opening
        let result3 = LuatParser::parse(Rule::template, "{#each");
        match result3 {
            Ok(_) => println!("✓ '{{#each' part works"),
            Err(e) => println!("✗ '{{#each' part failed: {}", e),
        }
}

#[test]
fn test_each_start_components() {
    // Test just opening brace
    let result1 = LuatParser::parse(Rule::template, "{");
    match result1 {
        Ok(_) => println!("✓ opening brace works"),
        Err(e) => println!("✗ opening brace failed: {}", e),
    }
    
    // Test #each literal
    let result2 = LuatParser::parse(Rule::template, "{#each");
    match result2 {
        Ok(_) => println!("✓ '{{#each' works"),
        Err(e) => println!("✗ '{{#each' failed: {}", e),
    }
    
    // Test ws rule directly
    let result3 = LuatParser::parse(Rule::ws, " ");
    match result3 {
        Ok(_) => println!("✓ ws works"),
        Err(e) => println!("✗ ws failed: {}", e),
    }
    
    // Test expr rule directly 
    let result4 = LuatParser::parse(Rule::expr, "items");
    match result4 {
        Ok(_) => println!("✓ expr 'items' works"),
        Err(e) => println!("✗ expr 'items' failed: {}", e),
    }
    
    // Test ident rule directly
    let result5 = LuatParser::parse(Rule::ident, "item");
    match result5 {
        Ok(_) => println!("✓ ident 'item' works"),
        Err(e) => println!("✗ ident 'item' failed: {}", e),
    }
    
    // Test building up the each_start manually
    println!("\nTesting manual builds:");
    
    // Try just "{#each "
    let test1 = "{#each ";
    let result_test1 = LuatParser::parse(Rule::template, test1);
    println!("'{:?}' -> {:?}", test1, result_test1.is_ok());
    
    // Try "{#each items"
    let test2 = "{#each items";
    let result_test2 = LuatParser::parse(Rule::template, test2);
    println!("'{:?}' -> {:?}", test2, result_test2.is_ok());
    
    // Try "{#each items "
    let test3 = "{#each items ";
    let result_test3 = LuatParser::parse(Rule::template, test3);
    println!("'{:?}' -> {:?}", test3, result_test3.is_ok());
    
    // Try "{#each items as"
    let test4 = "{#each items as";
    let result_test4 = LuatParser::parse(Rule::template, test4);
    println!("'{:?}' -> {:?}", test4, result_test4.is_ok());
    
    // Try "{#each items as "
    let test5 = "{#each items as ";
    let result_test5 = LuatParser::parse(Rule::template, test5);
    println!("'{:?}' -> {:?}", test5, result_test5.is_ok());
    
    // Try "{#each items as item"
    let test6 = "{#each items as item";
    let result_test6 = LuatParser::parse(Rule::template, test6);
    println!("'{:?}' -> {:?}", test6, result_test6.is_ok());
    
    // Try "{#each items as item}"
    let test7 = "{#each items as item}";
    let result_test7 = LuatParser::parse(Rule::each_start, test7);
    println!("'{:?}' with each_start -> {:?}", test7, result_test7.is_ok());
}

#[test]
fn test_render_children_parsing() {
    let source = r#"{@render children?.()}"#;
    println!("Testing render children: {}", source);
    
    let result = parse_template(source);
    match result {
        Ok(ast) => {
            println!("Render children parsed successfully: {:?}", ast);
        }
        Err(e) => {
            println!("Render children parse error: {:?}", e);
        }
    }
}

#[test] 
fn test_render_children_step_by_step() {
    // Test render_children rule directly (removed render_expr test)
    let result1 = LuatParser::parse(Rule::render_children, "{@render children?.()}");
    match result1 {
        Ok(_) => println!("✓ render_children rule works"),
        Err(e) => println!("✗ render_children rule failed: {}", e),
    }
    
    // Test basic ident
    let result4 = LuatParser::parse(Rule::ident, "children");
    match result4 {
        Ok(_) => println!("✓ ident 'children' works"),
        Err(e) => println!("✗ ident 'children' failed: {}", e),
    }
    
    // Test optional_call
    let result5 = LuatParser::parse(Rule::optional_call, "?");
    match result5 {
        Ok(_) => println!("✓ optional_call works"),
        Err(e) => println!("✗ optional_call failed: {}", e),
    }
}

#[test]
fn test_expr_parsing() {
    // Test simple identifier
    let result1 = LuatParser::parse(Rule::expr, "children");
    match result1 {
        Ok(_) => println!("✓ expr 'children' works"),
        Err(e) => println!("✗ expr 'children' failed: {}", e),
    }
    
    // Test identifier with question mark - this should fail because ? is not part of expr
    let result2 = LuatParser::parse(Rule::expr, "children?");
    match result2 {
        Ok(_) => println!("✓ expr 'children?' works"),
        Err(e) => println!("✗ expr 'children?' failed: {}", e),
    }
}

#[test]
fn test_render_children_debug() {
    // Check if it's being parsed as something else first
    let result4 = LuatParser::parse(Rule::mustache, "{@render children?.()}");
    println!("mustache rule -> {:?}", result4.is_ok());
    
    let result5 = LuatParser::parse(Rule::raw_html, "{@render children?.()}");
    println!("raw_html rule -> {:?}", result5.is_ok());
}

#[test]
fn test_render_children_parts() {
    // Test the exact syntax step by step by building the pattern
    println!("Testing render_children components...");
    
    // Test just the beginning part
    let test1 = "{@render";
    let result1 = LuatParser::parse(Rule::template, test1);
    println!("'{{@render' partial -> {:?}", result1.is_ok());
    
    // Try the whole thing but use different rule
    let full_input = "{@render children?.()}";
    
    // Try parsing as raw HTML (which is similar)
    let result_raw = LuatParser::parse(Rule::raw_html, full_input);
    println!("raw_html attempt -> {:?}", result_raw.is_ok());
    
    // Let's see if raw_html works on simpler case
    let simple_raw = "{@html expr}";
    let result_simple_raw = LuatParser::parse(Rule::raw_html, simple_raw);
    println!("simple raw_html -> {:?}", result_simple_raw.is_ok());
    
    // Let's try with a simpler render_children first
    let simple_render = "{@render children()}";
    let result_simple_render = LuatParser::parse(Rule::render_children, simple_render);
    println!("simple render_children (no ?) -> {:?}", result_simple_render.is_ok());
}

#[test]
fn test_optional_call_position() {
    // Test if the issue is with optional_call position
    let simple_render = "{@render children()}";
    let result1 = LuatParser::parse(Rule::render_children, simple_render);
    println!("render without ? -> {:?}", result1.is_ok());
    
    // Test our current format
    let current_render = "{@render children?.()}";
    let result3 = LuatParser::parse(Rule::render_children, current_render);
    println!("render with ? attached -> {:?}", result3.is_ok());
}
