// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Argument checks with the conversions and messages of Lua's auxiliary
//! library (`luaL_checklstring`, `luaL_optinteger`).

use mlua::{Lua, MultiValue, Value};

use super::Raise;

/// Argument `i` (1-based), or `None` if it was not passed.
pub(super) fn arg(args: &MultiValue, i: usize) -> Option<&Value> {
    args.get(i - 1)
}

/// Lua's truthiness: everything but `nil` and `false`.
pub(super) fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// The type name Lua's `type()` reports, or "no value" for a missing one.
pub(super) fn type_name(value: Option<&Value>) -> &'static str {
    match value {
        None => "no value",
        Some(Value::Integer(_) | Value::Number(_)) => "number",
        Some(Value::LightUserData(_) | Value::UserData(_) | Value::Error(_)) => "userdata",
        Some(other) => other.type_name(),
    }
}

pub(super) fn bad_arg(i: usize, fname: &str, message: &str) -> Raise {
    Raise::Msg(format!("bad argument #{i} to '{fname}' ({message})"))
}

/// A string argument; numbers are converted as Lua converts them.
pub(super) fn check_str(
    lua: &Lua,
    args: &MultiValue,
    i: usize,
    fname: &str,
) -> Result<mlua::String, Raise> {
    match arg(args, i) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(v @ (Value::Integer(_) | Value::Number(_))) => lua
            .coerce_string(v.clone())?
            .ok_or_else(|| bad_arg(i, fname, "string expected, got number")),
        other => Err(bad_arg(
            i,
            fname,
            &format!("string expected, got {}", type_name(other)),
        )),
    }
}

/// An optional integer argument.
pub(super) fn opt_int(
    lua: &Lua,
    args: &MultiValue,
    i: usize,
    fname: &str,
    default: i64,
) -> Result<i64, Raise> {
    let no_int = || bad_arg(i, fname, "number has no integer representation");
    match arg(args, i) {
        None | Some(Value::Nil) => Ok(default),
        Some(Value::Integer(n)) => Ok(*n),
        Some(Value::Number(f)) => float_to_int(*f).ok_or_else(no_int),
        Some(v @ Value::String(_)) => match lua.coerce_number(v.clone())? {
            Some(f) => match lua.coerce_integer(v.clone())? {
                Some(n) => Ok(n),
                None => float_to_int(f).ok_or_else(no_int),
            },
            None => Err(bad_arg(i, fname, "number expected, got string")),
        },
        other => Err(bad_arg(
            i,
            fname,
            &format!("number expected, got {}", type_name(other)),
        )),
    }
}

/// A float that is exactly an integer, as Lua converts it.
fn float_to_int(f: f64) -> Option<i64> {
    // 2^63 is exactly representable; i64 covers [-2^63, 2^63).
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    (f.fract() == 0.0 && (-LIMIT..LIMIT).contains(&f)).then_some(f as i64)
}

/// Converts a 1-based, possibly negative start position into a 1-based
/// position clipped to at least 1 (`posrelatI`).
pub(super) fn start_position(pos: i64, len: usize) -> usize {
    let len_i = i64::try_from(len).unwrap_or(i64::MAX);
    if pos > 0 {
        usize::try_from(pos).unwrap_or(usize::MAX)
    } else if pos == 0 || pos < -len_i {
        1
    } else {
        (len_i + pos + 1) as usize
    }
}
