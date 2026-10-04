// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Charges the matcher's work to the engine's limits.
//!
//! Every recursive match step, every byte a repetition scans and every
//! byte a back-reference or `%b` compares counts as one unit. Units are
//! charged in batches, as instructions, to the same budget the Lua hook
//! uses, and the deadline is checked with each batch.

use super::matcher::PatError;

#[cfg(not(target_arch = "wasm32"))]
pub(super) type Guard = crate::limits::LimitGuard;

/// Without the limits module there is nothing to charge.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
pub(super) struct Guard;

#[cfg(target_arch = "wasm32")]
impl Guard {
    fn charge(&self, _steps: i64) -> Option<&'static str> {
        None
    }

    pub(super) fn memory_headroom(&self, _lua: &mlua::Lua) -> Option<usize> {
        None
    }
}

/// Work units between two limit checks.
const CHECK_EVERY: u64 = 1000;

/// Counts matcher work for one call of a pattern function.
pub(super) struct Meter<'a> {
    guard: Option<&'a Guard>,
    pending: u64,
}

impl<'a> Meter<'a> {
    /// Starts metering; fails at once if a limit already tripped.
    pub(super) fn new(guard: Option<&'a Guard>) -> Result<Self, PatError> {
        let meter = Self { guard, pending: 0 };
        meter.settle(0)?;
        Ok(meter)
    }

    /// Records one unit of work.
    #[inline]
    pub(super) fn tick(&mut self) -> Result<(), PatError> {
        self.add(1)
    }

    /// Records `units` of work.
    #[inline]
    pub(super) fn add(&mut self, units: usize) -> Result<(), PatError> {
        self.pending = self.pending.saturating_add(units as u64);
        if self.pending >= CHECK_EVERY {
            let units = std::mem::take(&mut self.pending);
            self.settle(units)?;
        }
        Ok(())
    }

    #[cold]
    fn settle(&self, units: u64) -> Result<(), PatError> {
        match self.guard {
            Some(guard) => match guard.charge(i64::try_from(units).unwrap_or(i64::MAX)) {
                Some(message) => Err(PatError::Limit(message)),
                None => Ok(()),
            },
            None => Ok(()),
        }
    }
}
