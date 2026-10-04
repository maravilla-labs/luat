// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Script content processor for LUAT magic functions.

use crate::marker::{ReactiveKind, ReactiveMetadata, ReactiveVar};

/// Processes script content to transform LUAT magic functions like `$state()` and `$derived()`.
///
/// Returns the processed Lua code and reactive metadata about declared variables.
/// Magic functions are transformed into regular Lua assignments, and their reactive
/// nature is captured in the metadata for use by the codegen stage.
pub fn process_script_content_with_metadata(content: &str) -> (String, ReactiveMetadata) {
    let mut metadata = ReactiveMetadata::default();
    let lua_code = process_script_internal(content, &mut metadata);
    (lua_code, metadata)
}

/// Processes script content to transform LUAT magic functions like `$state()` and `$derived()`.
///
/// Magic functions are placeholders for future reactive primitives. Currently, they are
/// transformed into regular Lua code with comments indicating future implementation.
pub fn process_script_content(content: &str) -> String {
    let (lua_code, _metadata) = process_script_content_with_metadata(content);
    lua_code
}

fn process_script_internal(content: &str, metadata: &mut ReactiveMetadata) -> String {
    // Two-pass approach:
    // Pass 1: Extract all reactive var declarations (names, kinds, expressions)
    // Pass 2: Generate output with runtime API calls (needs all var names for deps)

    // --- Pass 1: Extract metadata ---
    extract_reactive_metadata(content, metadata);
    resolve_derived_deps(metadata);

    // --- Pass 2: Generate runtime API code ---
    generate_runtime_code(content, metadata)
}

/// Pass 1: Extract all reactive variable declarations from the script content.
/// Populates metadata with var names, kinds, and initial expressions.
fn extract_reactive_metadata(content: &str, metadata: &mut ReactiveMetadata) {
    let mut remaining = content;
    while let Some(dollar_idx) = remaining.find('$') {
        let line_start = remaining[..dollar_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let after_before = &remaining[line_start..];
        let assign_idx = after_before[..dollar_idx - line_start].rfind('=').unwrap_or(0);
        let (lhs, rhs) = if assign_idx > 0 {
            let lhs = after_before[..assign_idx + 1].trim_end();
            let rhs = after_before[assign_idx + 1..].trim_start();
            (lhs, rhs)
        } else {
            ("", after_before)
        };

        let rhs_dollar_idx = rhs.find('$').unwrap_or(0);
        let name_start = rhs_dollar_idx + 1;
        let name_end = rhs[name_start..].find('(').map(|i| name_start + i).unwrap_or(rhs.len());
        let function_name = &rhs[name_start..name_end];
        let args_start = name_end;

        let mut paren_count = 0;
        let mut args_end = 0;
        for (i, c) in rhs[args_start..].char_indices() {
            if c == '(' { paren_count += 1; }
            else if c == ')' {
                paren_count -= 1;
                if paren_count == 0 { args_end = args_start + i + 1; break; }
            }
        }

        let args_str = &rhs[args_start..args_end];
        let args_content = &args_str[1..args_str.len() - 1];
        let args = args_content.split(',').map(|s| s.trim()).collect::<Vec<_>>();
        let var_name = extract_var_name(lhs);

        match function_name {
            "derived" => {
                if let Some(name) = var_name {
                    metadata.vars.push(ReactiveVar {
                        name,
                        kind: ReactiveKind::Derived,
                        initial_expr: args_content.to_string(),
                        deps: vec![],
                    });
                }
            }
            "state" => {
                if let Some(name) = var_name {
                    let initial = if args.len() == 1 && args[0].is_empty() {
                        "nil".to_string()
                    } else {
                        args[0].to_string()
                    };
                    metadata.vars.push(ReactiveVar {
                        name,
                        kind: ReactiveKind::State,
                        initial_expr: initial,
                        deps: vec![],
                    });
                }
            }
            _ => {}
        }

        let after_magic_start = args_end;
        let after_magic = &rhs[after_magic_start..];
        let line_end_in_after = after_magic.find('\n').unwrap_or(after_magic.len());

        let mut skip_next = 0;
        let trimmed_after = &rhs[after_magic_start..];
        if trimmed_after.starts_with(')') {
            // skip
        } else if trimmed_after.is_empty() && remaining.len() > (line_start + assign_idx + 1 + rhs_dollar_idx + args_end) {
            let next_char = remaining.chars().nth(line_start + assign_idx + 1 + rhs_dollar_idx + args_end);
            if next_char == Some(')') { skip_next = 1; }
        }

        let consumed = line_start + assign_idx + 1 + rhs_dollar_idx + args_end + line_end_in_after + skip_next;
        if consumed >= remaining.len() { break; }
        remaining = &remaining[consumed..];
    }
}

/// Pass 2: Generate the Lua code with runtime API calls.
/// Replaces $state/$derived with runtime.state()/runtime.derived() calls.
fn generate_runtime_code(content: &str, metadata: &ReactiveMetadata) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut output_lines: Vec<String> = Vec::new();

    for line in &lines {
        if let Some(dollar_pos) = line.find('$') {
            // Check if this line has an assignment with a magic function
            let before_dollar = &line[..dollar_pos];
            if let Some(eq_pos) = before_dollar.rfind('=') {
                let lhs = line[..eq_pos + 1].trim_end();
                let rhs = line[eq_pos + 1..].trim();

                if let Some(transformed) = transform_magic_call(rhs, lhs, metadata) {
                    output_lines.push(format!("{} {}", lhs, transformed));
                    continue;
                }
            }
            // Standalone magic function (no assignment)
            let rhs = line.trim();
            if let Some(transformed) = transform_magic_call(rhs, "", metadata) {
                output_lines.push(transformed);
                continue;
            }
        }
        output_lines.push(line.to_string());
    }

    output_lines.join("\n")
}

/// Transform a magic function call ($state/$derived) to a runtime API call.
/// Returns None if the string doesn't contain a recognized magic function.
fn transform_magic_call(rhs: &str, lhs: &str, metadata: &ReactiveMetadata) -> Option<String> {
    let dollar_pos = rhs.find('$')?;
    let after_dollar = &rhs[dollar_pos + 1..];
    let paren_pos = after_dollar.find('(')?;
    let function_name = &after_dollar[..paren_pos];

    // Find matching closing paren
    let args_start_abs = dollar_pos + 1 + paren_pos;
    let mut paren_count = 0;
    let mut args_end_abs = 0;
    for (i, c) in rhs[args_start_abs..].char_indices() {
        if c == '(' { paren_count += 1; }
        else if c == ')' {
            paren_count -= 1;
            if paren_count == 0 {
                args_end_abs = args_start_abs + i + 1;
                break;
            }
        }
    }
    if args_end_abs == 0 { return None; }

    let args_str = &rhs[args_start_abs + 1..args_end_abs - 1]; // content inside parens
    let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();
    let var_name = extract_var_name(lhs);

    let value = match function_name {
        "derived" => {
            if let Some(ref name) = var_name {
                let deps = metadata.vars.iter()
                    .find(|v| &v.name == name && v.kind == ReactiveKind::Derived)
                    .map(|v| &v.deps)
                    .cloned()
                    .unwrap_or_default();
                let deps_lua = format!("{{{}}}", deps.iter().map(|d| format!("\"{}\"", d)).collect::<Vec<_>>().join(", "));
                format!(
                    "runtime.derived(\"{}\", function() return {} end, {})",
                    name, args_str, deps_lua
                )
            } else {
                args_str.to_string()
            }
        },
        "state" => {
            let initial = if args.len() == 1 && args[0].is_empty() {
                "nil".to_string()
            } else if args.len() == 1 {
                args[0].to_string()
            } else if args.len() >= 2 {
                format!("{} or {}", args[0], args[1])
            } else {
                "nil".to_string()
            };
            if let Some(ref name) = var_name {
                format!("runtime.state(\"{}\", {})", name, initial)
            } else {
                initial
            }
        },
        _ => {
            if args.len() == 1 && args[0].is_empty() {
                "nil".to_string()
            } else if !args.is_empty() {
                args[0].to_string()
            } else {
                "nil".to_string()
            }
        }
    };

    Some(value)
}

/// Extracts the variable name from an LHS like "local count =".
fn extract_var_name(lhs: &str) -> Option<String> {
    let trimmed = lhs.trim_end_matches('=').trim();
    // Handle "local varname" or just "varname"
    let name_part = trimmed.strip_prefix("local ").unwrap_or(trimmed);
    let name = name_part.trim();
    if name.is_empty() || !name.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) {
        return None;
    }
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

/// Resolves dependencies for $derived variables by checking which
/// reactive variable names appear in their expressions.
fn resolve_derived_deps(metadata: &mut ReactiveMetadata) {
    let var_names: Vec<String> = metadata.vars.iter().map(|v| v.name.clone()).collect();
    for var in &mut metadata.vars {
        if var.kind == ReactiveKind::Derived {
            var.deps = var_names
                .iter()
                .filter(|name| *name != &var.name && crate::marker::expr_references_var(&var.initial_expr, name))
                .cloned()
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_basic_state() {
        let input = "local ok = $state(false)";
        assert_eq!(
            process_script_content(input),
            "local ok = runtime.state(\"ok\", false)"
        );
    }

    #[test]
    fn test_process_state_with_default() {
        let input = "local conditionalok = $state(hello, \"no value\")";
        assert_eq!(
            process_script_content(input),
            "local conditionalok = runtime.state(\"conditionalok\", hello or \"no value\")"
        );
    }

    #[test]
    fn test_process_derived() {
        let input = "local ok = $state(false)\nlocal calc = $derived(\"Calculated: \" .. tostring(ok))";
        let result = process_script_content(input);
        assert!(result.contains("runtime.derived(\"calc\", function() return \"Calculated: \" .. tostring(ok) end, {\"ok\"})"));
    }

    #[test]
    fn test_process_unknown_magic_function() {
        let input = "local something = $init()";
        assert_eq!(process_script_content(input), "local something = nil");
    }

    #[test]
    fn test_metadata_extraction_state() {
        let input = "local count = $state(0)";
        let (lua, metadata) = process_script_content_with_metadata(input);
        assert_eq!(lua, "local count = runtime.state(\"count\", 0)");
        assert_eq!(metadata.vars.len(), 1);
        assert_eq!(metadata.vars[0].name, "count");
        assert_eq!(metadata.vars[0].kind, ReactiveKind::State);
        assert_eq!(metadata.vars[0].initial_expr, "0");
    }

    #[test]
    fn test_metadata_extraction_derived() {
        let input = "local count = $state(0)\nlocal doubled = $derived(count * 2)";
        let (lua, metadata) = process_script_content_with_metadata(input);
        assert!(lua.contains("runtime.state(\"count\", 0)"));
        assert!(lua.contains("runtime.derived(\"doubled\", function() return count * 2 end, {\"count\"})"));
        assert_eq!(metadata.vars.len(), 2);
        assert_eq!(metadata.vars[0].name, "count");
        assert_eq!(metadata.vars[0].kind, ReactiveKind::State);
        assert_eq!(metadata.vars[1].name, "doubled");
        assert_eq!(metadata.vars[1].kind, ReactiveKind::Derived);
        assert_eq!(metadata.vars[1].initial_expr, "count * 2");
        assert_eq!(metadata.vars[1].deps, vec!["count"]);
    }

    #[test]
    fn test_metadata_is_reactive() {
        let input = "local x = 5";
        let (_, metadata) = process_script_content_with_metadata(input);
        assert!(!metadata.is_reactive());

        let input = "local count = $state(0)";
        let (_, metadata) = process_script_content_with_metadata(input);
        assert!(metadata.is_reactive());
    }

    #[test]
    fn test_find_deps() {
        let input = "local count = $state(0)\nlocal doubled = $derived(count * 2)";
        let (_, metadata) = process_script_content_with_metadata(input);
        let deps = metadata.find_deps("count + 1");
        assert_eq!(deps, vec!["count"]);
    }

    #[test]
    fn test_state_generates_runtime_call() {
        let input = "local count = $state(0)";
        let (lua, _) = process_script_content_with_metadata(input);
        assert_eq!(lua, "local count = runtime.state(\"count\", 0)");
    }

    #[test]
    fn test_derived_generates_runtime_call_with_deps() {
        let input = "local count = $state(0)\nlocal doubled = $derived(count * 2)";
        let (lua, _) = process_script_content_with_metadata(input);
        assert!(lua.contains("runtime.derived(\"doubled\", function() return count * 2 end, {\"count\"})"));
    }

    #[test]
    fn test_derived_with_multiple_deps() {
        let input = "local a = $state(1)\nlocal b = $state(2)\nlocal sum = $derived(a + b)";
        let (lua, metadata) = process_script_content_with_metadata(input);
        assert!(lua.contains("runtime.derived(\"sum\", function() return a + b end, {\"a\", \"b\"})"));
        assert_eq!(metadata.vars[2].deps, vec!["a", "b"]);
    }

    #[test]
    fn test_multiline_state_and_derived_no_stray_chars() {
        let input = "local count = $state(0)\nlocal doubled = $derived(count * 2)";
        let (lua, _) = process_script_content_with_metadata(input);
        assert_eq!(
            lua,
            "local count = runtime.state(\"count\", 0)\nlocal doubled = runtime.derived(\"doubled\", function() return count * 2 end, {\"count\"})"
        );
    }
}
