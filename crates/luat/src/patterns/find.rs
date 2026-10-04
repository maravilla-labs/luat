// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `string.find`, `string.match` and `string.gmatch`.

use std::cell::Cell;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Value};

use super::args::{arg, check_str, opt_int, start_position, truthy};
use super::captures::Cap;
use super::matcher::MatchState;
use super::{Ctx, Ret};

/// Bytes that make a pattern more than a plain string.
const SPECIALS: &[u8] = b"^$*+?.([%-";

fn values(values: Vec<Value>) -> MultiValue {
    MultiValue::from_vec(values)
}

fn fail() -> MultiValue {
    values(vec![Value::Nil])
}

fn index(i: usize) -> Value {
    Value::Integer(i64::try_from(i).unwrap_or(i64::MAX))
}

/// A capture as a Lua value: the captured text, or a position.
pub(super) fn cap_value(lua: &Lua, src: &[u8], cap: Cap) -> mlua::Result<Value> {
    match cap {
        Cap::Str(start, end) => Ok(Value::String(lua.create_string(&src[start..end])?)),
        Cap::Pos(pos) => Ok(index(pos)),
    }
}

/// `string.find` (`find == true`) and `string.match`.
pub(super) fn find(lua: &Lua, ctx: &Ctx, args: MultiValue, find: bool) -> Ret {
    let fname = if find { "find" } else { "match" };
    let s = check_str(lua, &args, 1, fname)?;
    let p = check_str(lua, &args, 2, fname)?;
    let (src_bytes, pat_bytes) = (s.as_bytes(), p.as_bytes());
    let (src, pat): (&[u8], &[u8]) = (&src_bytes, &pat_bytes);
    let init = start_position(opt_int(lua, &args, 3, fname, 1)?, src.len()) - 1;
    if init > src.len() {
        return Ok(fail());
    }

    let plain = arg(&args, 4).is_some_and(truthy);
    if find && (plain || !pat.iter().any(|b| SPECIALS.contains(b))) {
        // Two-way search: linear in the subject, so it needs no metering.
        return Ok(match memchr::memmem::find(&src[init..], pat) {
            Some(at) => values(vec![index(init + at + 1), index(init + at + pat.len())]),
            None => fail(),
        });
    }

    let (anchor, pat) = match pat.split_first() {
        Some((b'^', rest)) => (true, rest),
        _ => (false, pat),
    };
    let mut meter = ctx.meter()?;
    let mut ms = MatchState::new(src, pat, &mut meter);
    let mut s1 = init;
    loop {
        if let Some(end) = ms.match_at(s1)? {
            let caps = ms.captures(if find { None } else { Some((s1, end)) })?;
            let mut out = Vec::with_capacity(caps.len() + 2);
            if find {
                out.push(index(s1 + 1));
                out.push(index(end));
            }
            for cap in caps {
                out.push(cap_value(lua, src, cap)?);
            }
            return Ok(values(out));
        }
        if anchor || s1 >= src.len() {
            return Ok(fail());
        }
        s1 += 1;
    }
}

/// `string.gmatch`: returns an iterator over the matches.
pub(super) fn gmatch(lua: &Lua, ctx: &Arc<Ctx>, args: MultiValue) -> Ret {
    let s = check_str(lua, &args, 1, "gmatch")?;
    let p = check_str(lua, &args, 2, "gmatch")?;
    let len = s.as_bytes().len();
    let init = start_position(opt_int(lua, &args, 3, "gmatch", 1)?, len) - 1;
    // Next start position and end of the last match.
    let state = Cell::new((init.min(len + 1), None::<usize>));
    let ctx = Arc::clone(ctx);
    let iter = lua.create_function(move |lua, _: MultiValue| {
        let result = gmatch_next(lua, &ctx, &s, &p, &state);
        ctx.finish(lua, result)
    })?;
    Ok(values(vec![Value::Function(iter)]))
}

fn gmatch_next(
    lua: &Lua,
    ctx: &Ctx,
    s: &mlua::String,
    p: &mlua::String,
    state: &Cell<(usize, Option<usize>)>,
) -> Ret {
    let (src_bytes, pat_bytes) = (s.as_bytes(), p.as_bytes());
    let (src, pat): (&[u8], &[u8]) = (&src_bytes, &pat_bytes);
    let (mut start, last) = state.get();
    let mut meter = ctx.meter()?;
    let mut ms = MatchState::new(src, pat, &mut meter);
    while start <= src.len() {
        // A match may not end where the previous one ended (no empty
        // match right after a match).
        if let Some(end) = ms.match_at(start)?.filter(|&end| Some(end) != last) {
            state.set((end, Some(end)));
            let caps = ms.captures(Some((start, end)))?;
            let out = caps
                .into_iter()
                .map(|cap| cap_value(lua, src, cap))
                .collect::<mlua::Result<Vec<_>>>()?;
            return Ok(values(out));
        }
        start += 1;
    }
    state.set((start, last));
    Ok(MultiValue::new())
}
