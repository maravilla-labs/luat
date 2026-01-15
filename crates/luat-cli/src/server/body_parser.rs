// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Request body parsing for form data and JSON.

use axum::body::Body;
use axum::http::{HeaderMap, Request};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Maximum body size to accept (1MB)
const MAX_BODY_SIZE: usize = 1024 * 1024;

/// Parses the request body based on Content-Type header.
///
/// Supports:
/// - `application/x-www-form-urlencoded` - URL-encoded form data
/// - `application/json` - JSON body
/// - `multipart/form-data` - Multipart form data (basic support)
pub async fn parse_request_body(
    request: Request<Body>,
) -> Result<(JsonValue, HeaderMap), BodyParseError> {
    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    // Get content type
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Read body bytes
    let body_bytes = axum::body::to_bytes(body, MAX_BODY_SIZE)
        .await
        .map_err(|_| BodyParseError::TooLarge)?;

    // Parse based on content type
    let body_value = if content_type.contains("application/json") {
        parse_json(&body_bytes)?
    } else if content_type.contains("application/x-www-form-urlencoded") {
        parse_form_urlencoded(&body_bytes)
    } else if content_type.contains("multipart/form-data") {
        // Basic multipart support - extract boundary and parse
        parse_multipart_basic(&body_bytes, content_type)?
    } else if body_bytes.is_empty() {
        JsonValue::Null
    } else {
        // Try to parse as JSON, fall back to null
        parse_json(&body_bytes).unwrap_or(JsonValue::Null)
    };

    Ok((body_value, headers))
}

/// Parses JSON body.
fn parse_json(bytes: &[u8]) -> Result<JsonValue, BodyParseError> {
    serde_json::from_slice(bytes).map_err(|e| BodyParseError::InvalidJson(e.to_string()))
}

/// Parses URL-encoded form data.
fn parse_form_urlencoded(bytes: &[u8]) -> JsonValue {
    let form: HashMap<String, String> = form_urlencoded::parse(bytes)
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    serde_json::to_value(form).unwrap_or(JsonValue::Null)
}

/// Basic multipart form data parsing.
/// This extracts text fields only; file uploads are not fully supported.
fn parse_multipart_basic(bytes: &[u8], content_type: &str) -> Result<JsonValue, BodyParseError> {
    // Extract boundary from content type
    let boundary = content_type
        .split(';')
        .find(|s| s.trim().starts_with("boundary="))
        .and_then(|s| s.trim().strip_prefix("boundary="))
        .ok_or_else(|| BodyParseError::InvalidMultipart("Missing boundary".to_string()))?;

    let boundary = boundary.trim_matches('"');
    let delimiter = format!("--{}", boundary);

    let body_str = String::from_utf8_lossy(bytes);
    let mut form_data = HashMap::new();

    // Split by boundary and parse each part
    for part in body_str.split(&delimiter) {
        if part.trim().is_empty() || part.starts_with("--") {
            continue;
        }

        // Parse headers and content
        if let Some(idx) = part.find("\r\n\r\n") {
            let headers_str = &part[..idx];
            let content = part[idx + 4..].trim_end_matches("\r\n");

            // Extract field name from Content-Disposition header
            if let Some(name) = extract_form_field_name(headers_str) {
                // Only include text fields (skip files for now)
                if !headers_str.contains("filename=") {
                    form_data.insert(name.to_string(), content.to_string());
                }
            }
        }
    }

    Ok(serde_json::to_value(form_data).unwrap_or(JsonValue::Null))
}

/// Extracts the field name from Content-Disposition header.
fn extract_form_field_name(headers: &str) -> Option<&str> {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-disposition:") {
            // Look for name="fieldname"
            if let Some(name_part) = line.split(';').find(|s| s.trim().starts_with("name=")) {
                let name = name_part.trim().strip_prefix("name=")?;
                return Some(name.trim_matches('"'));
            }
        }
    }
    None
}

/// Parses action name from query string.
///
/// The action name is specified as a query parameter starting with `/`:
/// - `?/login` -> Some("login")
/// - `?/publish&confirmed=true` -> Some("publish")
/// - `?foo=bar` -> None
pub fn parse_action_name(query: Option<&str>) -> Option<String> {
    query.and_then(|q| {
        q.split('&')
            .find(|part| part.starts_with('/'))
            .map(|part| {
                // Remove the leading `/` and any value after `=`
                let name = part.trim_start_matches('/');
                name.split('=').next().unwrap_or(name).to_string()
            })
    })
}

/// Parses query parameters, excluding the action name.
pub fn parse_query_params(query: Option<&str>) -> HashMap<String, String> {
    query
        .map(|q| {
            q.split('&')
                .filter(|part| !part.starts_with('/')) // Exclude action name
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    let key = parts.next()?.to_string();
                    let value = parts.next().unwrap_or("").to_string();
                    if key.is_empty() {
                        None
                    } else {
                        Some((key, value))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Error types for body parsing.
#[derive(Debug, thiserror::Error)]
pub enum BodyParseError {
    /// Request body exceeds the maximum allowed size.
    #[error("Request body too large (max {MAX_BODY_SIZE} bytes)")]
    TooLarge,

    /// JSON parsing failed.
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),

    /// Multipart form data parsing failed.
    #[error("Invalid multipart data: {0}")]
    InvalidMultipart(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_action_name() {
        assert_eq!(parse_action_name(Some("/login")), Some("login".to_string()));
        assert_eq!(
            parse_action_name(Some("/publish&confirmed=true")),
            Some("publish".to_string())
        );
        assert_eq!(parse_action_name(Some("foo=bar")), None);
        assert_eq!(parse_action_name(None), None);
        assert_eq!(
            parse_action_name(Some("foo=bar&/delete")),
            Some("delete".to_string())
        );
    }

    #[test]
    fn test_parse_query_params() {
        let params = parse_query_params(Some("foo=bar&/login&baz=qux"));
        assert_eq!(params.get("foo"), Some(&"bar".to_string()));
        assert_eq!(params.get("baz"), Some(&"qux".to_string()));
        assert!(!params.contains_key("login"));
    }

    #[test]
    fn test_parse_form_urlencoded() {
        let bytes = b"name=John&email=john%40example.com";
        let result = parse_form_urlencoded(bytes);
        assert_eq!(result["name"], "John");
        assert_eq!(result["email"], "john@example.com");
    }

    #[test]
    fn test_parse_json() {
        let bytes = br#"{"name": "John", "age": 30}"#;
        let result = parse_json(bytes).unwrap();
        assert_eq!(result["name"], "John");
        assert_eq!(result["age"], 30);
    }
}
