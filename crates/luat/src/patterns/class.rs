// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Single-character classes: `.`, `%a` and friends, and `[...]` sets.
//!
//! Works on bytes in the C locale, as Lua's own matcher does.

use super::matcher::PatError;

/// The byte at `i`, or 0 past the end (C reads the terminating NUL there).
#[inline]
pub(super) fn byte_at(bytes: &[u8], i: usize) -> u8 {
    bytes.get(i).copied().unwrap_or(0)
}

/// Whether `c` belongs to the class letter `cl` (`a`, `d`, `S`, ...).
/// Any other `cl` stands for itself.
pub(super) fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        // C isspace also accepts vertical tab, unlike is_ascii_whitespace.
        b's' => matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        // Deprecated in Lua 5.2, still accepted by 5.4.
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() {
        !res
    } else {
        res
    }
}

/// Index just past the single-character class starting at `p`.
pub(super) fn class_end(pat: &[u8], p: usize) -> Result<usize, PatError> {
    let mut p = p;
    let first = pat[p];
    p += 1;
    match first {
        b'%' => {
            if p >= pat.len() {
                return Err(PatError::pattern("malformed pattern (ends with '%')"));
            }
            Ok(p + 1)
        }
        b'[' => {
            if byte_at(pat, p) == b'^' {
                p += 1;
            }
            // Look for the closing ']'; a ']' right after '[' (or '[^') is
            // part of the set.
            loop {
                if p >= pat.len() {
                    return Err(PatError::pattern("malformed pattern (missing ']')"));
                }
                let c = pat[p];
                p += 1;
                if c == b'%' && p < pat.len() {
                    p += 1;
                }
                if byte_at(pat, p) == b']' {
                    break;
                }
            }
            Ok(p + 1)
        }
        _ => Ok(p),
    }
}

/// Whether `c` is in the set `[...]` that starts at `p` and whose closing
/// `]` is at `ec`.
pub(super) fn match_bracket_class(c: u8, pat: &[u8], p: usize, ec: usize) -> bool {
    let mut p = p;
    let mut sig = true;
    if byte_at(pat, p + 1) == b'^' {
        sig = false;
        p += 1;
    }
    loop {
        p += 1;
        if p >= ec {
            return !sig;
        }
        if pat[p] == b'%' {
            p += 1;
            if match_class(c, byte_at(pat, p)) {
                return sig;
            }
        } else if byte_at(pat, p + 1) == b'-' && p + 2 < ec {
            p += 2;
            if pat[p - 2] <= c && c <= pat[p] {
                return sig;
            }
        } else if pat[p] == c {
            return sig;
        }
    }
}

/// Whether the subject byte at `s` matches the class `pat[p..ep]`.
pub(super) fn single_match(src: &[u8], s: usize, pat: &[u8], p: usize, ep: usize) -> bool {
    let Some(&c) = src.get(s) else {
        return false;
    };
    match pat[p] {
        b'.' => true,
        b'%' => match_class(c, byte_at(pat, p + 1)),
        b'[' => match_bracket_class(c, pat, p, ep - 1),
        literal => literal == c,
    }
}
