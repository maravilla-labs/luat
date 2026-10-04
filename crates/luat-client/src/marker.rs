// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Marker types for client-side decoding.
//!
//! These types mirror the server-side marker types used for encoding.
//! The client decodes markers from HTML comments to build reactive bindings.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Prefix used in HTML comments: `<!--l:BASE64-->`
pub const MARKER_PREFIX: &str = "l:";

/// A reactive expression marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Unique ID within the page.
    pub id: u16,
    /// The kind of marker.
    pub kind: MarkerKind,
    /// The Lua expression to re-evaluate.
    pub expr: String,
    /// Reactive variable names this expression depends on.
    pub deps: Vec<String>,
    /// The initial value at server render time.
    pub value: MarkerValue,
}

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

/// A typed value carried in a marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MarkerValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
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

/// A reactive attribute binding.
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

/// An event marker carrying multiple event bindings for one element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMarker {
    /// Unique ID within the page.
    pub id: u16,
    /// The event bindings for the next sibling element.
    pub bindings: Vec<EventBinding>,
}

/// An attribute marker carrying reactive attribute bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeMarker {
    /// Unique ID within the page.
    pub id: u16,
    /// The attribute bindings for the next sibling element.
    pub bindings: Vec<AttrBinding>,
}

/// Errors that can occur during marker decoding.
#[derive(Debug)]
pub enum MarkerError {
    Base64(String),
    Postcard(String),
}

/// Decodes an expression marker from a base64 string.
pub fn decode_marker(encoded: &str) -> Result<Marker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Decodes an event marker from a base64 string.
pub fn decode_event_marker(encoded: &str) -> Result<EventMarker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Decodes an attribute marker from a base64 string.
pub fn decode_attribute_marker(encoded: &str) -> Result<AttributeMarker, MarkerError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| MarkerError::Base64(e.to_string()))?;
    postcard::from_bytes(&bytes).map_err(|e| MarkerError::Postcard(e.to_string()))
}

/// Attempts to decode a marker comment payload, trying each type.
pub enum DecodedMarker {
    Expression(Marker),
    Event(EventMarker),
    Attribute(AttributeMarker),
    Component(Marker),
    DerivedDef(Marker),
    TemplateBoundary(Marker),
}

/// Try to decode a base64 payload as any known marker type.
/// Returns the first successful decode.
pub fn try_decode(payload: &str) -> Option<DecodedMarker> {
    if let Ok(m) = decode_marker(payload) {
        return match m.kind {
            MarkerKind::Component => Some(DecodedMarker::Component(m)),
            MarkerKind::DerivedDef => Some(DecodedMarker::DerivedDef(m)),
            MarkerKind::TemplateBoundary => Some(DecodedMarker::TemplateBoundary(m)),
            _ => Some(DecodedMarker::Expression(m)),
        };
    }
    if let Ok(m) = decode_event_marker(payload) {
        return Some(DecodedMarker::Event(m));
    }
    if let Ok(m) = decode_attribute_marker(payload) {
        return Some(DecodedMarker::Attribute(m));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode the exact payloads generated by the server for the counter test.
    /// These base64 strings are the output of the server's encode functions.
    #[test]
    fn test_decode_server_generated_event_marker() {
        let payload = "AAEFY2xpY2sRY291bnQgPSBjb3VudCArIDEA";
        let decoded = try_decode(payload);

        match decoded {
            Some(DecodedMarker::Event(em)) => {
                assert_eq!(em.id, 0);
                assert_eq!(em.bindings.len(), 1);
                assert_eq!(em.bindings[0].event_type, "click");
                assert_eq!(em.bindings[0].handler, "count = count + 1");
                assert!(em.bindings[0].modifiers.is_empty());
            }
            other => panic!("Expected Event marker, got {:?}", other.map(|_| "other")),
        }
    }

    #[test]
    fn test_decode_server_generated_expression_marker() {
        let payload = "AQAFY291bnQBBWNvdW50AgA=";
        let decoded = try_decode(payload);

        match decoded {
            Some(DecodedMarker::Expression(m)) => {
                assert_eq!(m.id, 1);
                assert_eq!(m.kind, MarkerKind::Expression);
                assert_eq!(m.expr, "count");
                assert_eq!(m.deps, vec!["count"]);
                assert_eq!(m.value, MarkerValue::Int(0));
            }
            other => panic!("Expected Expression marker, got {:?}", other.map(|_| "other")),
        }
    }

    #[test]
    fn test_roundtrip_encode_decode() {
        use base64::Engine as _;

        let marker = Marker {
            id: 5,
            kind: MarkerKind::Expression,
            expr: "x + y".to_string(),
            deps: vec!["x".to_string(), "y".to_string()],
            value: MarkerValue::Float(3.14),
        };

        let bytes = postcard::to_allocvec(&marker).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let decoded = decode_marker(&encoded).unwrap();
        assert_eq!(decoded, marker);
    }
}
