// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! The pattern matcher: a port of the backtracking matcher in Lua 5.4's
//! `lstrlib.c`, with the same recursion limit and errors, that charges its
//! work to a [`Meter`].

use super::class::{byte_at, class_end, match_bracket_class, single_match};
use super::meter::Meter;

/// Maximum number of captures in a pattern (`LUA_MAXCAPTURES`).
const MAX_CAPTURES: usize = 32;
/// Maximum recursion depth of the matcher (`MAXCCALLS`).
const MAX_DEPTH: usize = 200;
pub(super) const CAP_UNFINISHED: isize = -1;
pub(super) const CAP_POSITION: isize = -2;

/// Why a match could not complete.
#[derive(Debug)]
pub(super) enum PatError {
    /// An error in the pattern, reported like Lua reports it.
    Pattern(String),
    /// An engine limit stopped the matcher.
    Limit(&'static str),
}

impl PatError {
    pub(super) fn pattern(message: impl Into<String>) -> Self {
        Self::Pattern(message.into())
    }
}

type MatchResult = Result<Option<usize>, PatError>;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Capture {
    pub(super) init: usize,
    pub(super) len: isize,
}

/// State of one match of `pat` against `src`.
pub(super) struct MatchState<'a, 'm> {
    pub(super) src: &'a [u8],
    pat: &'a [u8],
    pub(super) level: usize,
    depth: usize,
    pub(super) capture: [Capture; MAX_CAPTURES],
    meter: &'m mut Meter<'a>,
}

impl<'a, 'm> MatchState<'a, 'm> {
    pub(super) fn new(src: &'a [u8], pat: &'a [u8], meter: &'m mut Meter<'a>) -> Self {
        Self {
            src,
            pat,
            level: 0,
            depth: MAX_DEPTH,
            capture: [Capture::default(); MAX_CAPTURES],
            meter,
        }
    }

    /// Tries to match the whole pattern at subject position `s`. Returns
    /// the end of the match.
    pub(super) fn match_at(&mut self, s: usize) -> MatchResult {
        self.level = 0;
        self.depth = MAX_DEPTH;
        self.do_match(s, 0)
    }

    /// The meter, for work done outside the matcher.
    pub(super) fn meter(&mut self) -> &mut Meter<'a> {
        self.meter
    }

    fn do_match(&mut self, s: usize, p: usize) -> MatchResult {
        if self.depth == 0 {
            return Err(PatError::pattern("pattern too complex"));
        }
        self.depth -= 1;
        let res = self.match_here(s, p);
        self.depth += 1;
        res
    }

    fn match_here(&mut self, mut s: usize, mut p: usize) -> MatchResult {
        let (src, pat) = (self.src, self.pat);
        loop {
            self.meter.tick()?;
            if p == pat.len() {
                return Ok(Some(s));
            }
            match pat[p] {
                b'(' => {
                    return if byte_at(pat, p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == pat.len() => return Ok((s == src.len()).then_some(s)),
                b'%' => match byte_at(pat, p + 1) {
                    b'b' => match self.match_balance(s, p + 2)? {
                        Some(next) => {
                            s = next;
                            p += 4;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    b'f' => {
                        p += 2;
                        if byte_at(pat, p) != b'[' {
                            return Err(PatError::pattern("missing '[' after '%f' in pattern"));
                        }
                        let ep = class_end(pat, p)?;
                        let previous = if s == 0 { 0 } else { src[s - 1] };
                        let current = byte_at(src, s);
                        if !match_bracket_class(previous, pat, p, ep - 1)
                            && match_bracket_class(current, pat, p, ep - 1)
                        {
                            p = ep;
                            continue;
                        }
                        return Ok(None);
                    }
                    digit @ b'0'..=b'9' => match self.match_capture(s, digit)? {
                        Some(next) => {
                            s = next;
                            p += 2;
                            continue;
                        }
                        None => return Ok(None),
                    },
                    _ => {}
                },
                _ => {}
            }

            // A single-character class, possibly followed by a quantifier.
            let ep = class_end(pat, p)?;
            let quantifier = byte_at(pat, ep);
            if !single_match(src, s, pat, p, ep) {
                if matches!(quantifier, b'*' | b'?' | b'-') {
                    p = ep + 1;
                    continue;
                }
                return Ok(None);
            }
            match quantifier {
                b'?' => {
                    if let Some(end) = self.do_match(s + 1, ep + 1)? {
                        return Ok(Some(end));
                    }
                    p = ep + 1;
                }
                b'+' => return self.max_expand(s + 1, p, ep),
                b'*' => return self.max_expand(s, p, ep),
                b'-' => return self.min_expand(s, p, ep),
                _ => {
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> MatchResult {
        let mut i = 0;
        while single_match(self.src, s + i, self.pat, p, ep) {
            i += 1;
            self.meter.tick()?;
        }
        loop {
            if let Some(end) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(end));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> MatchResult {
        loop {
            if let Some(end) = self.do_match(s, ep + 1)? {
                return Ok(Some(end));
            }
            if !single_match(self.src, s, self.pat, p, ep) {
                return Ok(None);
            }
            s += 1;
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> MatchResult {
        if self.level >= MAX_CAPTURES {
            return Err(PatError::pattern("too many captures"));
        }
        self.capture[self.level] = Capture { init: s, len: what };
        self.level += 1;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.level -= 1;
        }
        Ok(res)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> MatchResult {
        let l = (0..self.level)
            .rev()
            .find(|&l| self.capture[l].len == CAP_UNFINISHED)
            .ok_or_else(|| PatError::pattern("invalid pattern capture"))?;
        self.capture[l].len = (s - self.capture[l].init) as isize;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.capture[l].len = CAP_UNFINISHED;
        }
        Ok(res)
    }

    /// `%1`..`%9`: matches the text of an earlier, closed capture.
    fn match_capture(&mut self, s: usize, digit: u8) -> MatchResult {
        let l = digit as isize - b'1' as isize;
        if l < 0 || l as usize >= self.level || self.capture[l as usize].len == CAP_UNFINISHED {
            return Err(PatError::Pattern(format!(
                "invalid capture index %{}",
                l + 1
            )));
        }
        let cap = self.capture[l as usize];
        // A position capture never matches (its length is not a length).
        let Ok(len) = usize::try_from(cap.len) else {
            return Ok(None);
        };
        self.meter.add(len)?;
        let matches =
            self.src.len() - s >= len && self.src[cap.init..cap.init + len] == self.src[s..s + len];
        Ok(matches.then_some(s + len))
    }

    /// `%bxy`: a balanced run from `x` to the matching `y`.
    fn match_balance(&mut self, s: usize, p: usize) -> MatchResult {
        if p + 1 >= self.pat.len() {
            return Err(PatError::pattern(
                "malformed pattern (missing arguments to '%b')",
            ));
        }
        let (open, close) = (self.pat[p], self.pat[p + 1]);
        if self.src.get(s) != Some(&open) {
            return Ok(None);
        }
        let mut depth = 1;
        for (offset, &c) in self.src[s + 1..].iter().enumerate() {
            if c == close {
                depth -= 1;
                if depth == 0 {
                    self.meter.add(offset + 1)?;
                    return Ok(Some(s + offset + 2));
                }
            } else if c == open {
                depth += 1;
            }
        }
        self.meter.add(self.src.len() - s)?;
        Ok(None)
    }
}
