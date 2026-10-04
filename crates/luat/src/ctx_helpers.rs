// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Request-scoped helpers shared by load functions, API handlers and form
//! actions: `ctx.setCookie`, `ctx.deleteCookie` and `ctx.error`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mlua::{Lua, Result as LuaResult, Table, Value};

/// An error raised by guest code with `ctx.error(status, message)`.
///
/// The engine turns it into a response with that status. Its message is
/// meant for clients, unlike internal errors, which are hidden outside
/// development mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{status} {message}")]
pub struct HttpError {
    /// HTTP status code (400-599).
    pub status: u16,
    /// Message shown to clients.
    pub message: String,
}

impl HttpError {
    /// Finds an `HttpError` raised anywhere in `err`'s cause chain.
    pub fn find(err: &mlua::Error) -> Option<&HttpError> {
        match err {
            mlua::Error::ExternalError(ext) => ext.downcast_ref::<HttpError>(),
            mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
                Self::find(cause)
            }
            _ => None,
        }
    }
}

/// `Set-Cookie` values collected while one request is handled.
#[derive(Debug, Clone, Default)]
pub struct CookieJar(Arc<Mutex<Vec<String>>>);

impl CookieJar {
    /// Creates an empty jar.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&self, header: String) {
        if let Ok(mut cookies) = self.0.lock() {
            cookies.push(header);
        }
    }

    /// Returns the collected `Set-Cookie` header values.
    pub fn take(&self) -> Vec<String> {
        self.0.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default()
    }
}

/// Installs `setCookie`, `deleteCookie` and `error` on a request `ctx` table.
pub(crate) fn install(lua: &Lua, ctx: &Table, jar: &CookieJar) -> LuaResult<()> {
    let set_jar = jar.clone();
    ctx.set(
        "setCookie",
        lua.create_function(move |_, (name, value, opts): (String, String, Option<Table>)| {
            let opts = CookieOptions::from_lua(opts.as_ref())?;
            set_jar.push(serialize_cookie(&name, &value, &opts).map_err(mlua::Error::runtime)?);
            Ok(())
        })?,
    )?;

    let delete_jar = jar.clone();
    ctx.set(
        "deleteCookie",
        lua.create_function(move |_, (name, opts): (String, Option<Table>)| {
            let mut opts = CookieOptions::from_lua(opts.as_ref())?;
            opts.max_age = Some(0);
            delete_jar.push(serialize_cookie(&name, "", &opts).map_err(mlua::Error::runtime)?);
            Ok(())
        })?,
    )?;

    ctx.set(
        "error",
        lua.create_function(|_, (status, message): (u16, Option<String>)| -> LuaResult<()> {
            if !(400..=599).contains(&status) {
                return Err(mlua::Error::runtime(format!(
                    "ctx.error: status must be 400-599, got {status}"
                )));
            }
            let message = message.unwrap_or_else(|| default_reason(status).to_string());
            Err(mlua::Error::external(HttpError { status, message }))
        })?,
    )?;
    Ok(())
}

/// Attributes accepted by `ctx.setCookie` / `ctx.deleteCookie`.
#[derive(Debug, Clone)]
struct CookieOptions {
    path: String,
    domain: Option<String>,
    max_age: Option<i64>,
    secure: bool,
    http_only: bool,
    same_site: Option<String>,
}

impl CookieOptions {
    fn from_lua(opts: Option<&Table>) -> LuaResult<Self> {
        let mut o = Self {
            path: "/".to_string(),
            domain: None,
            max_age: None,
            secure: false,
            http_only: true,
            same_site: Some("Lax".to_string()),
        };
        let Some(t) = opts else { return Ok(o) };
        if let Some(path) = t.get::<Option<String>>("path")? {
            o.path = path;
        }
        o.domain = t.get("domain")?;
        o.max_age = t.get("maxAge")?;
        if let Some(secure) = t.get::<Option<bool>>("secure")? {
            o.secure = secure;
        }
        if let Some(http_only) = t.get::<Option<bool>>("httpOnly")? {
            o.http_only = http_only;
        }
        match t.get::<Value>("sameSite")? {
            Value::Nil => {}
            Value::Boolean(false) => o.same_site = None,
            Value::String(s) => o.same_site = Some(s.to_str()?.to_string()),
            other => {
                return Err(mlua::Error::runtime(format!(
                    "setCookie: sameSite must be a string or false, got {}",
                    other.type_name()
                )))
            }
        }
        Ok(o)
    }
}

/// Builds a `Set-Cookie` header value. The value is percent-encoded where
/// RFC 6265 does not allow the byte; attributes are validated.
fn serialize_cookie(name: &str, value: &str, o: &CookieOptions) -> Result<String, String> {
    if name.is_empty() || !name.bytes().all(is_token_byte) {
        return Err(format!("setCookie: invalid cookie name {name:?}"));
    }
    for (attr, v) in [("path", Some(o.path.as_str())), ("domain", o.domain.as_deref())] {
        if v.is_some_and(|v| v.bytes().any(|b| b == b';' || b.is_ascii_control())) {
            return Err(format!("setCookie: invalid {attr}"));
        }
    }

    let mut out = format!("{name}={}", encode_cookie_value(value));
    out.push_str(&format!("; Path={}", o.path));
    if let Some(domain) = &o.domain {
        out.push_str(&format!("; Domain={domain}"));
    }
    if let Some(max_age) = o.max_age {
        out.push_str(&format!("; Max-Age={max_age}"));
    }
    if let Some(same_site) = &o.same_site {
        let same_site = match same_site.to_ascii_lowercase().as_str() {
            "lax" => "Lax",
            "strict" => "Strict",
            "none" => "None",
            other => return Err(format!("setCookie: invalid sameSite {other:?}")),
        };
        out.push_str(&format!("; SameSite={same_site}"));
    }
    if o.secure {
        out.push_str("; Secure");
    }
    if o.http_only {
        out.push_str("; HttpOnly");
    }
    Ok(out)
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?={}".contains(&b)
}

fn encode_cookie_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        let allowed = b == 0x21
            || (0x23..=0x2B).contains(&b)
            || (0x2D..=0x3A).contains(&b)
            || (0x3C..=0x5B).contains(&b)
            || (0x5D..=0x7E).contains(&b);
        if allowed && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parses a `Cookie` request header into name/value pairs, decoding
/// percent-escapes. Malformed pairs are skipped; the first value wins.
pub fn parse_cookie_header(header: &str) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for pair in header.split(';') {
        let Some((name, value)) = pair.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let value = value.trim().trim_matches('"');
        cookies
            .entry(name.to_string())
            .or_insert_with(|| decode_percent(value));
    }
    cookies
}

fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reason phrase used when `ctx.error` gets no message.
pub(crate) fn default_reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ if status < 500 => "Client Error",
        _ => "Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> CookieOptions {
        CookieOptions::from_lua(None).unwrap()
    }

    #[test]
    fn serializes_with_safe_defaults() {
        assert_eq!(
            serialize_cookie("sid", "abc", &defaults()).unwrap(),
            "sid=abc; Path=/; SameSite=Lax; HttpOnly"
        );
    }

    #[test]
    fn encodes_unsafe_value_bytes() {
        let header = serialize_cookie("n", "a b;c,%", &defaults()).unwrap();
        assert!(header.starts_with("n=a%20b%3Bc%2C%25;"), "{header}");
    }

    #[test]
    fn rejects_bad_names_and_attributes() {
        assert!(serialize_cookie("a;b", "v", &defaults()).is_err());
        let mut o = defaults();
        o.path = "/x; Domain=evil".to_string();
        assert!(serialize_cookie("a", "v", &o).is_err());
        o = defaults();
        o.same_site = Some("sometimes".to_string());
        assert!(serialize_cookie("a", "v", &o).is_err());
    }

    #[test]
    fn parses_and_decodes_cookie_header() {
        let c = parse_cookie_header("a=1; b=x%20y; a=2; junk; c=\"q\"");
        assert_eq!(c["a"], "1");
        assert_eq!(c["b"], "x y");
        assert_eq!(c["c"], "q");
        assert!(!c.contains_key("junk"));
    }

    #[test]
    fn round_trips_encoded_values() {
        let header = serialize_cookie("n", "a b;é", &defaults()).unwrap();
        let value = header.split(';').next().unwrap().split_once('=').unwrap().1;
        assert_eq!(parse_cookie_header(&format!("n={value}"))["n"], "a b;é");
    }
}
