// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Platform-agnostic action context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Platform-agnostic context passed to action handlers.
///
/// This struct contains all the information needed to process a form action,
/// and can be constructed from various sources (HTTP requests, WASM calls, etc.).
///
/// # Example
///
/// ```rust
/// use luat::actions::ActionContext;
/// use std::collections::HashMap;
///
/// let ctx = ActionContext::new("POST", "/blog/hello/edit")
///     .with_params([("slug".to_string(), "hello".to_string())].into())
///     .with_body(serde_json::json!({ "title": "Updated Title" }))
///     .with_action(Some("publish".to_string()));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionContext {
    /// HTTP method (POST, PUT, DELETE, PATCH, etc.)
    pub method: String,

    /// Full request URL (e.g., `/blog/hello/edit?/publish`)
    pub url: String,

    /// URL path parameters extracted from dynamic route segments.
    /// For route `/blog/[slug]/edit`, accessing `/blog/hello/edit`
    /// produces `{ "slug": "hello" }`.
    pub params: HashMap<String, String>,

    /// Query parameters from the URL (excluding the action name).
    /// For `?/login&redirect=/home`, this contains `{ "redirect": "/home" }`.
    pub query: HashMap<String, String>,

    /// Request headers as key-value pairs.
    pub headers: HashMap<String, String>,

    /// Request body as JSON value.
    /// This can be form data (parsed from `application/x-www-form-urlencoded`)
    /// or JSON body (parsed from `application/json`).
    pub body: serde_json::Value,

    /// The named action being invoked, if any.
    /// - `None` for default action (POST without `?/actionName`)
    /// - `Some("login")` for named action (POST `?/login`)
    pub action_name: Option<String>,

    /// Request cookies as key-value pairs.
    pub cookies: HashMap<String, String>,
}

impl ActionContext {
    /// Creates a new ActionContext with the given method and URL.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method (POST, PUT, DELETE, etc.)
    /// * `url` - Full request URL
    pub fn new(method: &str, url: &str) -> Self {
        Self {
            method: method.to_uppercase(),
            url: url.to_string(),
            params: HashMap::new(),
            query: HashMap::new(),
            headers: HashMap::new(),
            body: serde_json::Value::Null,
            action_name: None,
            cookies: HashMap::new(),
        }
    }

    /// Sets URL path parameters.
    pub fn with_params(mut self, params: HashMap<String, String>) -> Self {
        self.params = params;
        self
    }

    /// Sets query parameters.
    pub fn with_query(mut self, query: HashMap<String, String>) -> Self {
        self.query = query;
        self
    }

    /// Sets request headers.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Sets the request body.
    pub fn with_body(mut self, body: serde_json::Value) -> Self {
        self.body = body;
        self
    }

    /// Sets the action name.
    pub fn with_action(mut self, action_name: Option<String>) -> Self {
        self.action_name = action_name;
        self
    }

    /// Sets cookies.
    pub fn with_cookies(mut self, cookies: HashMap<String, String>) -> Self {
        self.cookies = cookies;
        self
    }

    /// Returns the effective action name for handler lookup.
    /// Returns "default" if no named action is specified.
    pub fn effective_action_name(&self) -> &str {
        self.action_name.as_deref().unwrap_or("default")
    }
}

impl Default for ActionContext {
    fn default() -> Self {
        Self::new("POST", "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context() {
        let ctx = ActionContext::new("POST", "/blog/hello/edit");
        assert_eq!(ctx.method, "POST");
        assert_eq!(ctx.url, "/blog/hello/edit");
        assert!(ctx.action_name.is_none());
    }

    #[test]
    fn test_method_case_normalization() {
        let ctx = ActionContext::new("post", "/test");
        assert_eq!(ctx.method, "POST");
    }

    #[test]
    fn test_with_action() {
        let ctx = ActionContext::new("POST", "/test")
            .with_action(Some("login".to_string()));
        assert_eq!(ctx.action_name, Some("login".to_string()));
        assert_eq!(ctx.effective_action_name(), "login");
    }

    #[test]
    fn test_effective_action_name_default() {
        let ctx = ActionContext::new("POST", "/test");
        assert_eq!(ctx.effective_action_name(), "default");
    }

    #[test]
    fn test_builder_pattern() {
        let ctx = ActionContext::new("PUT", "/api/users/123")
            .with_params([("id".to_string(), "123".to_string())].into())
            .with_body(serde_json::json!({ "name": "John" }))
            .with_headers([("Content-Type".to_string(), "application/json".to_string())].into());

        assert_eq!(ctx.params.get("id"), Some(&"123".to_string()));
        assert_eq!(ctx.body["name"], "John");
        assert_eq!(ctx.headers.get("Content-Type"), Some(&"application/json".to_string()));
    }
}
