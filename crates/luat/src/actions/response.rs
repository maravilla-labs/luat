// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Action response types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response from an action handler.
///
/// Contains the HTTP status, response headers (for HTMX integration),
/// and the response data that can be used as props for template rendering.
///
/// # Example
///
/// ```rust
/// use luat::actions::ActionResponse;
///
/// // Success response
/// let response = ActionResponse::ok(serde_json::json!({ "success": true }));
///
/// // Redirect response for HTMX
/// let response = ActionResponse::htmx_redirect("/dashboard");
///
/// // Error response with custom headers
/// let response = ActionResponse::fail(400, serde_json::json!({
///     "error": "Validation failed",
///     "fields": { "email": "Invalid email format" }
/// }))
/// .with_header("HX-Retarget", "#error-container");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResponse {
    /// HTTP status code (200, 400, 302, etc.)
    pub status: u16,

    /// Response headers.
    /// Commonly used for HTMX headers like:
    /// - `HX-Redirect`: Redirect the page
    /// - `HX-Trigger`: Trigger client-side events
    /// - `HX-Retarget`: Change the target element
    /// - `HX-Reswap`: Change the swap method
    /// - `Set-Cookie`: Set session cookies
    pub headers: HashMap<String, String>,

    /// Response data as JSON.
    /// This is used as props when rendering an action template,
    /// or returned directly as JSON if no template exists.
    pub data: serde_json::Value,
}

impl ActionResponse {
    /// Creates a new ActionResponse with the given status and data.
    pub fn new(status: u16, data: serde_json::Value) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            data,
        }
    }

    /// Creates a successful response (200) with the given data.
    pub fn ok(data: serde_json::Value) -> Self {
        Self::new(200, data)
    }

    /// Creates an error/fail response with the given status and data.
    ///
    /// This is equivalent to SvelteKit's `fail()` function.
    pub fn fail(status: u16, data: serde_json::Value) -> Self {
        Self::new(status, data)
    }

    /// Creates a redirect response (302) with a Location header.
    pub fn redirect(url: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Location".to_string(), url.to_string());
        Self {
            status: 302,
            headers,
            data: serde_json::Value::Null,
        }
    }

    /// Creates an HTMX-compatible redirect response.
    ///
    /// Uses `HX-Redirect` header instead of `Location` so HTMX
    /// handles the redirect properly on the client side.
    pub fn htmx_redirect(url: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("HX-Redirect".to_string(), url.to_string());
        Self {
            status: 200,
            headers,
            data: serde_json::Value::Null,
        }
    }

    /// Adds a header to the response.
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    /// Adds multiple headers to the response.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Sets the response status.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Returns true if this is a successful response (2xx status).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Returns true if this is a redirect response (3xx status).
    pub fn is_redirect(&self) -> bool {
        (300..400).contains(&self.status)
    }

    /// Returns true if this is an error response (4xx or 5xx status).
    pub fn is_error(&self) -> bool {
        self.status >= 400
    }
}

impl Default for ActionResponse {
    fn default() -> Self {
        Self::ok(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ok_response() {
        let response = ActionResponse::ok(serde_json::json!({ "success": true }));
        assert_eq!(response.status, 200);
        assert!(response.is_success());
        assert!(!response.is_error());
    }

    #[test]
    fn test_fail_response() {
        let response = ActionResponse::fail(400, serde_json::json!({ "error": "Bad request" }));
        assert_eq!(response.status, 400);
        assert!(response.is_error());
        assert!(!response.is_success());
    }

    #[test]
    fn test_redirect() {
        let response = ActionResponse::redirect("/home");
        assert_eq!(response.status, 302);
        assert!(response.is_redirect());
        assert_eq!(response.headers.get("Location"), Some(&"/home".to_string()));
    }

    #[test]
    fn test_htmx_redirect() {
        let response = ActionResponse::htmx_redirect("/dashboard");
        assert_eq!(response.status, 200);
        assert_eq!(response.headers.get("HX-Redirect"), Some(&"/dashboard".to_string()));
    }

    #[test]
    fn test_with_headers() {
        let response = ActionResponse::ok(serde_json::json!({}))
            .with_header("HX-Trigger", "showMessage")
            .with_header("HX-Retarget", "#messages");

        assert_eq!(response.headers.get("HX-Trigger"), Some(&"showMessage".to_string()));
        assert_eq!(response.headers.get("HX-Retarget"), Some(&"#messages".to_string()));
    }
}
