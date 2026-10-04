// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Lua 5.4 pattern matching that respects the engine's limits.
//!
//! Lua's own `string.find`, `match`, `gmatch` and `gsub` run entirely in C,
//! where the instruction hook cannot reach them, and a pattern such as
//! `string.rep("a*", 40) .. "b"` backtracks for hours. The engine replaces
//! them with a port of the same matcher that charges every step to the
//! instruction budget and checks the deadline (see [`crate::limits`]), so
//! such a pattern fails with an ordinary limit error.
//!
//! Semantics follow Lua 5.4: the same classes, sets, anchors, captures,
//! `%b`, `%f`, back-references, recursion limit and error messages.
//!
//! The Rust functions report errors by returning a marker, which a small
//! Lua wrapper turns into `error(message, level)`: guest code then sees
//! plain string errors with the caller's position, as with the C library,
//! and errors raised by a `gsub` replacement function pass through
//! unchanged.

mod args;
mod captures;
mod class;
mod find;
mod gsub;
mod matcher;
mod meter;

use std::sync::Arc;

use mlua::{Function, Lua, MultiValue, Table, Value};

use matcher::PatError;
use meter::{Guard, Meter};

/// Why a pattern function stopped, other than returning.
pub(super) enum Raise {
    /// A Lua-style error message, raised at the caller's position.
    Msg(String),
    /// An engine limit was exceeded.
    Limit(&'static str),
    /// An error raised by guest code, passed on as is.
    Rethrow(Value),
    /// The result would exceed the memory limit.
    Memory,
    /// An error from the Lua API itself.
    Lua(mlua::Error),
}

impl From<PatError> for Raise {
    fn from(err: PatError) -> Self {
        match err {
            PatError::Pattern(message) => Self::Msg(message),
            PatError::Limit(message) => Self::Limit(message),
        }
    }
}

impl From<mlua::Error> for Raise {
    fn from(err: mlua::Error) -> Self {
        Self::Lua(err)
    }
}

pub(super) type Ret = Result<MultiValue, Raise>;

/// Shared by the pattern functions of one Lua state.
pub(super) struct Ctx {
    /// Marker the wrapper recognises as "raise the next value".
    fail: Table,
    guard: Option<Guard>,
    /// The original `pcall`, captured before guest code runs.
    pcall: Function,
    /// `function(t, k) return t[k] end`, for table replacements.
    index: Function,
}

impl Ctx {
    fn meter(&self) -> Result<Meter<'_>, Raise> {
        Ok(Meter::new(self.guard.as_ref())?)
    }

    /// Turns a result into what the Lua wrapper expects.
    fn finish(&self, lua: &Lua, result: Ret) -> mlua::Result<MultiValue> {
        let (value, level) = match result {
            Ok(values) => return Ok(values),
            Err(Raise::Lua(err)) => return Err(err),
            Err(Raise::Msg(message)) => (Value::String(lua.create_string(message)?), 2),
            Err(Raise::Limit(message)) => {
                #[cfg(not(target_arch = "wasm32"))]
                crate::limits::halt_current_thread(lua);
                (Value::String(lua.create_string(message)?), 0)
            }
            Err(Raise::Rethrow(value)) => (value, 0),
            Err(Raise::Memory) => (Value::String(lua.create_string("not enough memory")?), 0),
        };
        Ok(MultiValue::from_vec(vec![
            Value::Table(self.fail.clone()),
            value,
            Value::Integer(level),
        ]))
    }
}

/// Turns the marker returned by the Rust functions into a Lua error. With
/// level 2, `error` blames the caller of the string function, because the
/// wrappers call `out` (or `error`) in tail position or directly.
const WRAPPER: &str = r#"
local fail, find, match, gmatch, gsub = ...
local error, rawequal = error, rawequal
local function out(...)
  if rawequal((...), fail) then
    local _, message, level = ...
    error(message, level)
  end
  return ...
end
local lib = {}
function lib.find(...) return out(find(...)) end
function lib.match(...) return out(match(...)) end
function lib.gsub(...) return out(gsub(...)) end
function lib.gmatch(...)
  local iter, message, level = gmatch(...)
  if rawequal(iter, fail) then error(message, level) end
  return function() return out(iter()) end
end
return lib
"#;

/// Replaces the pattern functions of `lua`'s `string` table (which is also
/// the `__index` of the string metatable, so `s:find()` is covered too).
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let guard = crate::limits::LimitGuard::of(lua);
    #[cfg(target_arch = "wasm32")]
    let guard = None;

    let ctx = Arc::new(Ctx {
        fail: lua.create_table()?,
        guard,
        pcall: lua.globals().get("pcall")?,
        index: lua
            .load("local t, k = ...; return t[k]")
            .set_name("=luat_patterns")
            .into_function()?,
    });

    let make = |f: fn(&Lua, &Arc<Ctx>, MultiValue) -> Ret| {
        let ctx = Arc::clone(&ctx);
        lua.create_function(move |lua, args: MultiValue| {
            let result = f(lua, &ctx, args);
            ctx.finish(lua, result)
        })
    };
    let lib: Table = lua.load(WRAPPER).set_name("=luat_patterns").call((
        ctx.fail.clone(),
        make(|lua, ctx, args| find::find(lua, ctx, args, true))?,
        make(|lua, ctx, args| find::find(lua, ctx, args, false))?,
        make(find::gmatch)?,
        make(|lua, ctx, args| gsub::gsub(lua, ctx, args))?,
    ))?;

    let string: Table = lua.globals().get("string")?;
    for name in ["find", "match", "gmatch", "gsub"] {
        string.raw_set(name, lib.raw_get::<Function>(name)?)?;
    }
    Ok(())
}
