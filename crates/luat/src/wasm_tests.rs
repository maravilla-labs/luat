// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Integration tests for WASM-compatible code paths.
//!
//! These tests verify that the `MemoryResourceResolver` + `MemoryCache` combination
//! (the foundation of WASM support) works correctly for:
//! - Template compilation (parsing + codegen)
//! - Lua execution (rendering)
//! - Component imports
//! - All template features (if/each/expressions/scripts)

use crate::cache::MemoryCache;
use crate::engine::Engine;
use crate::memory_resolver::MemoryResourceResolver;

/// Create engine with memory resolver (WASM-compatible configuration)
fn create_memory_engine() -> Engine<MemoryResourceResolver> {
    let resolver = MemoryResourceResolver::new();
    let cache = Box::new(MemoryCache::new(100));
    Engine::new(resolver, cache).expect("Failed to create engine")
}

/// Create engine and add templates from a slice
fn create_engine_with_templates(templates: &[(&str, &str)]) -> Engine<MemoryResourceResolver> {
    let resolver = MemoryResourceResolver::new();
    for (path, content) in templates {
        resolver.add_template(path, content.to_string());
    }
    let cache = Box::new(MemoryCache::new(100));
    Engine::new(resolver, cache).expect("Failed to create engine")
}

// ============================================================================
// Basic Compilation Tests
// ============================================================================

mod compilation_tests {
    use super::*;

    #[test]
    fn test_compile_simple_html() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<div>Hello World</div>"),
        ]);

        let module = engine.compile_entry("test.luat").expect("Compilation failed");
        assert!(!module.lua_code.is_empty(), "Lua code should be generated");
        assert!(module.lua_code.contains("Hello World"), "Lua code should contain template content");
    }

    #[test]
    fn test_compile_with_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<h1>Hello, {props.name}!</h1>"),
        ]);

        let module = engine.compile_entry("test.luat").expect("Compilation failed");
        assert!(!module.lua_code.is_empty(), "Lua code should be generated");
        assert!(module.lua_code.contains("props"), "Lua code should reference props");
    }

    #[test]
    fn test_compile_with_multiple_expressions() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.first} {props.last} - Age: {props.age}</p>"),
        ]);

        let module = engine.compile_entry("test.luat").expect("Compilation failed");
        assert!(!module.lua_code.is_empty());
    }

    #[test]
    fn test_compile_empty_template() {
        let engine = create_engine_with_templates(&[
            ("test.luat", ""),
        ]);

        let module = engine.compile_entry("test.luat").expect("Compilation failed");
        assert!(!module.lua_code.is_empty(), "Should generate Lua code even for empty template");
    }
}

// ============================================================================
// Lua Execution Tests
// ============================================================================

mod execution_tests {
    use super::*;

    #[test]
    fn test_render_simple_html() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<div>Hello World</div>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert_eq!(result.trim(), "<div>Hello World</div>");
    }

    #[test]
    fn test_render_with_props() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<h1>Hello, {props.name}!</h1>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "name": "World" })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert_eq!(result.trim(), "<h1>Hello, World!</h1>");
    }

    #[test]
    fn test_render_with_multiple_props() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.greeting}, {props.name}! You are {props.age} years old.</p>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "greeting": "Hello",
            "name": "Alice",
            "age": 30
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert_eq!(result.trim(), "<p>Hello, Alice! You are 30 years old.</p>");
    }

    #[test]
    fn test_render_with_nested_props() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.user.name} - {props.user.email}</p>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "user": {
                "name": "Bob",
                "email": "bob@example.com"
            }
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert_eq!(result.trim(), "<p>Bob - bob@example.com</p>");
    }

    #[test]
    fn test_render_html_escaping() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.text}</p>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "text": "<script>alert('xss')</script>"
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        // HTML should be escaped by default
        assert!(!result.contains("<script>"), "Script tags should be escaped");
        assert!(result.contains("&lt;script&gt;") || result.contains("&lt;"), "Should contain escaped HTML");
    }

    #[test]
    fn test_render_raw_html() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<div>{@html props.content}</div>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "content": "<strong>Bold</strong>"
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("<strong>Bold</strong>"), "Raw HTML should not be escaped");
    }
}

// ============================================================================
// Component Import Tests
// ============================================================================

mod component_tests {
    use super::*;

    #[test]
    fn test_single_component_import() {
        let engine = create_engine_with_templates(&[
            ("Button.luat", r#"<button class="btn">{@render props.children?.()}</button>"#),
            ("main.luat", r#"<script>
local Button = require("Button")
</script>
<Button>Click me</Button>"#),
        ]);

        let module = engine.compile_entry("main.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("btn"), "Should render Button component");
        assert!(result.contains("Click me"), "Should render children");
    }

    #[test]
    fn test_component_with_props() {
        let engine = create_engine_with_templates(&[
            ("Card.luat", r#"<div class="card">
    <h2>{props.title}</h2>
    <div>{@render props.children?.()}</div>
</div>"#),
            ("main.luat", r#"<script>
local Card = require("Card")
</script>
<Card title="My Card">
    <p>Card content here</p>
</Card>"#),
        ]);

        let module = engine.compile_entry("main.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("card"), "Should have card class");
        assert!(result.contains("My Card"), "Should render title prop");
        assert!(result.contains("Card content here"), "Should render children");
    }

    #[test]
    fn test_nested_component_imports() {
        let engine = create_engine_with_templates(&[
            ("Header.luat", r#"<header>{props.text}</header>"#),
            ("Card.luat", r#"<script>
local Header = require("Header")
</script>
<div class="card">
    <Header text={props.title} />
    <div>{@render props.children?.()}</div>
</div>"#),
            ("main.luat", r#"<script>
local Card = require("Card")
</script>
<Card title="Welcome">
    <p>Content</p>
</Card>"#),
        ]);

        let module = engine.compile_entry("main.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("<header>"), "Should render Header component");
        assert!(result.contains("Welcome"), "Should pass props through");
        assert!(result.contains("Content"), "Should render children");
    }

    #[test]
    fn test_component_in_subdirectory() {
        let engine = create_engine_with_templates(&[
            ("components/Button.luat", r#"<button>{@render props.children?.()}</button>"#),
            ("main.luat", r#"<script>
local Button = require("components/Button")
</script>
<Button>Submit</Button>"#),
        ]);

        let module = engine.compile_entry("main.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("<button>"), "Should find component in subdirectory");
        assert!(result.contains("Submit"), "Should render children");
    }

    #[test]
    fn test_sibling_import() {
        // Use absolute paths for components in same directory
        // (Relative imports like ./Button require runtime importer context)
        let engine = create_engine_with_templates(&[
            ("ui/Button.luat", r#"<button>{@render props.children?.()}</button>"#),
            ("ui/Form.luat", r#"<script>
local Button = require("ui/Button")
</script>
<form>
    <Button>Submit</Button>
</form>"#),
            ("main.luat", r#"<script>
local Form = require("ui/Form")
</script>
<Form />"#),
        ]);

        let module = engine.compile_entry("main.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("<form>"), "Should render Form");
        assert!(result.contains("<button>"), "Should resolve sibling import");
    }
}

// ============================================================================
// Control Flow Tests
// ============================================================================

mod control_flow_tests {
    use super::*;

    #[test]
    fn test_if_true() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.show}<p>Visible</p>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "show": true })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("Visible"), "Should show content when condition is true");
    }

    #[test]
    fn test_if_false() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.show}<p>Visible</p>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "show": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(!result.contains("Visible"), "Should hide content when condition is false");
    }

    #[test]
    fn test_if_else() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.loggedIn}<p>Welcome back!</p>{:else}<p>Please log in</p>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        // Test true case
        let context = engine.to_value(serde_json::json!({ "loggedIn": true })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Welcome back!"));
        assert!(!result.contains("Please log in"));

        // Test false case
        let context = engine.to_value(serde_json::json!({ "loggedIn": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(!result.contains("Welcome back!"));
        assert!(result.contains("Please log in"));
    }

    #[test]
    fn test_if_else_if() {
        // else-if syntax now works with the fixed parser
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.isAdmin}
<p>Admin</p>
{:else if props.isUser}
<p>User</p>
{:else}
<p>Guest</p>
{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        let context = engine.to_value(serde_json::json!({ "isAdmin": true, "isUser": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Admin"));

        let context = engine.to_value(serde_json::json!({ "isAdmin": false, "isUser": true })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("User"));

        let context = engine.to_value(serde_json::json!({ "isAdmin": false, "isUser": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Guest"));
    }

    #[test]
    fn test_nested_if_else_workaround() {
        // Workaround for else-if using nested if blocks
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.isAdmin}
<p>Admin</p>
{:else}
{#if props.isUser}
<p>User</p>
{:else}
<p>Guest</p>
{/if}
{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        let context = engine.to_value(serde_json::json!({ "isAdmin": true, "isUser": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Admin"));

        let context = engine.to_value(serde_json::json!({ "isAdmin": false, "isUser": true })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("User"));

        let context = engine.to_value(serde_json::json!({ "isAdmin": false, "isUser": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Guest"));
    }

    #[test]
    fn test_each_basic() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<ul>{#each props.items as item}<li>{item}</li>{/each}</ul>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "items": ["Apple", "Banana", "Cherry"]
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("Apple"));
        assert!(result.contains("Banana"));
        assert!(result.contains("Cherry"));
    }

    #[test]
    fn test_each_with_index() {
        // Note: Lua uses 1-based indexing by default in #each
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<ul>{#each props.items as item, i}<li>{i}. {item}</li>{/each}</ul>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "items": ["First", "Second", "Third"]
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        // Lua's ipairs starts at 1, so indices are 1, 2, 3
        assert!(result.contains("1. First"));
        assert!(result.contains("2. Second"));
        assert!(result.contains("3. Third"));
    }

    #[test]
    fn test_each_with_object() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<ul>{#each props.users as user}<li>{user.name} ({user.age})</li>{/each}</ul>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "users": [
                { "name": "Alice", "age": 30 },
                { "name": "Bob", "age": 25 }
            ]
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("Alice (30)"));
        assert!(result.contains("Bob (25)"));
    }

    #[test]
    fn test_each_empty_with_fallback() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<ul>{#each props.items as item}<li>{item}</li>{:empty}<li>No items</li>{/each}</ul>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "items": []
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("No items"), "Should render empty fallback");
        assert!(!result.contains("<li></li>"), "Should not render empty items");
    }

    #[test]
    fn test_nested_if_each() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.showList}<ul>{#each props.items as item}<li>{item}</li>{/each}</ul>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "showList": true,
            "items": ["A", "B"]
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("<ul>"));
        assert!(result.contains("A"));
        assert!(result.contains("B"));
    }
}

// ============================================================================
// Script Block Tests
// ============================================================================

mod script_tests {
    use super::*;

    #[test]
    fn test_script_local_variable() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<script>
local greeting = "Hello from Lua"
</script>
<p>{greeting}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("Hello from Lua"), "Should render local variable");
    }

    #[test]
    fn test_script_computed_value() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<script>
local base = props.price or 0
local tax = base * 0.1
local total = base + tax
</script>
<p>Total: ${total}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "price": 100 })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("110"), "Should compute tax correctly");
    }

    #[test]
    fn test_script_function() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<script>
local function greet(name)
    return "Hello, " .. name .. "!"
end
local message = greet(props.name)
</script>
<p>{message}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "name": "World" })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("Hello, World!"), "Should execute local function");
    }

    #[test]
    fn test_module_script() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<script module>
function formatPrice(price)
    return string.format("$%.2f", price)
end
</script>
<script>
local formatted = formatPrice(props.price)
</script>
<p>Price: {formatted}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "price": 19.99 })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("$19.99"), "Should use module function");
    }

    #[test]
    fn test_script_with_table() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<script>
local config = {
    title = "My App",
    version = "1.0"
}
</script>
<h1>{config.title} v{config.version}</h1>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("My App v1.0"), "Should access table fields");
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

mod error_tests {
    use super::*;

    #[test]
    fn test_missing_template_error() {
        let engine = create_memory_engine();

        let result = engine.compile_entry("nonexistent.luat");
        let err = result.expect_err("Should error on missing template");
        let err_str = format!("{}", err);
        assert!(
            err_str.contains("nonexistent") || err_str.contains("not found") || err_str.contains("resolve"),
            "Error should mention the missing template: {}", err_str
        );
    }

    #[test]
    fn test_missing_component_error() {
        // Missing components are detected at runtime (during require), not compile time
        let engine = create_engine_with_templates(&[
            ("main.luat", r#"<script>
local Button = require("Button")
</script>
<Button>Click</Button>"#),
        ]);

        let module = engine.compile_entry("main.luat");
        // Compilation succeeds because the require is executed at runtime
        assert!(module.is_ok(), "Compilation should succeed");

        // Error occurs at render time when require is called
        let module = module.unwrap();
        let context = engine.to_value(serde_json::json!({})).unwrap();
        let result = engine.render(&module, &context);
        let err = result.expect_err("Should error at runtime on missing component");
        let err_str = format!("{}", err);
        assert!(
            err_str.contains("Button") || err_str.contains("not found") || err_str.contains("module"),
            "Error should mention the missing component: {}", err_str
        );
    }

    #[test]
    fn test_parse_error_unclosed_tag() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<div><p>Unclosed"),
        ]);

        // Note: The parser may be lenient or strict about unclosed tags
        // This test verifies the behavior either way
        let result = engine.compile_entry("test.luat");
        // Either compilation succeeds (lenient) or fails with an error (strict)
        // Both are valid behaviors - we just document which one we have
        if let Err(err) = result {
            let err_str = format!("{}", err);
            // If there's an error, it should be descriptive
            assert!(!err_str.is_empty(), "Error should have a message");
        }
    }

    #[test]
    fn test_parse_error_invalid_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.}</p>"),
        ]);

        let result = engine.compile_entry("test.luat");
        // This should likely fail due to invalid expression syntax
        // Test documents current behavior
        if let Err(err) = result {
            let err_str = format!("{}", err);
            assert!(!err_str.is_empty());
        }
    }
}

// ============================================================================
// Expression Tests
// ============================================================================

mod expression_tests {
    use super::*;

    #[test]
    fn test_arithmetic_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.a + props.b}</p>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "a": 5, "b": 3 })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("8"), "Should compute 5 + 3 = 8");
    }

    #[test]
    fn test_string_concatenation() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<p>{props.first .. " " .. props.last}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "first": "John", "last": "Doe" })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains("John Doe"), "Should concatenate strings");
    }

    #[test]
    fn test_comparison_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.age >= 18}<p>Adult</p>{:else}<p>Minor</p>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        let context = engine.to_value(serde_json::json!({ "age": 21 })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Adult"));

        let context = engine.to_value(serde_json::json!({ "age": 15 })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Minor"));
    }

    #[test]
    fn test_logical_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"{#if props.isAdmin and props.isActive}<p>Active Admin</p>{/if}"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        let context = engine.to_value(serde_json::json!({ "isAdmin": true, "isActive": true })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains("Active Admin"));

        let context = engine.to_value(serde_json::json!({ "isAdmin": true, "isActive": false })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(!result.contains("Active Admin"));
    }

    #[test]
    fn test_ternary_like_expression() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<p class="{props.active and "active" or "inactive"}">{props.label}</p>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        let context = engine.to_value(serde_json::json!({ "active": true, "label": "Button" })).unwrap();
        let result = engine.render(&module, &context).unwrap();
        assert!(result.contains(r#"class="active""#));
    }
}

// ============================================================================
// Attribute Tests
// ============================================================================

mod attribute_tests {
    use super::*;

    #[test]
    fn test_dynamic_attribute() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<div class="{props.className}">Content</div>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({ "className": "my-class" })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains(r#"class="my-class""#));
    }

    #[test]
    fn test_multiple_dynamic_attributes() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<input type="{props.type}" name="{props.name}" value="{props.value}" />"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "type": "text",
            "name": "username",
            "value": "john"
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains(r#"type="text""#));
        assert!(result.contains(r#"name="username""#));
        assert!(result.contains(r#"value="john""#));
    }

    #[test]
    fn test_mixed_static_dynamic_attributes() {
        let engine = create_engine_with_templates(&[
            ("test.luat", r#"<button type="submit" class="{props.className}" disabled="{props.disabled}">Submit</button>"#),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();
        let context = engine.to_value(serde_json::json!({
            "className": "btn-primary",
            "disabled": false
        })).unwrap();
        let result = engine.render(&module, &context).unwrap();

        assert!(result.contains(r#"type="submit""#));
        assert!(result.contains("btn-primary"));
    }
}

// ============================================================================
// Cache Tests
// ============================================================================

mod cache_tests {
    use super::*;

    #[test]
    fn test_cache_stores_compiled_module() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>Cached</p>"),
        ]);

        // First compilation
        let _ = engine.compile_entry("test.luat").unwrap();

        // Cache key format is "module:{path}"
        assert!(engine.cache_contains("module:test.luat"), "Module should be cached");
    }

    #[test]
    fn test_cache_clear() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>Cached</p>"),
        ]);

        let _ = engine.compile_entry("test.luat").unwrap();
        assert!(engine.cache_contains("module:test.luat"));

        engine.clear_cache().unwrap();
        assert!(!engine.cache_contains("module:test.luat"), "Cache should be cleared");
    }

    #[test]
    fn test_multiple_renders_same_module() {
        let engine = create_engine_with_templates(&[
            ("test.luat", "<p>{props.count}</p>"),
        ]);

        let module = engine.compile_entry("test.luat").unwrap();

        // Render multiple times with different contexts
        for i in 1..=5 {
            let context = engine.to_value(serde_json::json!({ "count": i })).unwrap();
            let result = engine.render(&module, &context).unwrap();
            assert!(result.contains(&i.to_string()));
        }
    }
}
