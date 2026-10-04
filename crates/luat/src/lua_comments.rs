// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Blanking Lua comments before scanning source for `require("...")`.
//!
//! The dependency scanners use a regex over Lua source, which would also
//! match a commented-out `-- require("x")`. [`blank_comments`] replaces
//! every comment with spaces (keeping newlines, so offsets and line numbers
//! stay valid) and leaves string literals alone, so `"--"` inside a string
//! is not mistaken for a comment.

/// Returns `source` with its comments (`-- ...` and `--[[ ... ]]`, with any
/// number of `=` in the long brackets) replaced by spaces. Newlines inside
/// comments are kept.
pub fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let end = match long_bracket_level(bytes, i + 2) {
                    Some(level) => long_bracket_end(bytes, i + 2, level),
                    None => bytes[i..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |p| i + p),
                };
                out.extend(
                    bytes[i..end]
                        .iter()
                        .map(|&b| if b == b'\n' { b'\n' } else { b' ' }),
                );
                i = end;
            }
            b'"' | b'\'' => {
                let end = quoted_string_end(bytes, i);
                out.extend_from_slice(&bytes[i..end]);
                i = end;
            }
            b'[' => match long_bracket_level(bytes, i) {
                Some(level) => {
                    let end = long_bracket_end(bytes, i, level);
                    out.extend_from_slice(&bytes[i..end]);
                    i = end;
                }
                None => {
                    out.push(b'[');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    // Only ASCII bytes of comments were replaced, and whole multi-byte
    // sequences inside comments became spaces, so this is still UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

/// If an opening long bracket (`[[`, `[=[`, ...) starts at `i`, returns its
/// level (number of `=`).
fn long_bracket_level(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let level = bytes[i + 1..].iter().take_while(|&&b| b == b'=').count();
    (bytes.get(i + 1 + level) == Some(&b'[')).then_some(level)
}

/// End (exclusive) of the long bracket opened at `i` with `level`: just past
/// the matching `]=*]`, or the end of the source if it is not closed.
fn long_bracket_end(bytes: &[u8], i: usize, level: usize) -> usize {
    let close: Vec<u8> = std::iter::once(b']')
        .chain(std::iter::repeat(b'=').take(level))
        .chain(std::iter::once(b']'))
        .collect();
    let start = i + level + 2;
    bytes[start..]
        .windows(close.len())
        .position(|w| w == close.as_slice())
        .map_or(bytes.len(), |p| start + p + close.len())
}

/// End (exclusive) of the quoted string starting at `i`: past the closing
/// quote, or at the end of the line if it is not closed.
fn quoted_string_end(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'\n' => return j,
            b if b == quote => return j + 1,
            _ => j += 1,
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::blank_comments;

    #[test]
    fn line_comments_are_blanked() {
        let src = "local a = require(\"a\") -- require(\"b\")\nlocal c = 1";
        let out = blank_comments(src);
        assert_eq!(out.len(), src.len());
        assert!(out.contains("require(\"a\")"));
        assert!(!out.contains("\"b\""));
        assert!(out.ends_with("\nlocal c = 1"));
    }

    #[test]
    fn long_comments_are_blanked_and_keep_lines() {
        let src =
            "--[[ require(\"x\")\nrequire(\"y\") ]] local z = 1\n--[==[ ]] require(\"w\") ]==]";
        let out = blank_comments(src);
        assert!(!out.contains("require"));
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(out.contains("local z = 1"));
    }

    #[test]
    fn strings_are_kept() {
        let src = r#"local s = "-- not a comment" .. '--[[ nor this' .. [[ -- or this ]]
local t = "esc \" -- still string" -- comment"#;
        let out = blank_comments(src);
        assert!(out.contains("\"-- not a comment\""));
        assert!(out.contains("'--[[ nor this'"));
        assert!(out.contains("[[ -- or this ]]"));
        assert!(out.contains("\"esc \\\" -- still string\""));
        assert!(!out.contains("comment\"\n") && !out.ends_with("comment"));
    }

    #[test]
    fn unicode_in_comments_and_unclosed_comment() {
        let out = blank_comments("x = 1 -- héllo\n--[[ never closed require('a')");
        assert!(out.starts_with("x = 1 "));
        assert!(!out.contains("require"));
    }
}
