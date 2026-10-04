// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `string.gsub`.

use mlua::{Lua, MultiValue, Value};

use super::args::{arg, bad_arg, check_str, opt_int, truthy, type_name};
use super::captures::Cap;
use super::find::cap_value;
use super::matcher::MatchState;
use super::{Ctx, Raise, Ret};

/// What replaces each match.
enum Repl {
    Str(mlua::String),
    Table(mlua::Table),
    Func(mlua::Function),
}

/// The result being built, capped at the memory the engine may still use,
/// since it is held outside the Lua allocator until the end.
struct Output {
    buf: Vec<u8>,
    cap: Option<usize>,
}

impl Output {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Raise> {
        self.buf.extend_from_slice(bytes);
        match self.cap {
            Some(cap) if self.buf.len() > cap => Err(Raise::Memory),
            _ => Ok(()),
        }
    }
}

pub(super) fn gsub(lua: &Lua, ctx: &Ctx, args: MultiValue) -> Ret {
    let s = check_str(lua, &args, 1, "gsub")?;
    let p = check_str(lua, &args, 2, "gsub")?;
    let (src_bytes, pat_bytes) = (s.as_bytes(), p.as_bytes());
    let (src, pat): (&[u8], &[u8]) = (&src_bytes, &pat_bytes);
    let default_max = i64::try_from(src.len())
        .unwrap_or(i64::MAX)
        .saturating_add(1);
    let max = opt_int(lua, &args, 4, "gsub", default_max)?;
    let repl = match arg(&args, 3) {
        Some(Value::String(r)) => Repl::Str(r.clone()),
        Some(v @ (Value::Integer(_) | Value::Number(_))) => Repl::Str(
            lua.coerce_string(v.clone())?
                .expect("numbers convert to strings"),
        ),
        Some(Value::Table(t)) => Repl::Table(t.clone()),
        Some(Value::Function(f)) => Repl::Func(f.clone()),
        other => {
            let message = format!("string/function/table expected, got {}", type_name(other));
            return Err(bad_arg(3, "gsub", &message));
        }
    };

    let (anchor, pat) = match pat.split_first() {
        Some((b'^', rest)) => (true, rest),
        _ => (false, pat),
    };
    let mut out = Output {
        buf: Vec::new(),
        cap: ctx.guard.as_ref().and_then(|g| g.memory_headroom(lua)),
    };
    let mut meter = ctx.meter()?;
    let mut ms = MatchState::new(src, pat, &mut meter);
    let (mut pos, mut last, mut count, mut changed) = (0usize, None, 0i64, false);
    while count < max {
        match ms.match_at(pos)?.filter(|&end| Some(end) != last) {
            Some(end) => {
                count += 1;
                changed |= add_value(lua, ctx, &mut ms, &mut out, &repl, pos, end)?;
                pos = end;
                last = Some(end);
            }
            None if pos < src.len() => {
                out.push(&src[pos..=pos])?;
                pos += 1;
            }
            None => break,
        }
        if anchor {
            break;
        }
    }

    let result = if changed {
        out.push(&src[pos..])?;
        Value::String(lua.create_string(&out.buf)?)
    } else {
        Value::String(s.clone())
    };
    Ok(MultiValue::from_vec(vec![result, Value::Integer(count)]))
}

/// Appends the replacement for the match `s..e`. Returns whether the
/// original text was replaced (false, nil and missing values keep it).
fn add_value(
    lua: &Lua,
    ctx: &Ctx,
    ms: &mut MatchState,
    out: &mut Output,
    repl: &Repl,
    s: usize,
    e: usize,
) -> Result<bool, Raise> {
    let src = ms.src;
    let value = match repl {
        Repl::Str(r) => {
            add_string(ms, out, &r.as_bytes(), s, e)?;
            return Ok(true);
        }
        Repl::Table(t) => {
            let key = cap_value(lua, src, ms.capture(0, s, e)?)?;
            protected(
                ctx,
                &ctx.index,
                MultiValue::from_vec(vec![Value::Table(t.clone()), key]),
            )?
        }
        Repl::Func(f) => {
            let mut caps = Vec::new();
            for cap in ms.captures(Some((s, e)))? {
                caps.push(cap_value(lua, src, cap)?);
            }
            protected(ctx, f, MultiValue::from_vec(caps))?
        }
    };
    if !truthy(&value) {
        out.push(&src[s..e])?;
        return Ok(false);
    }
    match value {
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            let text = lua
                .coerce_string(value)?
                .expect("strings and numbers convert");
            out.push(&text.as_bytes())?;
            ms.meter().tick()?;
            Ok(true)
        }
        other => Err(Raise::Msg(format!(
            "invalid replacement value (a {})",
            type_name(Some(&other))
        ))),
    }
}

/// Runs `callee(args...)` under `pcall`, so that an error raised by guest
/// code is passed on as the very value it raised.
fn protected(ctx: &Ctx, callee: &mlua::Function, args: MultiValue) -> Result<Value, Raise> {
    let mut call = vec![Value::Function(callee.clone())];
    call.extend(args);
    let mut results = ctx
        .pcall
        .call::<MultiValue>(MultiValue::from_vec(call))?
        .into_iter();
    let ok = results.next().is_some_and(|v| truthy(&v));
    let value = results.next().unwrap_or(Value::Nil);
    if ok {
        Ok(value)
    } else {
        Err(Raise::Rethrow(value))
    }
}

/// Appends a replacement string, expanding `%0`-`%9` and `%%`.
fn add_string(
    ms: &mut MatchState,
    out: &mut Output,
    repl: &[u8],
    s: usize,
    e: usize,
) -> Result<(), Raise> {
    let src = ms.src;
    let mut rest = repl;
    while let Some(at) = memchr::memchr(b'%', rest) {
        out.push(&rest[..at])?;
        match rest.get(at + 1) {
            Some(b'%') => out.push(b"%")?,
            Some(b'0') => out.push(&src[s..e])?,
            Some(&d) if d.is_ascii_digit() => match ms.capture((d - b'1') as usize, s, e)? {
                Cap::Str(a, b) => out.push(&src[a..b])?,
                Cap::Pos(pos) => out.push(pos.to_string().as_bytes())?,
            },
            _ => {
                return Err(Raise::Msg(
                    "invalid use of '%' in replacement string".into(),
                ))
            }
        }
        rest = &rest[at + 2..];
    }
    out.push(rest)?;
    ms.meter().add(repl.len())?;
    Ok(())
}
