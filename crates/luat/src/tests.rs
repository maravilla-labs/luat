// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::parser::parse_template;
use crate::*;
use mlua::{Lua, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// Helper function to create string values for tests
// Note: This creates values in a separate Lua state - use engine.create_string() instead
#[allow(dead_code)]
fn string_value(s: &str) -> Value {
    // This is problematic - creates values in different Lua state
    let lua = Lua::new();
    Value::String(lua.create_string(s).unwrap())
}

// Helper function to create an engine with memory cache for tests
fn create_engine<P: AsRef<std::path::Path>>(root_dir: P) -> Result<Engine<FileSystemResolver>> {
    let resolver = FileSystemResolver::new(root_dir);
    Engine::with_memory_cache(resolver, 100)
}

// Helper function to create an engine with file system cache for tests
fn create_engine_with_cache<P: AsRef<std::path::Path>, C: AsRef<std::path::Path>>(
    root_dir: P,
    cache_dir: C,
) -> Result<Engine<FileSystemResolver>> {
    let resolver = FileSystemResolver::new(root_dir);
    Engine::with_filesystem_cache(resolver, cache_dir, 100)
}

#[cfg(test)]
mod spread_operator_tests {
    use super::*;
    #[test]
    fn test_spread_operator_with_table_props() {
        let temp_dir = TempDir::new().unwrap();

        // Button.luat: just renders the received props
        fs::write(
            temp_dir.path().join("Button.luat"),
            r#"
<button type={props.type}>{props.label}</button>
"#,
        )
        .unwrap();

        // Main.luat: defines buttonProps and uses the spread operator
        let main_template = r#"
<script>
    local Button = require("Button.luat")
    local buttonProps = { label = "Save", type = "button" }
</script>

<Button {...buttonProps} />
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let initial_map: HashMap<String, Value> = HashMap::new();
        let context = engine.to_value(initial_map).unwrap();
        let result = engine.render(&module, &context).unwrap();

        println!("Rendered output: {}", result);

        // Checks
        assert!(
            result.contains(r#"<button type="button">Save</button>"#),
            "Button props were not spread correctly"
        );
        assert!(result.contains("Save"), "Label was not rendered");
    }
    #[test]
    fn test_two_spread_operators_in_one_component() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(
            temp_dir.path().join("Button.luat"),
            r#"
<button type={props.type} data-x={props["data-x"]}>{props.label}</button>
"#,
        )
        .unwrap();

        let main_template = r#"
<script>
    local Button = require("Button.luat")
    local baseProps = { label = "Delete", type = "reset" }
    local dataProps = { ["data-x"] = "123" }
</script>

<Button {...baseProps} {...dataProps} />
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let initial_map: HashMap<String, Value> = HashMap::new();
        let context = engine.to_value(initial_map).unwrap();
        let result = engine.render(&module, &context).unwrap();

        println!("Rendered output: {}", result);

        assert!(
            result.contains(r#"<button type="reset" data-x="123">Delete</button>"#),
            "Button did not receive both spreads"
        );
    }

    #[test]
    fn test_spread_and_regular_props_mixed() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(
            temp_dir.path().join("Button.luat"),
            r#"
<button type={props.type} data-z={props["data-z"]}>{props.label}</button>
"#,
        )
        .unwrap();

        let main_template = r#"
<script>
    local Button = require("Button.luat")
    local baseProps = { label = "Go", type = "button" }
</script>

<Button {...baseProps} data-z="special" />
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let initial_map: HashMap<String, Value> = HashMap::new();
        let context = engine.to_value(initial_map).unwrap();
        let result = engine.render(&module, &context).unwrap();

        println!("Rendered output: {}", result);

        assert!(
            result.contains(r#"<button type="button" data-z="special">Go</button>"#),
            "Button did not merge spread and regular props"
        );
    }

    #[allow(dead_code)]
    fn test_spread_operator_overwrites_props() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(
            temp_dir.path().join("Button.luat"),
            r#"
<button type={props.type} data-x={props["data-x"]}>{props.label}</button>
"#,
        )
        .unwrap();

        let main_template = r#"
<script>
    local Button = require("Button.luat")
    local baseProps = { label = "Click", type = "button", ["data-x"] = "A" }
    local overrideProps = { type = "submit", ["data-x"] = "B" }
</script>

<Button {...baseProps} type="reset" {...overrideProps} label="Final" />
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let initial_map: HashMap<String, Value> = HashMap::new();
        let context = engine.to_value(initial_map).unwrap();
        let result = engine.render(&module, &context).unwrap();

        println!("Rendered output: {}", result);

        // The expected output:
        // type: "submit" (from overrideProps, last spread wins)
        // label: "Final" (explicit prop wins)
        // data-x: "B" (from overrideProps, last spread wins)
        assert!(
            result.contains(r#"<button type="submit" data-x="B">Final</button>"#),
            "Prop overwrite by order failed: last wins is not respected"
        );
    }
}

#[cfg(test)]
mod local_expression_tests {
    use super::*;

    #[test]
    fn test_local_tag_in_each_block() {
        let source = r#"
{#each boxes as box}
    {@local area = box.width * box.height}
    {box.width} * {box.height} = {area}
{/each}
"#;

        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        // Assert that area calculation is present in Lua code (adjust this for your IR)
        assert!(lua_code.contains("local area = box.width * box.height"));
        // Assert output string interpolates correctly
        assert!(lua_code.contains("smart_tostring(box.width)"));
        assert!(lua_code.contains("smart_tostring(box.height)"));
        assert!(lua_code.contains("smart_tostring(area)"));
    }

    #[test]
    fn test_local_tag_illegal_usage_outside_block() {
        let source = r#"
{@local foo = 42}
"#;
        let ast = parse_template(source).unwrap();
        let err = transform_ast(ast).unwrap_err();

        assert!(err
            .to_string()
            .contains("{@local} is only allowed as an immediate child of a block"));
    }
    #[test]
    fn test_multiple_locals_in_each_block() {
        let source = r#"
{#each boxes as box}
    {@local w = box.width}
    {@local h = box.height}
    {w} x {h}
{/each}
"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        assert!(lua_code.contains("local w = box.width"));
        assert!(lua_code.contains("local h = box.height"));
        assert!(lua_code.contains("smart_tostring(w)"));
        assert!(lua_code.contains("smart_tostring(h)"));
    }
    #[test]
    fn test_local_tag_in_component() {
        let source = r#"
<Component>
    {@local foo = 99}
    {foo}
</Component>
"#;
        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        assert!(lua_code.contains("local foo = 99"));
        assert!(lua_code.contains("smart_tostring(foo)"));
    }

    #[test]
    fn test_local_tag_in_negated_if_block() {
        let source = r#"
{!if notFlag}
    {@local tripled = value * 3}
    Tripled: {tripled}
{/if}
"#;

        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        // Should declare and use local in negated if block
        assert!(lua_code.contains("local tripled = value * 3"));
        assert!(lua_code.contains("smart_tostring(tripled)"));
    }
    #[test]
    fn test_local_tag_in_if_block() {
        let source = r#"
{#if flag}
    {@local doubled = value * 2}
    Doubled: {doubled}
{/if}
"#;

        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        // The Lua should declare local and use it
        assert!(lua_code.contains("local doubled = value * 2"));
        assert!(lua_code.contains("smart_tostring(doubled)"));
    }
}
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_pipeline_simple_template() {
        let source = r#"<div class="container">Hello {name}!</div>"#;

        // Parse
        let ast = parse_template(source).unwrap();
        assert_eq!(ast.body.len(), 1);

        // Transform
        let ir = transform_ast(ast).unwrap();
        validate_ir(&ir).unwrap();

        // Generate
        let lua_code = generate_lua_code(ir, "test").unwrap();
        assert!(lua_code.contains("function render"));
        assert!(lua_code.contains("html_escape"));
    }

    #[test]
    fn test_full_pipeline_with_components() {
        let temp_dir = TempDir::new().unwrap();

        // Create Card component
        let card_template = r#"
<script module>
    function getCardClass(variant)
        return "card card-" .. (variant or "default")
    end
</script>

<script>
    local variant = props and props.variant or "default"
    local class = getCardClass(variant)
    local title = props and props.title or ""
    local render_children = function()
        if props and props.children then
            return props.children()
        else
            return ""
        end
    end
</script>

<div class="{class}">
    <h2>{title}</h2>
    {@render render_children()}
</div>
"#;
        fs::write(temp_dir.path().join("Card.luat"), card_template).unwrap();

        // Create main template
        let main_template = r#"
<script>
    local Card = require("Card.luat")
</script>

<Card variant="primary" title="Welcome">
    <p>This is the card content</p>
</Card>
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let context = engine.to_value(HashMap::<String, String>::new()).unwrap();
        let result = engine.render(&module, &context).unwrap();

        // The result should contain the rendered component
        assert!(result.contains("card"));
        assert!(result.contains("Welcome"));
    }

    #[test]
    fn test_control_flow_if_block() {
        let source = r#"
{#if props.show}
    <div>Visible content</div>
{:else}
    <div>Hidden content</div>
{/if}
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        // Test with show = true
        let mut context = HashMap::new();
        context.insert("show".to_string(), Value::Boolean(true));
        let result = engine.render_source(source, &context).unwrap();
        println!("Rendered output: {}", result);
        assert!(result.contains("Visible content"));
        assert!(!result.contains("Hidden content"));

        // Test with show = false
        context.insert("show".to_string(), Value::Boolean(false));
        let result = engine.render_source(source, &context).unwrap();
        assert!(!result.contains("Visible content"));
        assert!(result.contains("Hidden content"));
    }

    #[test]
    fn test_control_flow_each_block() {
        let source = r#"
<ul>
{#each props.items as item, index}
    <li>{index + 1}: {item}</li>
{:empty}
    <li>No items</li>
{/each}
</ul>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        // Test with items
        let mut context = HashMap::new();

        // Create the items array in the engine's context
        let table = engine.create_table().unwrap();
        table.set(1, "Apple").unwrap();
        table.set(2, "Banana").unwrap();
        table.set(3, "Cherry").unwrap();
        let items_table = Value::Table(table);

        context.insert("items".to_string(), items_table);

        // First compile the template
        let module = engine.compile_template_string("test", source).unwrap();
        // println!("Lua code: {:?}", module);

        let context = engine.to_value(context).unwrap();
        // Then render using the module directly instead of render_source
        let result = engine.render(&module, &context).unwrap();
        println!("Generated output:\n{}", result);
        assert!(result.contains("2: Apple"));
        assert!(result.contains("3: Banana"));
        assert!(result.contains("4: Cherry"));
        assert!(!result.contains("No items"));

        // Test with empty items
        let items: Vec<String> = vec![];
        let mut empty_context: HashMap<String, Vec<String>> = HashMap::new();
        empty_context.insert("items".to_string(), items);

        let result = engine
            .render(&module, &engine.to_value(empty_context).unwrap())
            .unwrap();
        println!("Generated output with empty items:\n{}", result);
        assert!(result.contains("No items"));
        assert!(!result.contains("Apple"));
    }

    #[test]
    fn test_sensitive_blocks() {
        let source = r#"
{!if secret}
    <div>Secret content: {secretData}</div>
{/if}

{!each sensitiveItems as item}
    <div>Item: {item}</div>
{/each}
"#;

        let ast = parse_template(source).unwrap();
        let ir = transform_ast(ast).unwrap();
        let lua_code = generate_lua_code(ir, "test").unwrap();

        // Check that sensitive comments are generated
        assert!(lua_code.contains("<!-- sensitive -->"));
        assert!(lua_code.contains("-- sensitive"));
    }

    #[test]
    fn test_raw_html() {
        let source = r#"<div>{@html props.htmlContent}</div>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let mut context = HashMap::new();
        engine
            .insert_string(&mut context, "htmlContent", "<strong>Bold</strong>")
            .unwrap();

        let result = engine.render_source(source, &context).unwrap();
        println!("Rendered output: {}", result);
        assert!(result.contains("<strong>Bold</strong>"));
        assert!(!result.contains("&lt;strong&gt;"));
    }

    #[test]
    fn test_html_escaping() {
        let source = r#"<div>{props.content}</div>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let mut context = HashMap::new();
        engine
            .insert_string(&mut context, "content", "<script>alert('xss')</script>")
            .unwrap();

        let result = engine.render_source(source, &context).unwrap();
        assert!(result.contains("&lt;script&gt;"));
        assert!(!result.contains("<script>"));
    }

    #[test]
    fn test_attributes() {
        // Create a template with static attributes (no expressions)
        let source = r#"
<div class="static" id="my-id" data-test="value">
    Content
</div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();

        println!("Result: {}", result);
        assert!(result.contains("class=\"static\""));
        assert!(result.contains("id=\"my-id\""));
        assert!(result.contains("data-test=\"value\""));
    }

    #[test]
    fn test_attributes_and_expression() {
        // Create a template with static attributes (no expressions)
        let source = r#"
        <script>
            local clazz = "text-blue"
        </script>
<div class="static {clazz}" id="my-id" data-test="value">
    Content
</div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();

        println!("Result: {}", result);
        assert!(result.contains("class=\"static text-blue\""));
        assert!(result.contains("id=\"my-id\""));
        assert!(result.contains("data-test=\"value\""));
    }

    #[test]
    fn test_class_with_table_expression() {
        // Create a template with static attributes (no expressions)
        let source = r#"
        <div id="shit" class={
            {
        ["text-blue-200 bg-red-100"] = true,
        ["font-bold"] = false
    }
        }>
        shit
        </div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();

        println!("Result: {}", result);
        assert!(result.contains("class=\"text-blue-200 bg-red-100\""));
        assert!(!result.contains("font-bold"));
    }

    #[test]
    fn test_class_with_table_expression_reference() {
        // Create a template with static attributes (no expressions)
        let source = r#"
        <script>
            local classes = {
                ["text-blue-200 bg-red-100"] = true,
                ["font-bold"] = false
            }
        </script>
        <div id="shit" class={classes}>
        shit
        </div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();

        println!("Result: {}", result);
        assert!(result.contains("class=\"text-blue-200 bg-red-100\""));
        assert!(!result.contains("font-bold"));
    }

    #[test]
    fn test_script_with_expression() {
        let source = r#"

<script src={props.htmlsource} deffer></script>

"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        // Test with show = true
        let mut context = HashMap::new();
        engine
            .insert_string(&mut context, "htmlsource", "htmlink.js")
            .unwrap();
        let result = engine.render_source(source, &context).unwrap();
        println!("Rendered output: {}", result);
        assert!(result.contains("htmlink.js"));
        assert!(!result.contains("htmlsource"));
    }

    #[test]
    fn test_html_and_luat_comments() {
        let source = r#"<div><!-- Hello {props.name} -->{/* ignore */}</div>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let mut context = HashMap::new();
        engine.insert_string(&mut context, "name", "Bob").unwrap();

        let result = engine.render_source(source, &context).unwrap();
        assert!(result.contains("<!-- Hello Bob-->"));
        assert!(!result.contains("ignore"));
    }

    #[test]
    fn test_multiline_comment_with_curly_braces() {
        // Test that {} inside multi-line comments are NOT parsed
        let source = r#"
{/*
This comment contains {props.value} and {#if condition}
and even {/if} {#each items as item}{/each}
None of this should be parsed
*/}
<div>Hello</div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();
        // The comment should be removed, not cause parse errors
        assert!(result.contains("<div>Hello</div>"));
        assert!(!result.contains("props.value"));
        assert!(!result.contains("{#if"));
    }

    #[test]
    fn test_lua_style_line_comment() {
        // Test the new {-- comment --} syntax
        let source = r#"{-- This is a Lua-style comment --}<div>Content</div>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();
        assert!(result.contains("<div>Content</div>"));
        assert!(!result.contains("Lua-style comment"));
    }

    #[test]
    fn test_comment_as_first_line_before_script() {
        // Test that comments can appear before script blocks
        let source = r#"{/* Header comment */}
<script>
local x = 42
</script>
<div>{x}</div>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();
        assert!(result.contains("<div>42</div>"));
        assert!(!result.contains("Header comment"));
    }

    #[test]
    fn test_lua_line_comment_with_curly_braces() {
        // Test that {} inside line comments are NOT parsed
        let source = r#"{-- {props.value} {#if x}{/if} --}<span>OK</span>"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let context = HashMap::new();
        let result = engine.render_source(source, &context).unwrap();
        assert!(result.contains("<span>OK</span>"));
        assert!(!result.contains("props.value"));
    }

    #[test]
    fn test_nested_components() {
        let temp_dir = TempDir::new().unwrap();

        // Create Button component
        fs::write(
            temp_dir.path().join("Button.luat"),
            r#"        
<button class="btn">{@render props.children?.()}</button>
"#,
        )
        .unwrap();

        // Create Card component that uses Button
        fs::write(
            temp_dir.path().join("Card.luat"),
            r#"
<script>
    local Button = require("Button.luat")
</script>

<div class="card">
    <h3>{props.title}</h3>
    <div class="content">
        {@render props.children?.()}
    </div>
    <div class="actions">
        <Button>OK</Button>
    </div>
</div>
"#,
        )
        .unwrap();

        // Create main template
        let main_template = r#"
<script>
    local Button = require("Button.luat")
    local Card = require("Card.luat")
    local hello = "Hello"
    local ok = false
    local default = hello or "no value"
    local calculatedvalue = "Calculated: " .. tostring(ok)
</script>

<Card title="Confirmation">
    <p>Are you sure?</p>
</Card>
"#;
        fs::write(temp_dir.path().join("main.luat"), main_template).unwrap();

        let engine = create_engine(temp_dir.path()).unwrap();
        let module = engine.compile_entry("main.luat").unwrap();

        let initial_map: HashMap<String, Value> = HashMap::new();
        let context = engine.to_value(initial_map).unwrap();
        let result = engine.render(&module, &context).unwrap();

        println!("Rendered output: {}", result);
        assert!(result.contains("card"));
        assert!(result.contains("Confirmation"));
        assert!(result.contains("Are you sure?"));
        assert!(result.contains("btn"));
        assert!(result.contains("OK"));
    }

    #[test]
    fn test_script_blocks() {
        let source = r#"
<script module>
    function helper(name)
        return "Hello, " .. name .. "!"
    end
</script>

<script>
    local greeting = helper(props.name or "World")
    local testme = "hallo"
</script>

<div>
{greeting}
testlocal: {testme}
props name: {props.name}
</div>
"#;

        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let mut context = HashMap::new();
        engine.insert_string(&mut context, "name", "Alice").unwrap();
        let luaresult = engine.compile_template_string("Main", source);
        println!("Lua code: {:?}", luaresult);
        let result = engine.render_source(source, &context).unwrap();
        println!("Rendered output: {}", result);
        // Check for the content without requiring exact formatting
        assert!(result.contains("Hello, Alice"));
        assert!(result.contains("testlocal: hallo"));
    }

    #[test]
    fn test_cache_functionality() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();

        fs::write(temp_dir.path().join("test.luat"), "<div>Test</div>").unwrap();

        let engine = create_engine_with_cache(temp_dir.path(), cache_dir.path()).unwrap();

        // First compilation should cache the module
        let module1 = engine.compile_entry("test").unwrap();
        println!("Module hash: {:?}", module1);

        assert!(engine.cache_contains("module:test"));

        // Second compilation should use cached version
        let module2 = engine.compile_entry("test").unwrap();
        assert_eq!(module1.hash, module2.hash);

        // Clear cache
        engine.clear_cache().unwrap();
        assert!(!engine.cache_contains("module:test"));
    }

    #[test]
    fn test_bundle_generation() {
        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let sources = vec![
            (
                "Card".to_string(),
                r#"<div class="card">{@render children?.()}</div>"#.to_string(),
            ),
            (
                "Button".to_string(),
                r#"<button>{@render children?.()}</button>"#.to_string(),
            ),
            (
                "App".to_string(),
                r#"<Card><Button>Click me</Button></Card>"#.to_string(),
            ),
        ];

        let (bundle, _source_map) = engine.bundle_sources(sources, |_, _| {}).unwrap();
        println!("Bundle content:\n{}", bundle);
        assert!(bundle.contains("__module_loaders[\"Card\"]"));
        assert!(bundle.contains("__module_loaders[\"Button\"]"));
        assert!(bundle.contains("__module_loaders[\"App\"]"));
        assert!(bundle.contains("local function __require"));
    }

    #[test]
    fn test_error_handling() {
        // Test parse error
        let invalid_source = r#"<div>{unclosed"#;
        let result = parse_template(invalid_source);
        assert!(result.is_err());

        // Test invalid script placement
        let invalid_script = r#"
<div>Content</div>
<script module>
    -- This should fail because module script must be first
</script>
"#;
        let result = parse_template(invalid_script);
        assert!(result.is_err());

        // Test multiple module scripts
        let multiple_modules = r#"
<script module>
    -- First module script
</script>
<script module>
    -- Second module script (should fail)
</script>
"#;
        let result = parse_template(multiple_modules);
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_compilation_performance() {
        let source = r#"
<div class="container">
    {#each items as item, index}
        <div class="item">
            <h3>{item.title}</h3>
            <p>{item.description}</p>
            {#if item.featured}
                <span class="badge">Featured</span>
            {/if}
        </div>
    {/each}
</div>
"#;

        let start = Instant::now();

        for _ in 0..100 {
            let ast = parse_template(source).unwrap();
            let ir = transform_ast(ast).unwrap();
            let _lua_code = generate_lua_code(ir, "perf_test").unwrap();
        }

        let duration = start.elapsed();
        println!("100 compilations took: {:?}", duration);

        // Should be reasonably fast (adjust threshold as needed)
        assert!(duration.as_millis() < 5000);
    }

    #[test]
    fn test_rendering_performance() {
        let temp_dir = TempDir::new().unwrap();
        let engine = create_engine(temp_dir.path()).unwrap();

        let source = r#"
<ul>
{#each items as item}
    <li>{item}</li>
{/each}
</ul>
"#;

        // Create large dataset
        let mut context = HashMap::new();

        // Create the items table in the engine's context
        let table = engine.create_table().unwrap();
        for i in 0..1000 {
            table.set(i + 1, format!("Item {}", i)).unwrap();
        }
        let items_table = Value::Table(table);

        context.insert("items".to_string(), items_table);

        let start = Instant::now();

        for _ in 0..10 {
            let _result = engine.render_source(source, &context).unwrap();
        }

        let duration = start.elapsed();
        println!("10 renders of 1000 items took: {:?}", duration);

        // Should be reasonably fast
        assert!(duration.as_millis() < 2000);
    }
}
