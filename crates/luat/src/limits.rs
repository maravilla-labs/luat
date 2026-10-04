// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Execution limits for guest code: memory, instruction count and a
//! wall-clock deadline.
//!
//! Limits apply to everything an [`Engine`](crate::Engine) runs, including
//! coroutines created by `respond_async` or by guest code. For per-request
//! limits, give each request its own engine.
//!
//! # How it works
//!
//! Memory is capped by the allocator (`Lua::set_memory_limit`). Instructions
//! and the deadline are checked by a native Lua count hook installed on the
//! main thread when the engine is created; Lua copies a thread's hook into
//! every thread created from it, so all coroutines are covered. (mlua's own
//! `set_hook` only fires for a single thread, which would miss coroutines.)
//!
//! When a limit is exceeded the hook raises an error and from then on fires
//! on every instruction of that thread, so guest code cannot catch the error
//! with `pcall` and keep running. An engine whose limits tripped should be
//! discarded.
//!
//! The deadline is checked while Lua code executes. Time spent awaiting host
//! futures is not interrupted here; hosts should also put a timeout around
//! the future they poll.
//!
//! # Long native calls
//!
//! The hook runs between Lua instructions, so a call into a C library
//! function is not interrupted while it runs. Pattern matching
//! (`string.find`, `match`, `gmatch`, `gsub`) is the one standard function
//! whose running time is not bounded by the size of its data, so the
//! engine replaces it with a matcher that charges its steps to the same
//! instruction budget and deadline (see `patterns`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use mlua::{ffi, Lua};

use crate::error::{LuatError, Result};

/// Instructions executed between two limit checks.
const CHECK_EVERY: i32 = 1000;

/// Limits applied to guest code run by an engine. `None` means unlimited.
#[derive(Debug, Clone, Default)]
pub struct EngineLimits {
    /// Maximum memory the Lua state may allocate, in bytes.
    pub memory_bytes: Option<usize>,
    /// Maximum number of Lua VM instructions (checked every 1000).
    pub instruction_budget: Option<u64>,
    /// Point in time after which running Lua code is interrupted.
    pub deadline: Option<Instant>,
}

/// Which limit stopped guest code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitExceeded {
    /// The memory limit was reached.
    Memory,
    /// The instruction budget was used up.
    Instructions,
    /// The deadline passed while Lua code was running.
    Deadline,
}

impl LimitExceeded {
    /// Identifies the limit behind `err`, if it was caused by one.
    pub fn from_error(err: &LuatError) -> Option<Self> {
        let message = match err {
            LuatError::LuaError(mlua::Error::MemoryError(_)) => return Some(Self::Memory),
            LuatError::LuaError(e) => e.to_string(),
            other => other.to_string(),
        };
        if message.contains(INSTRUCTIONS_MSG) {
            Some(Self::Instructions)
        } else if message.contains(DEADLINE_MSG) {
            Some(Self::Deadline)
        } else if message.contains("not enough memory") {
            Some(Self::Memory)
        } else {
            None
        }
    }
}

const INSTRUCTIONS_MSG: &str = "luat: instruction budget exceeded";
const DEADLINE_MSG: &str = "luat: execution deadline exceeded";
// NUL-terminated copies for the Lua C API (c"" literals need Rust 1.77).
const INSTRUCTIONS_CMSG: &[u8] = b"luat: instruction budget exceeded\0";
const DEADLINE_CMSG: &[u8] = b"luat: execution deadline exceeded\0";

const NOT_TRIPPED: u8 = 0;
const TRIPPED_INSTRUCTIONS: u8 = 1;
const TRIPPED_DEADLINE: u8 = 2;

/// Mutable limit state for one Lua state, shared with the hook.
#[derive(Debug)]
struct LimitState {
    /// Whether `remaining` is enforced.
    count_instructions: AtomicBool,
    /// Remaining instructions while `count_instructions` is set.
    remaining: AtomicI64,
    deadline: RwLock<Option<Instant>>,
    tripped: AtomicU8,
    /// Memory limit in bytes, 0 for none.
    memory_bytes: AtomicUsize,
}

impl LimitState {
    fn unlimited() -> Self {
        Self {
            count_instructions: AtomicBool::new(false),
            remaining: AtomicI64::new(0),
            deadline: RwLock::new(None),
            tripped: AtomicU8::new(NOT_TRIPPED),
            memory_bytes: AtomicUsize::new(0),
        }
    }
}

/// Limit state per Lua state, keyed by the state's allocator userdata,
/// which is unique per `Lua` instance and shared by all its threads.
fn registry() -> &'static RwLock<HashMap<usize, Arc<LimitState>>> {
    static REGISTRY: OnceLock<RwLock<HashMap<usize, Arc<LimitState>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

unsafe fn state_key(state: *mut ffi::lua_State) -> usize {
    let mut ud = std::ptr::null_mut();
    ffi::lua_getallocf(state, &mut ud);
    ud as usize
}

/// Installs the limit hook on `lua`'s main thread and registers unlimited
/// state for it. Call once, before the state runs any code, so that every
/// thread created afterwards inherits the hook.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    // SAFETY: we only read the main thread from the registry and set a hook
    // on it; the stack is balanced before returning.
    let key = unsafe {
        lua.exec_raw::<()>((), |state| {
            ffi::lua_rawgeti(state, ffi::LUA_REGISTRYINDEX, ffi::LUA_RIDX_MAINTHREAD);
            let main = ffi::lua_tothread(state, -1);
            ffi::lua_pop(state, 1);
            ffi::lua_sethook(main, Some(limit_hook), ffi::LUA_MASKCOUNT, CHECK_EVERY);
        })?;
        let mut key = 0usize;
        lua.exec_raw::<()>((), |state| key = state_key(state))?;
        key
    };
    registry()
        .write()
        .expect("limit registry poisoned")
        .insert(key, Arc::new(LimitState::unlimited()));
    Ok(())
}

/// Removes `lua`'s limit state. Called when the engine is dropped.
pub(crate) fn uninstall(lua: &Lua) {
    let mut key = 0usize;
    // SAFETY: reads the allocator userdata only.
    let ok = unsafe { lua.exec_raw::<()>((), |state| key = state_key(state)) }.is_ok();
    if ok {
        if let Ok(mut map) = registry().write() {
            map.remove(&key);
        }
    }
}

fn state_for(lua: &Lua) -> Result<Arc<LimitState>> {
    let mut key = 0usize;
    // SAFETY: reads the allocator userdata only.
    unsafe { lua.exec_raw::<()>((), |state| key = state_key(state))? };
    registry()
        .read()
        .expect("limit registry poisoned")
        .get(&key)
        .cloned()
        .ok_or_else(|| LuatError::InvalidTemplate("engine has no limit state".to_string()))
}

/// Applies `limits` to `lua`, replacing any previous limits.
pub(crate) fn apply(lua: &Lua, limits: &EngineLimits) -> Result<()> {
    let state = state_for(lua)?;
    if state.tripped.load(Ordering::Acquire) != NOT_TRIPPED {
        return Err(LuatError::InvalidTemplate(
            "limits already tripped; discard this engine".to_string(),
        ));
    }
    // mlua treats 0 as "no limit".
    lua.set_memory_limit(limits.memory_bytes.unwrap_or(0))?;
    state
        .memory_bytes
        .store(limits.memory_bytes.unwrap_or(0), Ordering::Release);
    match limits.instruction_budget {
        Some(budget) => {
            state
                .remaining
                .store(i64::try_from(budget).unwrap_or(i64::MAX), Ordering::Release);
            state.count_instructions.store(true, Ordering::Release);
        }
        None => state.count_instructions.store(false, Ordering::Release),
    }
    *state.deadline.write().expect("limit state poisoned") = limits.deadline;
    Ok(())
}

/// Reports which instruction or deadline limit has tripped, if any.
pub(crate) fn tripped(lua: &Lua) -> Option<LimitExceeded> {
    match state_for(lua).ok()?.tripped.load(Ordering::Acquire) {
        TRIPPED_INSTRUCTIONS => Some(LimitExceeded::Instructions),
        TRIPPED_DEADLINE => Some(LimitExceeded::Deadline),
        _ => None,
    }
}

/// Decides whether the current thread must stop. Kept separate from the
/// hook so that no value with a destructor is alive when the hook raises.
///
/// Fails closed: if the limit state cannot be read (poisoned lock), the
/// code is stopped rather than allowed to run unlimited.
fn check(state: *mut ffi::lua_State) -> Option<&'static [u8]> {
    // SAFETY: called from the hook with a valid state.
    let key = unsafe { state_key(state) };
    let limits = match registry().read() {
        Ok(map) => map.get(&key).cloned()?,
        Err(_) => return Some(INSTRUCTIONS_CMSG),
    };
    charge(&limits, CHECK_EVERY as i64).map(message_for)
}

/// Charges `cost` instructions and checks the deadline. Returns the
/// tripped limit, marking it so that every later check fails too.
fn charge(limits: &LimitState, cost: i64) -> Option<u8> {
    let tripped = limits.tripped.load(Ordering::Acquire);
    if tripped != NOT_TRIPPED {
        return Some(tripped);
    }

    if limits.count_instructions.load(Ordering::Acquire) {
        let before = limits.remaining.fetch_sub(cost, Ordering::AcqRel);
        if before <= cost {
            limits.tripped.store(TRIPPED_INSTRUCTIONS, Ordering::Release);
            return Some(TRIPPED_INSTRUCTIONS);
        }
    }

    let deadline = match limits.deadline.read() {
        Ok(deadline) => *deadline,
        Err(_) => return Some(TRIPPED_DEADLINE),
    };
    if deadline.is_some_and(|d| Instant::now() >= d) {
        limits.tripped.store(TRIPPED_DEADLINE, Ordering::Release);
        return Some(TRIPPED_DEADLINE);
    }
    None
}

/// Lets native code that runs long (the pattern matcher) charge its work
/// to the engine's limits.
#[derive(Debug, Clone)]
pub(crate) struct LimitGuard(Arc<LimitState>);

impl LimitGuard {
    /// The guard of `lua`'s limit state, if limits are installed.
    pub(crate) fn of(lua: &Lua) -> Option<Self> {
        state_for(lua).ok().map(Self)
    }

    /// Charges `steps` units of work as instructions. Returns the error
    /// message if a limit is (or already was) exceeded.
    pub(crate) fn charge(&self, steps: i64) -> Option<&'static str> {
        charge(&self.0, steps).map(|tripped| {
            if tripped == TRIPPED_DEADLINE {
                DEADLINE_MSG
            } else {
                INSTRUCTIONS_MSG
            }
        })
    }

    /// Bytes `lua` may still allocate under its memory limit, if any.
    pub(crate) fn memory_headroom(&self, lua: &Lua) -> Option<usize> {
        let limit = self.0.memory_bytes.load(Ordering::Acquire);
        (limit != 0).then(|| limit.saturating_sub(lua.used_memory()))
    }
}

/// Makes the hook fire on the very next instruction of the running thread,
/// as it does after it raised a limit error itself. Native code that reports
/// a tripped limit calls this so a guest `pcall` cannot keep running.
pub(crate) fn halt_current_thread(lua: &Lua) {
    // SAFETY: only sets the hook of the currently running thread.
    let _ = unsafe {
        lua.exec_raw::<()>((), |state| {
            ffi::lua_sethook(state, Some(limit_hook), ffi::LUA_MASKCOUNT, 1);
        })
    };
}

fn message_for(tripped: u8) -> &'static [u8] {
    if tripped == TRIPPED_DEADLINE {
        DEADLINE_CMSG
    } else {
        INSTRUCTIONS_CMSG
    }
}

unsafe extern "C-unwind" fn limit_hook(state: *mut ffi::lua_State, _ar: *mut ffi::lua_Debug) {
    if let Some(message) = check(state) {
        // From now on stop at the very next instruction, so a guest pcall
        // that catches this error cannot keep running.
        ffi::lua_sethook(state, Some(limit_hook), ffi::LUA_MASKCOUNT, 1);
        ffi::lua_pushstring(state, message.as_ptr().cast());
        ffi::lua_error(state);
    }
}
