// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use pest::Parser;
use crate::parser::LuatParser;
use crate::parser::Rule;
use crate::parser;

#[test]
fn test_script_string_parsing() {
    let input = r#"<script>
        local Card = require("Card.luat")
        local hello = "<script>alert('Hello')</script>"
        local ok = _state(false)
        </script>"#;
    
    // First try parsing with script_content rule
    println!("Parsing with script_content rule:");
    let content_result = LuatParser::parse(Rule::script_content, &input[8..input.len()-9]);
    match content_result {
        Ok(pairs) => {
            for pair in pairs {
                println!("Content Rule: {:?}", pair.as_rule());
                println!("Content: {:?}", pair.as_str());
            }
        },
        Err(e) => {
            println!("Content parse error: {}", e);
        }
    }

    // Now try the full script_regular rule
    println!("\nParsing with script_regular rule:");
    let result = LuatParser::parse(Rule::script_regular, input);
    match result {
        Ok(pairs) => {
            for pair in pairs {
                println!("Rule: {:?}", pair.as_rule());
                
                for inner_pair in pair.into_inner() {
                    println!("Inner rule: {:?}", inner_pair.as_rule());
                    
                    if inner_pair.as_rule() == Rule::script_content {
                        let content = inner_pair.as_str().trim();
                        println!("Content length: {}", content.len());
                        println!("Content: {:?}", content);
                        
                        // Try to validate it as Lua
                        let lua_result = LuatParser::parse(Rule::lua_block, content);
                        match lua_result {
                            Ok(_) => println!("Valid Lua!"),
                            Err(e) => println!("Invalid Lua: {}", e),
                        }
                    }
                }
            }
        },
        Err(e) => {
            println!("Parse error: {}", e);
        }
    }
    
    // Try parsing the template to see extract_script_content results
    println!("\nParsing full template:");
    let template = format!("{}{}", input, "<p>Test</p>");
    match parser::parse_template(&template) {
        Ok(ast) => {
            println!("Template parse successful!");
            println!("Has regular script: {}", ast.regular_script.is_some());
        },
        Err(e) => {
            println!("Template parse error: {}", e);
        }
    }
}
