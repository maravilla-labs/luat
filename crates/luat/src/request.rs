// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP request abstraction for the Luat engine.
//!
//! This module provides a platform-agnostic request type that can be used
//! by different adapters (HTTP servers, WASM, etc.) to pass request data
//! to the engine.

use std::collections::HashMap;

/// A platform-agnostic HTTP request.
///
/// This struct contains all the information needed by the engine to
/// handle a request: routing, executing load functions, API handlers,
/// and rendering templates.
///
/// # Example
///
/// ```rust
/// use luat::LuatRequest;
///
/// let request = LuatRequest::new("/blog/hello", "GET")
///     .with_query([("page".into(), "1".into())].into());
/// ```
#[derive(Debug, Clone)]
pub struct LuatRequest {
    /// The request path (e.g., "/blog/hello")
    pub path: String,

    /// The HTTP method (e.g., "GET", "POST")
    pub method: String,

    /// HTTP headers
    pub headers: HashMap<String, String>,

    /// Request body (for POST/PUT/PATCH)
    pub body: Option<Vec<u8>>,

    /// Query parameters (parsed from URL)
    pub query: HashMap<String, String>,

    /// The raw query string as the client sent it, without the leading
    /// `?` (e.g. `"page=2&sort=new"`). When unset, [`search`](Self::search)
    /// rebuilds one from [`query`](Self::query).
    pub raw_query: Option<String>,

    /// Cookies
    pub cookies: HashMap<String, String>,
}

impl LuatRequest {
    /// Creates a new request with the given path and method.
    pub fn new(path: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            method: method.into(),
            headers: HashMap::new(),
            body: None,
            query: HashMap::new(),
            raw_query: None,
            cookies: HashMap::new(),
        }
    }

    /// Adds headers to the request.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Adds a body to the request.
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Adds query parameters to the request.
    pub fn with_query(mut self, query: HashMap<String, String>) -> Self {
        self.query = query;
        self
    }

    /// Sets the raw query string (without the leading `?`), exposed to
    /// handlers as `ctx.search`. A leading `?` is stripped.
    pub fn with_raw_query(mut self, raw_query: impl Into<String>) -> Self {
        let raw: String = raw_query.into();
        self.raw_query = Some(raw.strip_prefix('?').map(str::to_string).unwrap_or(raw));
        self
    }

    /// The query string including its leading `?`, or `""` when there is
    /// none (like `URL.search`). Uses [`raw_query`](Self::raw_query) when
    /// set; otherwise it is rebuilt from [`query`](Self::query), sorted by
    /// key.
    pub fn search(&self) -> String {
        let query = match &self.raw_query {
            Some(raw) => raw.clone(),
            None => {
                let mut pairs: Vec<_> = self.query.iter().collect();
                pairs.sort();
                let mut serializer = form_urlencoded::Serializer::new(String::new());
                for (key, value) in pairs {
                    serializer.append_pair(key, value);
                }
                serializer.finish()
            }
        };
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    }

    /// The path followed by the query string (`/search?q=lua`): the request
    /// target, relative to the site's origin.
    pub fn href(&self) -> String {
        format!("{}{}", self.path, self.search())
    }

    /// Adds cookies to the request.
    pub fn with_cookies(mut self, cookies: HashMap<String, String>) -> Self {
        self.cookies = cookies;
        self
    }

    /// Returns the body as a string, if present and valid UTF-8.
    pub fn body_str(&self) -> Option<&str> {
        self.body.as_ref().and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Returns the body parsed as JSON, if present and valid.
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body_str()
            .and_then(|s| serde_json::from_str(s).ok())
    }

    /// Returns the value of header `name`, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Returns the request cookies: those set explicitly with
    /// [`with_cookies`](Self::with_cookies), or else parsed from the
    /// `Cookie` header.
    pub fn cookie_map(&self) -> HashMap<String, String> {
        if !self.cookies.is_empty() {
            return self.cookies.clone();
        }
        self.header("cookie")
            .map(crate::ctx_helpers::parse_cookie_header)
            .unwrap_or_default()
    }

    /// Returns the Content-Type header, if present.
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(|s| s.as_str())
            .or_else(|| self.headers.get("Content-Type").map(|s| s.as_str()))
    }

    /// Checks if this is a form submission (POST with form content type).
    pub fn is_form_submission(&self) -> bool {
        self.method.eq_ignore_ascii_case("POST")
            && self.content_type()
                .map(|ct| ct.starts_with("application/x-www-form-urlencoded")
                      || ct.starts_with("multipart/form-data"))
                .unwrap_or(false)
    }

    /// Checks if this is a JSON request.
    pub fn is_json(&self) -> bool {
        self.content_type()
            .map(|ct| ct.starts_with("application/json"))
            .unwrap_or(false)
    }

    /// Extracts the action name from query string (e.g., "?/login" -> "login").
    pub fn action_name(&self) -> Option<&str> {
        // Look for ?/actionName pattern in query
        self.query.keys()
            .find(|k| k.starts_with('/'))
            .map(|k| &k[1..])
    }
}

impl Default for LuatRequest {
    fn default() -> Self {
        Self::new("/", "GET")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_request() {
        let req = LuatRequest::new("/blog/hello", "GET");
        assert_eq!(req.path, "/blog/hello");
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn test_with_query() {
        let req = LuatRequest::new("/search", "GET")
            .with_query([("q".into(), "rust".into())].into());
        assert_eq!(req.query.get("q"), Some(&"rust".to_string()));
    }

    #[test]
    fn search_and_href_use_the_raw_query() {
        let req = LuatRequest::new("/search", "GET").with_raw_query("q=a%20b&q=c");
        assert_eq!(req.search(), "?q=a%20b&q=c");
        assert_eq!(req.href(), "/search?q=a%20b&q=c");
        assert_eq!(LuatRequest::new("/", "GET").with_raw_query("?x=1").search(), "?x=1");
        assert_eq!(LuatRequest::new("/", "GET").with_raw_query("").search(), "");
    }

    #[test]
    fn search_is_rebuilt_from_the_query_map() {
        let req = LuatRequest::new("/s", "GET")
            .with_query([("b".into(), "x y".into()), ("a".into(), "1".into())].into());
        assert_eq!(req.search(), "?a=1&b=x+y");
        assert_eq!(LuatRequest::new("/s", "GET").search(), "");
    }

    #[test]
    fn test_body_str() {
        let req = LuatRequest::new("/api", "POST")
            .with_body(b"hello world".to_vec());
        assert_eq!(req.body_str(), Some("hello world"));
    }

    #[test]
    fn test_body_json() {
        let req = LuatRequest::new("/api", "POST")
            .with_body(br#"{"name": "test"}"#.to_vec());
        let json = req.body_json().unwrap();
        assert_eq!(json["name"], "test");
    }

    #[test]
    fn test_action_name() {
        let req = LuatRequest::new("/auth", "POST")
            .with_query([("/login".into(), "".into())].into());
        assert_eq!(req.action_name(), Some("login"));
    }

    #[test]
    fn test_is_form_submission() {
        let req = LuatRequest::new("/form", "POST")
            .with_headers([("content-type".into(), "application/x-www-form-urlencoded".into())].into());
        assert!(req.is_form_submission());
    }
}
