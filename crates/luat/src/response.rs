// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP response abstraction for the Luat engine.
//!
//! This module provides a platform-agnostic response type that the engine
//! returns after handling a request. Adapters can convert this to their
//! platform-specific response format.

use std::collections::HashMap;
use serde_json::Value as JsonValue;

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
        headers: HashMap<String, String>,
        /// HTML body
        body: String,
    },

    /// JSON response (from API handlers)
    Json {
        /// HTTP status code
        status: u16,
        /// HTTP headers
        headers: HashMap<String, String>,
        /// JSON body
        body: JsonValue,
    },

    /// Redirect response
    Redirect {
        /// HTTP status code (301, 302, 303, 307, 308)
        status: u16,
        /// Redirect location
        location: String,
    },

    /// Error response
    Error {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },
}

impl LuatResponse {
    /// Creates an HTML response.
    pub fn html(status: u16, body: impl Into<String>) -> Self {
        Self::Html {
            status,
            headers: HashMap::new(),
            body: body.into(),
        }
    }

    /// Creates an HTML response with headers.
    pub fn html_with_headers(
        status: u16,
        body: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self::Html {
            status,
            headers,
            body: body.into(),
        }
    }

    /// Creates a JSON response.
    pub fn json(status: u16, body: JsonValue) -> Self {
        Self::Json {
            status,
            headers: HashMap::new(),
            body,
        }
    }

    /// Creates a JSON response with headers.
    pub fn json_with_headers(
        status: u16,
        body: JsonValue,
        headers: HashMap<String, String>,
    ) -> Self {
        Self::Json {
            status,
            headers,
            body,
        }
    }

    /// Creates a redirect response (HTTP 302 by default).
    pub fn redirect(location: impl Into<String>) -> Self {
        Self::Redirect {
            status: 302,
            location: location.into(),
        }
    }

    /// Creates a redirect response with a specific status code.
    pub fn redirect_with_status(status: u16, location: impl Into<String>) -> Self {
        Self::Redirect {
            status,
            location: location.into(),
        }
    }

    /// Creates an error response.
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self::Error {
            status,
            message: message.into(),
        }
    }

    /// Creates a 404 Not Found response.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::error(404, message)
    }

    /// Creates a 500 Internal Server Error response.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::error(500, message)
    }

    /// Creates a 400 Bad Request response.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::error(400, message)
    }

    /// Returns the status code.
    pub fn status(&self) -> u16 {
        match self {
            Self::Html { status, .. } => *status,
            Self::Json { status, .. } => *status,
            Self::Redirect { status, .. } => *status,
            Self::Error { status, .. } => *status,
        }
    }

    /// Returns true if this is a success response (2xx).
    pub fn is_success(&self) -> bool {
        let status = self.status();
        (200..300).contains(&status)
    }

    /// Returns true if this is an error response (4xx or 5xx).
    pub fn is_error(&self) -> bool {
        self.status() >= 400
    }

    /// Returns true if this is a redirect response (3xx).
    pub fn is_redirect(&self) -> bool {
        let status = self.status();
        (300..400).contains(&status)
    }

    /// Adds a header to the response (only for Html and Json variants).
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self {
            Self::Html { headers, .. } | Self::Json { headers, .. } => {
                headers.insert(key.into(), value.into());
            }
            _ => {}
        }
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
            assert_eq!(headers.get("X-Custom"), Some(&"value".to_string()));
        } else {
            panic!("Expected Html variant");
        }
    }
}
