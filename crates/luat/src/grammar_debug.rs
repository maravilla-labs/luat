// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod grammar_debug {
    use crate::parser::*;
    use pest::Parser;

    #[test]
    fn test_simple_attribute() {
        let input = r#"class="container""#;
        let result = LuatParser::parse(Rule::attribute, input);
        match result {
            Ok(pairs) => {
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    for inner in pair.into_inner() {
                        println!("  Inner: {:?} -> {}", inner.as_rule(), inner.as_str());
                        for inner2 in inner.into_inner() {
                            println!("    Inner2: {:?} -> {}", inner2.as_rule(), inner2.as_str());
                        }
                    }
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_simple_div() {
        let input = r#"<div class="container">Hello</div>"#;
        let result = LuatParser::parse(Rule::template, input);
        match result {
            Ok(pairs) => {
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_opening_tag() {
        let input = r#"<div class="container">"#;
        let result = LuatParser::parse(Rule::opening_tag, input);
        match result {
            Ok(pairs) => {
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_element_or_component() {
        let input = r#"<div class="container">Hello</div>"#;
        let result = LuatParser::parse(Rule::element_or_component_node, input);
        match result {
            Ok(pairs) => {
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_each_block_rule2() {
        let input = r#"{#each items as item}Hello {item}{/each}"#;
        let result = LuatParser::parse(Rule::each_block, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_block parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
                panic!("Each block should parse successfully");
            }
        }
    }

    #[test]
    fn test_each_start() {
        let input = r#"{#each items as item}"#;
        let result = LuatParser::parse(Rule::each_start, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_start parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for each_start: {}", e);
            }
        }
    }

    #[test]
    fn test_each_end() {
        let input = r#"{/each}"#;
        let result = LuatParser::parse(Rule::each_end, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_end parsed correctly");
            }
            Err(e) => {
                println!("Parse error for each_end: {}", e);
            }
        }
    }

    #[test]
    fn test_mustache_simple() {
        let input = r#"{item}"#;
        let result = LuatParser::parse(Rule::mustache, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: mustache parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for mustache: {}", e);
            }
        }
    }

    #[test]
    fn test_simple_each_block() {
        let input = r#"{#each items as item}{/each}"#;
        let result = LuatParser::parse(Rule::each_block, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: simple each_block parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for simple each_block: {}", e);
            }
        }
    }

    #[test]
    fn test_text_hello() {
        let input = r#"Hello"#;
        let result = LuatParser::parse(Rule::text, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: text 'Hello' parsed correctly");
            }
            Err(e) => {
                println!("Parse error for text: {}", e);
            }
        }
    }

    #[test]
    fn test_node_hello() {
        let input = r#"Hello"#;
        let result = LuatParser::parse(Rule::template_node, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: node 'Hello' parsed correctly");
            }
            Err(e) => {
                println!("Parse error for node: {}", e);
            }
        }
    }

    #[test]
    fn test_each_with_text() {
        let input = r#"{#each items as item}Hello{/each}"#;
        let result = LuatParser::parse(Rule::each_block, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_block with text parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for each_block with text: {}", e);
            }
        }
    }

    #[test]
    fn test_each_with_mustache() {
        let input = r#"{#each items as item}{item}{/each}"#;
        let result = LuatParser::parse(Rule::each_block, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_block with mustache parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for each_block with mustache: {}", e);
            }
        }
    }

    #[test]
    fn test_each_with_space() {
        let input = r#"{#each items as item}Hello {item}{/each}"#;
        let result = LuatParser::parse(Rule::each_block, input);
        match result {
            Ok(pairs) => {
                println!("SUCCESS: each_block with space parsed correctly");
                for pair in pairs {
                    println!("Pair: {:?} -> {}", pair.as_rule(), pair.as_str());
                    print_pairs(pair, 1);
                }
            }
            Err(e) => {
                println!("Parse error for each_block with space: {}", e);
            }
        }
    }

    fn print_pairs(pair: pest::iterators::Pair<Rule>, indent: usize) {
        let prefix = "  ".repeat(indent);
        for inner in pair.into_inner() {
            println!("{}Inner: {:?} -> {}", prefix, inner.as_rule(), inner.as_str());
            print_pairs(inner, indent + 1);
        }
    }
}
