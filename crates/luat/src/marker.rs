// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Reactive markers for client-side resumability.
//!
//! Markers are encoded as HTML comments (`<!--l:BASE64-->`) that carry
//! reactive metadata for the WASM client runtime. The client reads these
//! markers to build its reactive registry without replaying server logic.
//!
//! This module defines the shared types used by both the server (encoding)
//! and the client runtime (decoding).

use serde::{Deserialize, Serialize};

/// The kind of reactive marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MarkerKind {
    /// A reactive text expression: `{count}`, `{name}`.
    Expression,
    /// An event listener: `on:click`, `on:submit`.
    Event,
    /// Reactive attribute bindings: `cx={x}`, `class={cls}`.
    Attribute,
    /// Start of a conditional block: `{#if condition}`.
    BlockStart,
    /// End of a conditional/iteration block: `{/if}`, `{/each}`.
    BlockEnd,
    /// Start of an iteration block: `{#each items as item}`.
    EachStart,
    /// A component boundary: `<MyComponent />`.
    Component,
    /// A derived variable definition: carries name, expression, and deps.
    DerivedDef,
    /// A template boundary: marks start of a template's rendered output.
    TemplateBoundary,
}

/// A typed value that can be serialized in markers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MarkerValue {
    /// Nil/null value.
    Nil,
    /// Boolean value.
    Bool(bool),
    /// Integer value.
    Int(i64),
    /// Floating-point value.
    Float(f64),
    /// String value.
    Str(String),
}

/// A reactive marker embedded in HTML output.
///
/// Each marker identifies a DOM location and its reactive bindings,
/// enabling the client runtime to resume reactivity without replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Unique ID within the page (used for DOM targeting).
    pub id: u16,
    /// What kind of reactive binding this represents.
    pub kind: MarkerKind,
    /// The Lua expression text (e.g., `"count + 1"`).
    pub expr: String,
    /// Reactive dependencies (variable names this expression reads).
    pub deps: Vec<String>,
    /// The initial value at render time.
    pub value: MarkerValue,
}

/// An event binding carried in an Event marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventBinding {
    /// The event type (e.g., "click", "submit", "input").
    pub event_type: String,
    /// The Lua handler expression.
    pub handler: String,
    /// Event modifiers (preventDefault, stopPropagation, etc.).
    pub modifiers: Vec<String>,
}

/// A reactive attribute binding carried in an Attribute marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrBinding {
    /// The attribute name (e.g., "cx", "style", "class").
    pub attr: String,
    /// The Lua expression for the attribute value.
    pub expr: String,
    /// Reactive dependencies.
    pub deps: Vec<String>,
    /// The initial value at render time.
    pub value: MarkerValue,
}

/// An attribute marker carrying multiple bindings for one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeMarker {
    /// Unique ID within the page.
    pub id: u16,
    /// The attribute bindings for the next sibling element.
    pub bindings: Vec<AttrBinding>,
}

/// An event marker carrying multiple event bindings for one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMarker {
    /// Unique ID within the page.
    pub id: u16,
    /// The event bindings for the next sibling element.
    pub bindings: Vec<EventBinding>,
}

/// The kind of a reactive variable declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactiveKind {
    /// `$state(initial)` — a mutable reactive value.
    State,
    /// `$derived(expr)` — a computed value from other reactive values.
    Derived,
}

/// A reactive variable extracted from a `<script>` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ReactiveVar {
    /// Variable name (e.g., "count").
    pub name: String,
    /// Whether this is $state or $derived.
    pub kind: ReactiveKind,
    /// The initial value expression (e.g., "0", "count * 2").
    pub initial_expr: String,
    /// For $derived: the dependency variable names.
    pub deps: Vec<String>,
}

/// Reactive metadata extracted from a script block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReactiveMetadata {
    /// All reactive variables declared in this component.
    pub vars: Vec<ReactiveVar>,
}

impl ReactiveMetadata {
    /// Returns true if any reactive variables are declared.
    pub fn is_reactive(&self) -> bool {
        !self.vars.is_empty()
    }

    /// Checks if a given variable name is reactive.
    pub fn is_var_reactive(&self, name: &str) -> bool {
        self.vars.iter().any(|v| v.name == name)
    }

    /// Gets the reactive var info for a variable name.
    pub fn get_var(&self, name: &str) -> Option<&ReactiveVar> {
        self.vars.iter().find(|v| v.name == name)
    }

    /// Returns the initial MarkerValue for a simple expression.
    ///
    /// If the expression is a single reactive variable name, returns its literal
    /// initial value. For complex expressions, returns Nil (client will evaluate).
    pub fn get_initial_value_for_expr(&self, expr: &str) -> MarkerValue {
        let trimmed = expr.trim();
        if let Some(var) = self.vars.iter().find(|v| v.name == trimmed) {
            parse_literal_to_marker_value(&var.initial_expr)
        } else {
            MarkerValue::Nil
        }
    }

    /// Returns all variable names that the given expression depends on.
    /// Simple heuristic: checks if any reactive var name appears in the expression.
    pub fn find_deps(&self, expr: &str) -> Vec<String> {
        self.vars
            .iter()
            .filter(|v| expr_references_var(expr, &v.name))
            .map(|v| v.name.clone())
            .collect()
    }
}

/// Checks if an expression text references a variable name.
/// Uses word-boundary-aware matching to avoid false positives
/// (e.g., "count" shouldn't match "counter").
pub fn expr_references_var(expr: &str, var_name: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = expr[search_from..].find(var_name) {
        let abs_pos = search_from + pos;
        let before_ok = abs_pos == 0
            || !expr.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                && expr.as_bytes()[abs_pos - 1] != b'_';
        let end_pos = abs_pos + var_name.len();
        let after_ok = end_pos >= expr.len()
            || !expr.as_bytes()[end_pos].is_ascii_alphanumeric()
                && expr.as_bytes()[end_pos] != b'_';
        if before_ok && after_ok {
            return true;
        }
        search_from = abs_pos + 1;
    }
    false
}

/// Parses a literal Lua expression string into a MarkerValue.
///
/// Handles integers, floats, booleans, nil, and quoted strings.
/// Returns `MarkerValue::Nil` for complex expressions that can't be
/// determined at compile time.
pub fn parse_literal_to_marker_value(expr: &str) -> MarkerValue {
    let trimmed = expr.trim();
    if let Ok(n) = trimmed.parse::<i64>() {
        return MarkerValue::Int(n);
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return MarkerValue::Float(f);
    }
    if trimmed == "true" {
        return MarkerValue::Bool(true);
    }
    if trimmed == "false" {
        return MarkerValue::Bool(false);
    }
    if trimmed == "nil" {
        return MarkerValue::Nil;
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return MarkerValue::Str(trimmed[1..trimmed.len() - 1].to_string());
    }
    MarkerValue::Nil
}

/// Marker prefix used in HTML comments.
pub const MARKER_PREFIX: &str = "l:";

/// End marker used in HTML comments to close expression boundaries.
/// Forces the browser to split text nodes at marker boundaries.
pub const END_MARKER: &str = "<!--/l-->";

/// Encodes a marker to a base64 string suitable for embedding in HTML comments.
///
/// The format is: `<!--l:BASE64_ENCODED_POSTCARD-->`
#[cfg(feature = "client-markers")]
pub fn encode_marker(marker: &Marker) -> String {
    use base64::Engine as _;
    let bytes = postcard::to_allocvec(marker).expect("marker serialization should not fail");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    format!("<!--{}{}-->", MARKER_PREFIX, encoded)
}

/// Encodes an event marker to a base64 string.
#[cfg(feature = "client-markers")]
pub fn encode_event_marker(marker: &EventMarker) -> String {
    use base64::Engine as _;
    let bytes = postcard::to_allocvec(marker).expect("event marker serialization should not fail");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    format!("<!--{}{}-->", MARKER_PREFIX, encoded)
}

/// Encodes an attribute marker to a base64 string.
#[cfg(feature = "client-markers")]
pub fn encode_attribute_marker(marker: &AttributeMarker) -> String {
    use base64::Engine as _;
    let bytes =
        postcard::to_allocvec(marker).expect("attribute marker serialization should not fail");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    format!("<!--{}{}-->", MARKER_PREFIX, encoded)
}

/// Decodes a marker from a base64 string (extracted from HTML comment content).
///
/// Input should be the content after `l:` prefix, without the `<!--` and `-->`.
#[cfg(feature = "client-markers")]
pub fn decode_marker(encoded: &str) -> Result<Marker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Decodes an event marker from a base64 string.
#[cfg(feature = "client-markers")]
pub fn decode_event_marker(encoded: &str) -> Result<EventMarker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Decodes an attribute marker from a base64 string.
#[cfg(feature = "client-markers")]
pub fn decode_attribute_marker(encoded: &str) -> Result<AttributeMarker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Errors that can occur during marker encoding/decoding.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkerError {
    /// Base64 decoding failed.
    Base64(String),
    /// Postcard deserialization failed.
    Postcard(String),
}

impl std::fmt::Display for MarkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkerError::Base64(e) => write!(f, "base64 decode error: {}", e),
            MarkerError::Postcard(e) => write!(f, "postcard decode error: {}", e),
        }
    }
}

impl std::error::Error for MarkerError {}

#[cfg(all(test, feature = "client-markers"))]
mod tests {
    use super::*;

    #[test]
    fn test_marker_roundtrip() {
        let marker = Marker {
            id: 1,
            kind: MarkerKind::Expression,
            expr: "count + 1".to_string(),
            deps: vec!["count".to_string()],
            value: MarkerValue::Int(0),
        };

        let encoded = encode_marker(&marker);
        assert!(encoded.starts_with("<!--l:"));
        assert!(encoded.ends_with("-->"));

        // Extract the base64 content
        let content = &encoded[6..encoded.len() - 3]; // Strip <!--l: and -->
        let decoded = decode_marker(content).unwrap();
        assert_eq!(decoded, marker);
    }

    #[test]
    fn test_event_marker_roundtrip() {
        let marker = EventMarker {
            id: 2,
            bindings: vec![EventBinding {
                event_type: "click".to_string(),
                handler: "count = count + 1".to_string(),
                modifiers: vec![],
            }],
        };

        let encoded = encode_event_marker(&marker);
        let content = &encoded[6..encoded.len() - 3];
        let decoded = decode_event_marker(content).unwrap();
        assert_eq!(decoded, marker);
    }

    #[test]
    fn test_attribute_marker_roundtrip() {
        let marker = AttributeMarker {
            id: 3,
            bindings: vec![AttrBinding {
                attr: "cx".to_string(),
                expr: "x".to_string(),
                deps: vec!["x".to_string()],
                value: MarkerValue::Int(50),
            }],
        };

        let encoded = encode_attribute_marker(&marker);
        let content = &encoded[6..encoded.len() - 3];
        let decoded = decode_attribute_marker(content).unwrap();
        assert_eq!(decoded, marker);
    }

    #[test]
    fn test_marker_size() {
        // Verify markers are compact
        let marker = Marker {
            id: 1,
            kind: MarkerKind::Expression,
            expr: "count".to_string(),
            deps: vec!["count".to_string()],
            value: MarkerValue::Int(0),
        };

        let encoded = encode_marker(&marker);
        // The base64 content (without HTML comment wrapper)
        let content_len = encoded.len() - 9; // minus <!--l: and -->
        // A simple expression marker should be well under 100 bytes
        assert!(
            content_len < 100,
            "Marker too large: {} bytes",
            content_len
        );
    }
}
