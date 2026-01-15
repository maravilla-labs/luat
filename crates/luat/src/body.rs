// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Request body parsing helpers shared by engine and runtime.

use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Errors that can occur while parsing a request body.
#[derive(Debug, thiserror::Error)]
pub enum BodyParseError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
    #[error("Invalid multipart data: {0}")]
    InvalidMultipart(String),
}

/// Parses a request body for form actions (strict for known content types).
pub fn parse_action_body(body: &[u8], content_type: Option<&str>) -> Result<JsonValue, BodyParseError> {
    let content_type = content_type.unwrap_or("");

    if content_type.contains("application/json") {
        return parse_json(body);
    }

    if content_type.contains("application/x-www-form-urlencoded") {
        return Ok(parse_form_urlencoded(body));
    }

    if content_type.contains("multipart/form-data") {
        return parse_multipart_basic(body, content_type);
    }

    if body.is_empty() {
        return Ok(JsonValue::Null);
    }

    parse_json(body).or(Ok(JsonValue::Null))
}

/// Parses structured body content (lenient). Returns None for unrecognized content.
pub fn parse_structured_body(body: &[u8], content_type: Option<&str>) -> Option<JsonValue> {
    let content_type = content_type.unwrap_or("");

    if content_type.contains("application/json") {
        return parse_json(body).ok();
    }

    if content_type.contains("application/x-www-form-urlencoded") {
        return Some(parse_form_urlencoded(body));
    }

    if content_type.contains("multipart/form-data") {
        return parse_multipart_basic(body, content_type).ok();
    }

    parse_json(body).ok()
}

fn parse_json(bytes: &[u8]) -> Result<JsonValue, BodyParseError> {
    serde_json::from_slice(bytes).map_err(|e| BodyParseError::InvalidJson(e.to_string()))
}

fn parse_form_urlencoded(bytes: &[u8]) -> JsonValue {
    let form: HashMap<String, String> = form_urlencoded::parse(bytes)
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    serde_json::to_value(form).unwrap_or(JsonValue::Null)
}

fn parse_multipart_basic(bytes: &[u8], content_type: &str) -> Result<JsonValue, BodyParseError> {
    let boundary = content_type
        .split(';')
        .find(|s| s.trim().starts_with("boundary="))
        .and_then(|s| s.trim().strip_prefix("boundary="))
        .ok_or_else(|| BodyParseError::InvalidMultipart("Missing boundary".to_string()))?;

    let boundary = boundary.trim_matches('"');
    let delimiter = format!("--{}", boundary);

    let body_str = String::from_utf8_lossy(bytes);
    let mut form_data = HashMap::new();

    for part in body_str.split(&delimiter) {
        if part.trim().is_empty() || part.starts_with("--") {
            continue;
        }

        if let Some(idx) = part.find("\r\n\r\n") {
            let headers_str = &part[..idx];
            let content = part[idx + 4..].trim_end_matches("\r\n");

            if let Some(name) = extract_form_field_name(headers_str) {
                if !headers_str.contains("filename=") {
                    form_data.insert(name.to_string(), content.to_string());
                }
            }
        }
    }

    Ok(serde_json::to_value(form_data).unwrap_or(JsonValue::Null))
}

fn extract_form_field_name(headers: &str) -> Option<&str> {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-disposition:") {
            if let Some(name_part) = line.split(';').find(|s| s.trim().starts_with("name=")) {
                let name = name_part.trim().strip_prefix("name=")?;
                return Some(name.trim_matches('"'));
            }
        }
    }
    None
}
