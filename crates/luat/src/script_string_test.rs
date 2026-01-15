// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use crate::parser::parse_template;
    #[allow(unused_imports)]
    use crate::ast::{ScriptBlock, ScriptType, LuatMagicFunction};
    #[allow(unused_imports)]
    use pest::Parser;
    #[allow(unused_imports)]
    use crate::parser::LuatParser;
    #[allow(unused_imports)]
    use crate::parser::Rule;

    // TODO; this has been intentionally disabled for now
    // because the script processor is not yet implemented like that.
    // It will be implemented in the future.
    #[allow(dead_code)]
    fn test_script_with_script_tags_in_strings() {
        let template = r#"<script>
local Card = require("Card.luat")
local hello = "scriptalert('Hello')/script"
local ok = $state(false)
let conditionalok = $state(hello, "no value")
</script>
<Card title="Confirmation">
    <p>Are you sure?</p>
</Card>"#;

        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template: {:?}", result.err());
        
        let ast = result.unwrap();
        assert!(ast.regular_script.is_some(), "Missing regular script block");
        
        let script = ast.regular_script.unwrap();
        assert!(script.content.contains("<script>alert('Hello')</script>"), 
            "Script content doesn't contain the expected string literal with script tags");
    }

    #[test]
    fn test_lua_grammar_with_luat_magic_functions() {
        // Test basic magic function
        let template = r#"<script>
        local ok = $state("hello world", "default value")
</script>
<div>Hello world</div>"#;
        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template with basic magic function: {:?}", result.err());
        
        let ast = result.unwrap();
        assert!(ast.regular_script.is_some(), "Missing regular script block");
        
        let script = ast.regular_script.unwrap();
        println!("Script content: {}", script.content);
        // The script processor should have transformed the magic function
        assert!(script.content.contains("-- LUAT magic function $state will be implemented in future"), 
            "Script content doesn't contain the expected LUAT magic function comment: {}", script.content);
        
        // Test magic function with a string
        let template = r#"<script>local hello = $state("hello world")
</script>
<div>Hello world</div>"#;
        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template with magic function and string: {:?}", result.err());
        
        // Test magic function with a variable
        let template = r#"<script>local result = $state(someVariable)
</script>
<div>Hello world</div>"#;
        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template with magic function and variable: {:?}", result.err());
        
        // Test magic function with default value
        let template = r#"<script>let conditionalok = $state(hello, "no value")
</script>
<div>Hello world</div>"#;
        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template with magic function and default: {:?}", result.err());
        
        // Check that the default value is properly transformed
        let ast = result.unwrap();
        let script = ast.regular_script.unwrap();
        assert!(script.content.contains("hello or \"no value\""), 
            "Script content doesn't contain the expected default value handling: {}", script.content);
        
        // Test empty magic function
        let template = r#"<script>local something = $init()
</script>
<div>Hello world</div>"#;
        let result = parse_template(template);
        assert!(result.is_ok(), "Failed to parse template with empty magic function: {:?}", result.err());
    }
}
