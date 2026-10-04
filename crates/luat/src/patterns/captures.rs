// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Reading the captures of a successful match.

use super::matcher::{MatchState, PatError, CAP_POSITION, CAP_UNFINISHED};

/// A capture's value: a substring (byte range) or a position (1-based).
#[derive(Debug, Clone, Copy)]
pub(super) enum Cap {
    Str(usize, usize),
    Pos(usize),
}

impl MatchState<'_, '_> {
    /// Capture `i` of the last match `s..e`. With no captures, capture 0 is
    /// the whole match.
    pub(super) fn capture(&self, i: usize, s: usize, e: usize) -> Result<Cap, PatError> {
        if i >= self.level {
            if i != 0 {
                return Err(PatError::Pattern(format!(
                    "invalid capture index %{}",
                    i + 1
                )));
            }
            return Ok(Cap::Str(s, e));
        }
        let cap = self.capture[i];
        match cap.len {
            CAP_UNFINISHED => Err(PatError::pattern("unfinished capture")),
            CAP_POSITION => Ok(Cap::Pos(cap.init + 1)),
            len => Ok(Cap::Str(cap.init, cap.init + len as usize)),
        }
    }

    /// All captures of the last match. `whole` is the match itself, used
    /// as the only capture when the pattern has none; without it a pattern
    /// with no captures yields nothing (as `find` wants).
    pub(super) fn captures(&self, whole: Option<(usize, usize)>) -> Result<Vec<Cap>, PatError> {
        let count = if self.level == 0 && whole.is_some() {
            1
        } else {
            self.level
        };
        let (s, e) = whole.unwrap_or((0, 0));
        (0..count).map(|i| self.capture(i, s, e)).collect()
    }
}
