// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod simple_tests {
    use crate::*;
    use crate::parser::parse_template;

    #[test]
    fn test_basic_parsing() {
        let source = "Hello World";
        let result = parse_template(source);
        
        if let Err(e) = &result {
            println!("Parse error: {:?}", e);
        }
        
        assert!(result.is_ok());
        
        let ast = result.unwrap();
        assert_eq!(ast.body.len(), 1);
        
        if let Node::TextNode { content } = &ast.body[0] {
            assert_eq!(content, "Hello World");
        } else {
            panic!("Expected text node");
        }
    }

    #[test]
    fn test_basic_mustache() {
        let source = "Hello {name}";
        let result = parse_template(source);
        assert!(result.is_ok());
        
        let ast = result.unwrap();
        println!("AST: {:?}", ast);
        assert_eq!(ast.body.len(), 2);
        
        // First should be text "Hello " (including the space before mustache)
        if let Node::TextNode { content } = &ast.body[0] {
            println!("First node content: '{}'", content);
            assert_eq!(content, "Hello ");
        } else {
            panic!("Expected text node, got {:?}", ast.body[0]);
        }
        
        // Second should be mustache {name}
        if let Node::MustacheNode { expression } = &ast.body[1] {
            assert_eq!(expression.content, "name");
        } else {
            panic!("Expected mustache node, got {:?}", ast.body[1]);
        }
    }

    #[test]
    fn test_simple_element() {
        let source = r#"<div>content</div>"#;
        let result = parse_template(source);
        assert!(result.is_ok());
        
        let ast = result.unwrap();
        assert_eq!(ast.body.len(), 1);
        
        if let Node::ElementNode { tag, children, .. } = &ast.body[0] {
            assert_eq!(tag, "div");
            assert_eq!(children.len(), 1);
            
            if let Node::TextNode { content } = &children[0] {
                assert_eq!(content, "content");
            } else {
                panic!("Expected text node in element");
            }
        } else {
            panic!("Expected element node, got {:?}", ast.body[0]);
        }
    }
    
    #[test]
    fn test_if_block() {
        // Try a very simple if block first
        let source = "{#if true}test{/if}";
        let result = parse_template(source);
        
        if let Err(e) = &result {
            println!("Parse error: {:?}", e);
        }
        
        // If this fails, let's try without the complex nested node structure
        // and just manually test the grammar rules
        if result.is_err() {
            println!("Simple if block failed, trying even simpler...");
            
            // Test if the issue is with the recursive node definition
            let source2 = "test";
            let result2 = parse_template(source2);
            println!("Simple text parse result: {:?}", result2.is_ok());
        }
        
        assert!(result.is_ok(), "If block parsing should work");
        
        let ast = result.unwrap();
        assert_eq!(ast.body.len(), 1);
        
        if let Node::IfBlock { condition, then_branch, .. } = &ast.body[0] {
            assert_eq!(condition.content, "true");
            assert_eq!(then_branch.len(), 1);
            
            if let Node::TextNode { content } = &then_branch[0] {
                assert_eq!(content, "test");
            } else {
                panic!("Expected text node in if block");
            }
        } else {
            panic!("Expected if block, got {:?}", ast.body[0]);
        }
    }
}
