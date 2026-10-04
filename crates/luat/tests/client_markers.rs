// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Integration tests for client-side marker emission.
//!
//! Verifies that templates with reactive state produce correct markers
//! (TB for template boundaries, EV for event handlers) and that the
//! generated Lua code uses the runtime API correctly.

#![cfg(feature = "client-markers")]

use luat::codegen::generate_lua_code;
use luat::parser::parse_template;
use luat::transform::transform_ast;

/// Helper: compile a template to Lua code, return the generated string.
fn compile(source: &str) -> String {
    let ast = parse_template(source).unwrap();
    let ir = transform_ast(ast).unwrap();
    generate_lua_code(ir, "test").unwrap()
}

/// Extract all marker payloads from generated Lua code.
/// Returns payloads (content between `<!--l:` and `-->`).
fn extract_marker_payloads(lua_code: &str) -> Vec<String> {
    let prefix = "<!--l:";
    let suffix = "-->";
    let mut payloads = Vec::new();
    let mut search_from = 0;

    while let Some(start) = lua_code[search_from..].find(prefix) {
        let abs_start = search_from + start + prefix.len();
        if let Some(end) = lua_code[abs_start..].find(suffix) {
            payloads.push(lua_code[abs_start..abs_start + end].to_string());
            search_from = abs_start + end + suffix.len();
        } else {
            break;
        }
    }
    payloads
}

// ============================================================
// Template Boundary (TB) Marker Tests
// ============================================================

#[test]
fn test_template_boundary_marker_emitted() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<div>{count}</div>"#,
    );

    assert!(
        lua_code.contains("<!--l:TB(test)-->"),
        "Should emit TB start marker with module name"
    );
    assert!(
        lua_code.contains("<!--/l-->"),
        "Should emit TB end marker"
    );
}

#[test]
fn test_template_boundary_wraps_content() {
    let lua_code = compile(r#"<p>Hello</p>"#);

    let tb_start = lua_code.find("<!--l:TB(test)-->").expect("TB start marker");
    let tb_end = lua_code.find("<!--/l-->").expect("TB end marker");
    let p_pos = lua_code.find("<p").expect("<p> tag");

    assert!(tb_start < p_pos, "TB start should precede content");
    assert!(p_pos < tb_end, "Content should precede TB end");
}

#[test]
fn test_non_reactive_template_still_has_tb_marker() {
    let lua_code = compile(r#"<div>Hello {name}</div>"#);

    assert!(
        lua_code.contains("<!--l:TB(test)-->"),
        "Even non-reactive templates get TB markers"
    );
    assert!(lua_code.contains("<!--/l-->"));
}

// ============================================================
// Event (EV) Marker Tests
// ============================================================

#[test]
fn test_event_marker_emitted_for_click() {
    let lua_code = compile(r#"<button on:click={handleClick}>Click</button>"#);

    assert!(
        lua_code.contains("<!--l:EV(click_0)-->"),
        "Should emit EV marker with handler name"
    );
}

#[test]
fn test_event_marker_precedes_element() {
    let lua_code = compile(r#"<button on:click={handleClick}>Click</button>"#);

    let ev_pos = lua_code
        .find("<!--l:EV(click_0)-->")
        .expect("EV marker should exist");
    let button_pos = lua_code.find("<button").expect("button element");
    assert!(
        ev_pos < button_pos,
        "EV marker should precede the element"
    );
}

#[test]
fn test_no_onclick_attribute_in_html() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<button on:click={count = count + 1}>Inc</button>"#,
    );

    assert!(
        !lua_code.contains("on:click"),
        "on:click should NOT appear as HTML attribute"
    );
}

#[test]
fn test_multiple_events_get_unique_names() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<button on:click={count = count + 1}>Inc</button>
<button on:click={count = count - 1}>Dec</button>"#,
    );

    assert!(lua_code.contains("<!--l:EV(click_0)-->"));
    assert!(lua_code.contains("<!--l:EV(click_1)-->"));
}

#[test]
fn test_different_event_types() {
    let lua_code = compile(
        r#"<form on:submit={handleSubmit}>
    <input on:input={handleInput} />
    <button on:click={handleClick}>Go</button>
</form>"#,
    );

    assert!(lua_code.contains("<!--l:EV(submit_0)-->"));
    assert!(lua_code.contains("<!--l:EV(input_1)-->"));
    assert!(lua_code.contains("<!--l:EV(click_2)-->"));
}

// ============================================================
// Handler Function Generation Tests
// ============================================================

#[test]
fn test_handler_function_generated() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<button on:click={count = count + 1}>Inc</button>"#,
    );

    assert!(
        lua_code.contains("exports.__handlers = {}"),
        "Handlers table should be initialized"
    );
    assert!(
        lua_code.contains("exports.__handlers[\"click_0\"]"),
        "Handler function should be registered"
    );
    assert!(
        lua_code.contains("function(rt)"),
        "Handler should take rt parameter"
    );
}

#[test]
fn test_handler_uses_rt_get_set() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<button on:click={count = count + 1}>Inc</button>"#,
    );
    println!("Generated:\n{}", lua_code);

    assert!(
        lua_code.contains("rt.set(\"count\""),
        "Handler should use rt.set for assignments"
    );
    assert!(
        lua_code.contains("rt.get(\"count\")"),
        "Handler should use rt.get for reads"
    );
}

#[test]
fn test_handler_with_multiple_reactive_vars() {
    let lua_code = compile(
        r#"<script>
local x = $state(1)
local y = $state(2)
</script>
<button on:click={x = y + 1}>Set</button>"#,
    );
    println!("Generated:\n{}", lua_code);

    assert!(lua_code.contains("rt.set(\"x\""));
    assert!(lua_code.contains("rt.get(\"y\")"));
}

// ============================================================
// Runtime API Tests (Script Transformation)
// ============================================================

#[test]
fn test_state_transformed_to_runtime_call() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<div>{count}</div>"#,
    );

    assert!(
        lua_code.contains("runtime.state(\"count\", 0)"),
        "Should transform $state to runtime.state() call"
    );
}

#[test]
fn test_derived_transformed_to_runtime_call() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
local doubled = $derived(count * 2)
</script>
<div>{doubled}</div>"#,
    );
    println!("Generated:\n{}", lua_code);

    assert!(
        lua_code.contains("runtime.state(\"count\", 0)"),
        "Should transform $state"
    );
    assert!(
        lua_code.contains("runtime.derived(\"doubled\""),
        "Should transform $derived to runtime.derived() call"
    );
}

#[test]
fn test_state_with_string_initial() {
    let lua_code = compile(
        r#"<script>
local name = $state("world")
</script>
<p>Hello {name}</p>"#,
    );

    assert!(lua_code.contains("runtime.state(\"name\", \"world\")"));
}

#[test]
fn test_state_with_boolean_initial() {
    let lua_code = compile(
        r#"<script>
local visible = $state(true)
</script>
<div>{visible}</div>"#,
    );

    assert!(lua_code.contains("runtime.state(\"visible\", true)"));
}

// ============================================================
// Module Exports Structure Tests
// ============================================================

#[test]
fn test_exports_structure() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<div>{count}</div>"#,
    );

    assert!(lua_code.contains("local exports = {}"));
    assert!(lua_code.contains("exports.render = render"));
    assert!(lua_code.contains("exports.moduleName = \"test\""));
    assert!(lua_code.contains("return exports"));
}

#[test]
fn test_state_defs_exported() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
local name = $state("hello")
</script>
<div>{count} {name}</div>"#,
    );
    println!("Generated:\n{}", lua_code);

    assert!(
        lua_code.contains("exports.state_defs"),
        "Should export state_defs table"
    );
}

#[test]
fn test_derived_defs_exported() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
local doubled = $derived(count * 2)
</script>
<div>{doubled}</div>"#,
    );

    assert!(
        lua_code.contains("exports.derived_defs"),
        "Should export derived_defs table"
    );
}

// ============================================================
// No Old-Style Markers Tests
// ============================================================

#[test]
fn test_no_base64_markers() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
local doubled = $derived(count * 2)
</script>
<button on:click={count = count + 1} class={count > 0 and "active" or ""}>
    {count} doubled: {doubled}
</button>"#,
    );

    let payloads = extract_marker_payloads(&lua_code);
    for payload in &payloads {
        assert!(
            payload.starts_with("TB(") || payload.starts_with("EV(") || payload == "/l",
            "All markers should be TB or EV format, got: {}",
            payload
        );
    }
}

#[test]
fn test_no_expression_markers() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<span>{count}</span>"#,
    );

    // Only TB and EV markers should exist, no Expression markers
    let payloads = extract_marker_payloads(&lua_code);
    for payload in &payloads {
        assert!(
            payload.starts_with("TB(") || payload.starts_with("EV(") || payload == "/l",
            "Should not have Expression markers, got: {}",
            payload
        );
    }
}

#[test]
fn test_no_attribute_markers() {
    let lua_code = compile(
        r#"<script>
local x = $state(50)
</script>
<div style={x .. "px"}>Content</div>"#,
    );

    let payloads = extract_marker_payloads(&lua_code);
    for payload in &payloads {
        assert!(
            payload.starts_with("TB(") || payload.starts_with("EV(") || payload == "/l",
            "Should not have Attribute markers, got: {}",
            payload
        );
    }
}

// ============================================================
// Non-Reactive Templates
// ============================================================

#[test]
fn test_non_reactive_no_event_markers() {
    let lua_code = compile(r#"<div>{name}</div>"#);

    let payloads = extract_marker_payloads(&lua_code);
    let ev_markers: Vec<_> = payloads.iter().filter(|p| p.starts_with("EV(")).collect();
    assert!(
        ev_markers.is_empty(),
        "Non-reactive template should have no EV markers"
    );
}

#[test]
fn test_static_template_no_handlers() {
    let lua_code = compile(r#"<div class="static">Hello</div>"#);

    assert!(
        !lua_code.contains("exports.__handlers"),
        "Static template should not have handlers"
    );
}

// ============================================================
// Counter Template (End-to-End)
// ============================================================

#[test]
fn test_counter_template_full_output() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
</script>
<button on:click={count = count + 1}>Count: {count}</button>"#,
    );
    println!("Generated counter template:\n{}", lua_code);

    // TB markers
    assert!(lua_code.contains("<!--l:TB(test)-->"));
    assert!(lua_code.contains("<!--/l-->"));

    // EV marker for click handler
    assert!(lua_code.contains("<!--l:EV(click_0)-->"));

    // No on:click HTML attribute
    assert!(!lua_code.contains("on:click"));

    // Runtime state API
    assert!(lua_code.contains("runtime.state(\"count\", 0)"));

    // Handler function
    assert!(lua_code.contains("exports.__handlers[\"click_0\"]"));
    assert!(lua_code.contains("rt.set(\"count\""));
    assert!(lua_code.contains("rt.get(\"count\")"));

    // Module exports
    assert!(lua_code.contains("exports.render = render"));
    assert!(lua_code.contains("return exports"));
}

#[test]
fn test_counter_with_derived() {
    let lua_code = compile(
        r#"<script>
local count = $state(0)
local doubled = $derived(count * 2)
</script>
<button on:click={count = count + 1}>
    Count: {count}, Doubled: {doubled}
</button>"#,
    );
    println!("Generated:\n{}", lua_code);

    // Both state and derived should use runtime API
    assert!(lua_code.contains("runtime.state(\"count\", 0)"));
    assert!(lua_code.contains("runtime.derived(\"doubled\""));
    // Template should render both vars
    assert!(lua_code.contains("smart_tostring(count)"));
    assert!(lua_code.contains("smart_tostring(doubled)"));
}

// ============================================================
// Edge Cases
// ============================================================

#[test]
fn test_event_on_component_not_ev_marker() {
    let lua_code = compile(r#"<Button on:click={handleClick}>Go</Button>"#);

    // Component events become props, not EV markers
    let payloads = extract_marker_payloads(&lua_code);
    let ev_markers: Vec<_> = payloads.iter().filter(|p| p.starts_with("EV(")).collect();
    assert!(
        ev_markers.is_empty(),
        "Component events should not produce EV markers"
    );
    assert!(lua_code.contains("on_click"), "Should pass as prop");
}

#[test]
fn test_marker_ordering_in_output() {
    let lua_code = compile(
        r#"<script>
local x = $state(0)
</script>
<div>
    <button on:click={x = x + 1}>Click</button>
    <span>{x}</span>
</div>"#,
    );

    // TB should come first, then EV before button, then content
    let tb_pos = lua_code.find("<!--l:TB(test)-->").unwrap();
    let ev_pos = lua_code.find("<!--l:EV(click_0)-->").unwrap();
    let end_pos = lua_code.find("<!--/l-->").unwrap();

    assert!(tb_pos < ev_pos, "TB should precede EV");
    assert!(ev_pos < end_pos, "EV should precede end marker");
}
