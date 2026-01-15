// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Script content processor for LUAT magic functions.

/// Processes script content to transform LUAT magic functions like `$state()` and `$derived()`.
///
/// Magic functions are placeholders for future reactive primitives. Currently, they are
/// transformed into regular Lua code with comments indicating future implementation.
pub fn process_script_content(content: &str) -> String {
    let mut output = String::new();
    let mut remaining = content;
    while let Some(dollar_idx) = remaining.find('$') {
        // Find the start of the line containing the magic function
        let line_start = remaining[..dollar_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let before = &remaining[..line_start];
        output.push_str(before);
        let after_before = &remaining[line_start..];
        // Find the assignment operator before the magic function
        let assign_idx = after_before[..dollar_idx - line_start].rfind('=').unwrap_or(0);
        let (lhs, rhs) = if assign_idx > 0 {
            let lhs = after_before[..assign_idx + 1].trim_end();
            let rhs = after_before[assign_idx + 1..].trim_start();
            (lhs, rhs)
        } else {
            ("", after_before)
        };
        // Find the function name and args
        let rhs_dollar_idx = rhs.find('$').unwrap_or(0);
        let name_start = rhs_dollar_idx + 1;
        let name_end = rhs[name_start..].find('(').map(|i| name_start + i).unwrap_or(rhs.len());
        let function_name = &rhs[name_start..name_end];
        let args_start = name_end;
        // Find the matching parenthesis
        let mut paren_count = 0;
        let mut args_end = 0;
        for (i, c) in rhs[args_start..].char_indices() {
            if c == '(' {
                paren_count += 1;
            } else if c == ')' {
                paren_count -= 1;
                if paren_count == 0 {
                    args_end = args_start + i + 1;
                    break;
                }
            }
        }
        let args_str = &rhs[args_start..args_end];
        let after_magic_start = args_end;
        let mut after_magic = &rhs[after_magic_start..];
        // Remove a single trailing ')' if it is the first char in after_magic, or if after_magic is empty but the next char in remaining is ')'
        let mut skip_next = 0;
        if after_magic.starts_with(')') {
            after_magic = &after_magic[1..];
        } else if after_magic.is_empty() && remaining.len() > (line_start + assign_idx + 1 + rhs_dollar_idx + args_end) {
            let next_char = remaining.chars().nth(line_start + assign_idx + 1 + rhs_dollar_idx + args_end);
            if next_char == Some(')') {
                skip_next = 1;
            }
        }
        let args_content = &args_str[1..args_str.len() - 1];
        let args = args_content.split(',').map(|s| s.trim()).collect::<Vec<_>>();
        // Compose the replacement
        let (comment, value): (String, String) = match function_name {
            "derived" => (
                "-- LUAT magic function $derived will be implemented in future".to_string(),
                args_content.to_string()
            ),
            "state" => {
                if args.len() == 1 && args[0].is_empty() {
                    ("-- LUAT magic function $state will be implemented in future".to_string(), "nil".to_string())
                } else if args.len() == 1 {
                    ("-- LUAT magic function $state will be implemented in future".to_string(), args[0].to_string())
                } else if args.len() >= 2 {
                    ("-- LUAT magic function $state will be implemented in future".to_string(), format!("{} or {}", args[0], args[1]))
                } else {
                    ("-- LUAT magic function $state will be implemented in future".to_string(), "nil".to_string())
                }
            },
            // TODO: Implement other LUAT magic functions ($derived, $effect, etc.)
            // These placeholders will be replaced with actual implementations in future releases.
            // The identical branches are intentional - they serve as scaffolding for future logic.
            #[allow(clippy::if_same_then_else)]
            _ => {
                if args.len() == 1 && args[0].is_empty() {
                    (format!("-- LUAT magic function ${} will be implemented in future", function_name), "nil".to_string())
                } else if args.len() == 1 {
                    (format!("-- LUAT magic function ${} will be implemented in future", function_name), args[0].to_string())
                } else if args.len() >= 2 {
                    // Future: Handle default values and other arguments differently
                    (format!("-- LUAT magic function ${} will be implemented in future", function_name), args[0].to_string())
                } else {
                    (format!("-- LUAT magic function ${} will be implemented in future", function_name), "nil".to_string())
                }
            }
        };
        // Write the comment above the assignment
        if !lhs.is_empty() {
            output.push_str(&comment);
            output.push('\n');
            output.push_str(lhs);
            output.push(' ');
            output.push_str(&value);
            output.push_str(after_magic);
        } else {
            // No assignment, just replace inline
            output.push_str(&comment);
            output.push('\n');
            output.push_str(&value);
            output.push_str(after_magic);
        }
        // Move remaining pointer
        let consumed = line_start + assign_idx + 1 + rhs_dollar_idx + args_end + after_magic.len() + skip_next;
        remaining = &remaining[consumed..];
    }
    output.push_str(remaining);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_process_basic_magic_function() {
        let input = "local ok = $state(false)";
        let expected = "-- LUAT magic function $state will be implemented in future\nlocal ok = false";
        assert_eq!(process_script_content(input), expected);
    }
    
    #[test]
    fn test_process_magic_function_with_default() {
        let input = "let conditionalok = $state(hello, \"no value\")";
        let expected = "-- LUAT magic function $state will be implemented in future\nlet conditionalok = hello or \"no value\"";
        assert_eq!(process_script_content(input), expected);
    }
    
    #[test]
    fn test_process_derived_function() {
        let input = "local calc = $derived(function() return \"Calculated: \" .. ok end)";
        let expected = "-- LUAT magic function $derived will be implemented in future\nlocal calc = function() return \"Calculated: \" .. ok end";
        assert_eq!(process_script_content(input), expected);
    }
    
    #[test]
    fn test_process_empty_magic_function() {
        let input = "local something = $init()";
        let expected = "-- LUAT magic function $init will be implemented in future\nlocal something = nil";
        assert_eq!(process_script_content(input), expected);
    }
}
