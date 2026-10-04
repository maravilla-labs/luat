// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Request-scoped helpers shared by load functions, API handlers and form
//! actions: `ctx.setCookie`, `ctx.deleteCookie`, `ctx.setHeader`,
//! `ctx.appendHeader`, `ctx.setStatus` and `ctx.error`.

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

/// Response state collected while one request is handled: `Set-Cookie`
/// values from `ctx.setCookie` / `ctx.deleteCookie`, headers from
/// `ctx.setHeader` / `ctx.appendHeader`, and the status from
/// `ctx.setStatus`.
#[derive(Debug, Clone, Default)]
pub struct CookieJar(Arc<Mutex<JarState>>);

#[derive(Debug, Default)]
struct JarState {
    cookies: Vec<String>,
    headers: Vec<(String, String)>,
    status: Option<u16>,
}

impl CookieJar {
    /// Creates an empty jar.
    pub fn new() -> Self {
        Self::default()
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut JarState) -> T) -> Option<T> {
        self.0.lock().ok().map(|mut state| f(&mut state))
    }

    fn push(&self, header: String) {
        self.with_state(|s| s.cookies.push(header));
    }

    /// Returns the collected `Set-Cookie` header values.
    pub fn take(&self) -> Vec<String> {
        self.with_state(|s| std::mem::take(&mut s.cookies)).unwrap_or_default()
    }

    /// Sets header `name` to `value`, replacing values set earlier for the
    /// same name (ignoring case). The jar does not validate; the `ctx`
    /// helpers check names and values before they get here.
    pub fn set_header(&self, name: &str, value: &str) {
        self.with_state(|s| {
            s.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            s.headers.push((name.to_string(), value.to_string()));
        });
    }

    /// Adds another value for header `name`, keeping earlier ones.
    pub fn append_header(&self, name: &str, value: &str) {
        self.with_state(|s| s.headers.push((name.to_string(), value.to_string())));
    }

    /// Returns the collected headers, in the order they were set.
    pub fn take_headers(&self) -> Vec<(String, String)> {
        self.with_state(|s| std::mem::take(&mut s.headers)).unwrap_or_default()
    }

    /// Sets the response status. Not validated here, like `set_header`.
    pub fn set_status(&self, status: u16) {
        self.with_state(|s| s.status = Some(status));
    }

    /// The status set with `ctx.setStatus` (or a load function's `status`),
    /// if any.
    pub fn status(&self) -> Option<u16> {
        self.with_state(|s| s.status).flatten()
    }
}

/// Which request handler a `ctx` table is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandlerKind {
    /// A page or layout `load` function: `ctx.setStatus` is available.
    Load,
    /// An API handler or form action: the returned `status` sets the status.
    Other,
}

/// Installs `setCookie`, `deleteCookie`, `setHeader`, `appendHeader`,
/// `setStatus` and `error` on a request `ctx` table.
pub(crate) fn install(lua: &Lua, ctx: &Table, jar: &CookieJar, kind: HandlerKind) -> LuaResult<()> {
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

    let header_jar = jar.clone();
    ctx.set(
        "setHeader",
        lua.create_function(move |_, (name, value): (String, String)| {
            validate_header("setHeader", &name, &value).map_err(mlua::Error::runtime)?;
            header_jar.set_header(&name, &value);
            Ok(())
        })?,
    )?;

    let append_jar = jar.clone();
    ctx.set(
        "appendHeader",
        lua.create_function(move |_, (name, value): (String, String)| {
            validate_header("appendHeader", &name, &value).map_err(mlua::Error::runtime)?;
            append_jar.append_header(&name, &value);
            Ok(())
        })?,
    )?;

    let status_jar = jar.clone();
    ctx.set(
        "setStatus",
        lua.create_function(move |_, status: i64| {
            if kind != HandlerKind::Load {
                return Err(mlua::Error::runtime(
                    "ctx.setStatus is only available in load functions; API handlers and actions return { status = ... }",
                ));
            }
            let status = validate_status(status).map_err(mlua::Error::runtime)?;
            status_jar.set_status(status);
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

/// Sets `ctx.path` (the request path), `ctx.search` (the query string with
/// its `?`, or `""`) and `ctx.href` (path followed by search).
pub(crate) fn install_url(ctx: &Table, path: &str, search: &str) -> LuaResult<()> {
    ctx.set("path", path)?;
    ctx.set("search", search)?;
    ctx.set("href", format!("{path}{search}"))
}

/// Headers handlers may not set with `ctx.setHeader`: cookies go through
/// `ctx.setCookie`, redirects through `redirect`, and framing headers belong
/// to the server.
const RESERVED_HEADERS: &[&str] = &[
    "set-cookie",
    "location",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "trailer",
];

/// Checks a header set from guest code: the name must be an HTTP token and
/// not reserved (see `RESERVED_HEADERS`, plus the engine's `x-luat-*`
/// headers); the value must not contain control characters other than
/// tab, which rules out CR/LF header injection.
pub(crate) fn validate_header(func: &str, name: &str, value: &str) -> Result<(), String> {
    if name.is_empty() || !name.bytes().all(is_token_byte) {
        return Err(format!("ctx.{func}: invalid header name {name:?}"));
    }
    let lower = name.to_ascii_lowercase();
    if lower == "set-cookie" {
        return Err(format!("ctx.{func}: use ctx.setCookie to set cookies"));
    }
    if RESERVED_HEADERS.contains(&lower.as_str()) || lower.starts_with("x-luat-") {
        return Err(format!("ctx.{func}: header {name:?} cannot be set by handlers"));
    }
    if value.bytes().any(|b| (b.is_ascii_control() && b != b'\t') || b == 0x7f) {
        return Err(format!("ctx.{func}: invalid value for header {name:?}"));
    }
    Ok(())
}

/// Checks a status passed to `ctx.setStatus`: 200-299 or 400-599.
/// Redirects use `redirect`, errors may also use `ctx.error`.
pub(crate) fn validate_status(status: i64) -> Result<u16, String> {
    match status {
        200..=299 | 400..=599 => Ok(status as u16),
        300..=399 => Err(format!(
            "ctx.setStatus: {status} is a redirect status; return {{ redirect = url, status = {status} }} instead"
        )),
        _ => Err(format!("ctx.setStatus: status must be 200-299 or 400-599, got {status}")),
    }
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
    partitioned: bool,
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
            partitioned: false,
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
        if let Some(partitioned) = t.get::<Option<bool>>("partitioned")? {
            o.partitioned = partitioned;
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
        if same_site == "None" && !o.secure {
            return Err("setCookie: sameSite = \"None\" requires secure = true".to_string());
        }
        out.push_str(&format!("; SameSite={same_site}"));
    }
    if o.partitioned && !o.secure {
        return Err("setCookie: partitioned cookies require secure = true".to_string());
    }
    if o.secure {
        out.push_str("; Secure");
    }
    if o.http_only {
        out.push_str("; HttpOnly");
    }
    if o.partitioned {
        out.push_str("; Partitioned");
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

/// Decodes `%XX` escapes; invalid escapes are kept as-is.
pub(crate) fn decode_percent(value: &str) -> String {
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
    fn partitioned_cookies_need_secure() {
        let mut o = defaults();
        o.partitioned = true;
        assert!(serialize_cookie("a", "v", &o).is_err());
        o.secure = true;
        o.same_site = Some("none".to_string());
        assert_eq!(
            serialize_cookie("a", "v", &o).unwrap(),
            "a=v; Path=/; SameSite=None; Secure; HttpOnly; Partitioned"
        );
    }

    #[test]
    fn same_site_none_needs_secure() {
        let mut o = defaults();
        o.same_site = Some("None".to_string());
        assert!(serialize_cookie("a", "v", &o).is_err());
        o.secure = true;
        assert!(serialize_cookie("a", "v", &o).is_ok());
    }

    #[test]
    fn header_validation() {
        assert!(validate_header("setHeader", "Cache-Control", "public, max-age=60").is_ok());
        assert!(validate_header("setHeader", "X-Tab", "a\tb").is_ok());
        assert!(validate_header("setHeader", "X-Evil", "a\r\nSet-Cookie: x=1").is_err());
        assert!(validate_header("setHeader", "X-Evil", "a\nb").is_err());
        assert!(validate_header("setHeader", "X-Bad\r\nName", "v").is_err());
        assert!(validate_header("setHeader", "Bad Name", "v").is_err());
        assert!(validate_header("setHeader", "", "v").is_err());
        assert!(validate_header("setHeader", "Set-Cookie", "a=1").is_err());
        assert!(validate_header("setHeader", "content-length", "1").is_err());
        assert!(validate_header("setHeader", "X-Luat-Title", "t").is_err());
    }

    #[test]
    fn status_validation() {
        assert_eq!(validate_status(404), Ok(404));
        assert_eq!(validate_status(203), Ok(203));
        assert!(validate_status(301).is_err());
        assert!(validate_status(100).is_err());
        assert!(validate_status(600).is_err());
    }

    #[test]
    fn jar_set_header_replaces_and_append_keeps() {
        let jar = CookieJar::new();
        jar.set_header("Cache-Control", "no-store");
        jar.set_header("cache-control", "public, max-age=60");
        jar.append_header("Link", "</a.css>; rel=preload");
        jar.append_header("Link", "</b.css>; rel=preload");
        assert_eq!(
            jar.take_headers(),
            [
                ("cache-control".to_string(), "public, max-age=60".to_string()),
                ("Link".to_string(), "</a.css>; rel=preload".to_string()),
                ("Link".to_string(), "</b.css>; rel=preload".to_string()),
            ]
        );
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
