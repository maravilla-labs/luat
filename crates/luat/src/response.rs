// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP response abstraction for the Luat engine.
//!
//! This module provides a platform-agnostic response type that the engine
//! returns after handling a request. Adapters can convert this to their
//! platform-specific response format.

use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// HTTP headers: ordered, case-insensitive names, repeats allowed (needed
/// for `Set-Cookie`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(Vec<(String, String)>);

impl Headers {
    /// Creates an empty header list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first value for `name`, ignoring case.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Returns every value for `name`, ignoring case.
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Returns true if a header named `name` is present.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Sets `name` to `value`, replacing any existing values.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
        self.0.push((name, value.into()));
    }

    /// Adds a value for `name`, keeping existing ones.
    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }

    /// Removes every value for `name` and returns the first one.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        let first = self.get(name).map(str::to_string);
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        first
    }

    /// Iterates over `(name, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of header values.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if there are no headers.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<HashMap<String, String>> for Headers {
    fn from(map: HashMap<String, String>) -> Self {
        Self(map.into_iter().collect())
    }
}

impl FromIterator<(String, String)> for Headers {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Headers {
    type Item = (String, String);
    type IntoIter = std::vec::IntoIter<(String, String)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Extend<(String, String)> for Headers {
    fn extend<I: IntoIterator<Item = (String, String)>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

/// A platform-agnostic HTTP response from the Luat engine.
///
/// The engine returns one of these variants after handling a request.
/// Adapters convert this to their platform-specific response format.
///
/// # Example
///
/// ```rust
/// use luat::LuatResponse;
///
/// // HTML response
/// let html = LuatResponse::html(200, "<h1>Hello</h1>");
///
/// // JSON response
/// let json = LuatResponse::json(200, serde_json::json!({"success": true}));
///
/// // Redirect
/// let redirect = LuatResponse::redirect("/login");
/// ```
#[derive(Debug, Clone)]
pub enum LuatResponse {
    /// HTML response (from template rendering)
    Html {
        /// HTTP status code
        status: u16,
        /// HTTP headers
        headers: Headers,
        /// HTML body
        body: String,
    },

    /// JSON response (from API handlers)
    Json {
        /// HTTP status code
        status: u16,
        /// HTTP headers
        headers: Headers,
        /// JSON body
        body: JsonValue,
    },

    /// Raw body (text, binary or pre-rendered markup from API handlers).
    /// The `content-type` header says what it is.
    Body {
        /// HTTP status code
        status: u16,
        /// HTTP headers
        headers: Headers,
        /// Response body bytes
        body: Vec<u8>,
    },

    /// Redirect response
    Redirect {
        /// HTTP status code (301, 302, 303, 307, 308)
        status: u16,
        /// Redirect location
        location: String,
        /// HTTP headers (e.g. `Set-Cookie`)
        headers: Headers,
    },

    /// Error response. `message` is safe to show to clients.
    Error {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
        /// HTTP headers
        headers: Headers,
    },
}

impl LuatResponse {
    /// Creates an HTML response.
    pub fn html(status: u16, body: impl Into<String>) -> Self {
        Self::html_with_headers(status, body, Headers::new())
    }

    /// Creates an HTML response with headers.
    pub fn html_with_headers(status: u16, body: impl Into<String>, headers: impl Into<Headers>) -> Self {
        Self::Html {
            status,
            headers: headers.into(),
            body: body.into(),
        }
    }

    /// Creates a JSON response.
    pub fn json(status: u16, body: JsonValue) -> Self {
        Self::json_with_headers(status, body, Headers::new())
    }

    /// Creates a JSON response with headers.
    pub fn json_with_headers(status: u16, body: JsonValue, headers: impl Into<Headers>) -> Self {
        Self::Json {
            status,
            headers: headers.into(),
            body,
        }
    }

    /// Creates a raw-body response. Adds `content-type: text/plain` when the
    /// headers don't name one.
    pub fn body(status: u16, body: impl Into<Vec<u8>>, headers: impl Into<Headers>) -> Self {
        let mut headers = headers.into();
        if !headers.contains("content-type") {
            headers.insert("content-type", "text/plain; charset=utf-8");
        }
        Self::Body {
            status,
            headers,
            body: body.into(),
        }
    }

    /// Creates a 302 redirect.
    pub fn redirect(location: impl Into<String>) -> Self {
        Self::redirect_with_status(302, location)
    }

    /// Creates a redirect with the given status.
    pub fn redirect_with_status(status: u16, location: impl Into<String>) -> Self {
        Self::Redirect {
            status,
            location: location.into(),
            headers: Headers::new(),
        }
    }

    /// Creates an error response. `message` is shown to clients.
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self::Error {
            status,
            message: message.into(),
            headers: Headers::new(),
        }
    }

    /// Creates a 404 error response.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::error(404, message)
    }

    /// Creates a 500 error response.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::error(500, message)
    }

    /// Creates a 400 error response.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::error(400, message)
    }

    /// Returns the HTTP status code.
    pub fn status(&self) -> u16 {
        match self {
            Self::Html { status, .. }
            | Self::Json { status, .. }
            | Self::Body { status, .. }
            | Self::Redirect { status, .. }
            | Self::Error { status, .. } => *status,
        }
    }

    /// Returns the response headers.
    pub fn headers(&self) -> &Headers {
        match self {
            Self::Html { headers, .. }
            | Self::Json { headers, .. }
            | Self::Body { headers, .. }
            | Self::Redirect { headers, .. }
            | Self::Error { headers, .. } => headers,
        }
    }

    /// Returns the response headers mutably.
    pub fn headers_mut(&mut self) -> &mut Headers {
        match self {
            Self::Html { headers, .. }
            | Self::Json { headers, .. }
            | Self::Body { headers, .. }
            | Self::Redirect { headers, .. }
            | Self::Error { headers, .. } => headers,
        }
    }

    /// Returns true for 2xx responses.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status())
    }

    /// Returns true for 4xx and 5xx responses.
    pub fn is_error(&self) -> bool {
        self.status() >= 400
    }

    /// Returns true for 3xx responses.
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status())
    }

    /// Sets a header, replacing existing values of the same name.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers_mut().insert(key, value);
        self
    }
}

impl Default for LuatResponse {
    fn default() -> Self {
        Self::html(200, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_keep_repeats_and_ignore_case() {
        let mut h = Headers::new();
        h.append("Set-Cookie", "a=1");
        h.append("set-cookie", "b=2");
        h.insert("Content-Type", "text/html");
        h.insert("content-type", "application/json");
        assert_eq!(h.get_all("SET-COOKIE").collect::<Vec<_>>(), ["a=1", "b=2"]);
        assert_eq!(h.get("Content-Type"), Some("application/json"));
        assert_eq!(h.remove("set-cookie").as_deref(), Some("a=1"));
        assert!(!h.contains("set-cookie"));
    }

    #[test]
    fn body_defaults_to_plain_text() {
        let resp = LuatResponse::body(200, "hi", Headers::new());
        assert_eq!(resp.headers().get("content-type"), Some("text/plain; charset=utf-8"));
    }

    #[test]
    fn test_html_response() {
        let resp = LuatResponse::html(200, "<h1>Hello</h1>");
        assert_eq!(resp.status(), 200);
        assert!(resp.is_success());

        if let LuatResponse::Html { body, .. } = resp {
            assert_eq!(body, "<h1>Hello</h1>");
        } else {
            panic!("Expected Html variant");
        }
    }

    #[test]
    fn test_json_response() {
        let resp = LuatResponse::json(200, serde_json::json!({"success": true}));
        assert_eq!(resp.status(), 200);

        if let LuatResponse::Json { body, .. } = resp {
            assert_eq!(body["success"], true);
        } else {
            panic!("Expected Json variant");
        }
    }

    #[test]
    fn test_redirect() {
        let resp = LuatResponse::redirect("/login");
        assert_eq!(resp.status(), 302);
        assert!(resp.is_redirect());

        if let LuatResponse::Redirect { location, .. } = resp {
            assert_eq!(location, "/login");
        } else {
            panic!("Expected Redirect variant");
        }
    }

    #[test]
    fn test_error() {
        let resp = LuatResponse::not_found("Page not found");
        assert_eq!(resp.status(), 404);
        assert!(resp.is_error());
    }

    #[test]
    fn test_with_header() {
        let resp = LuatResponse::html(200, "test")
            .with_header("X-Custom", "value");

        if let LuatResponse::Html { headers, .. } = resp {
            assert_eq!(headers.get("x-custom"), Some("value"));
        } else {
            panic!("Expected Html variant");
        }
    }
}
