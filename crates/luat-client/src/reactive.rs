// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Reactive state management for the client runtime.
//!
//! Implements a fine-grained dependency graph where:
//! - `$state` variables are atomic mutable values
//! - `$derived` variables are computed from other reactive values
//! - Subscribers (DOM bindings) are notified when their deps change

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

/// A reactive value that can be a state or derived computation.
#[derive(Debug, Clone)]
pub enum ReactiveValue {
    /// A raw string value (Lua expressions evaluate to strings for DOM).
    Str(String),
    /// Numeric value.
    Num(f64),
    /// Boolean value.
    Bool(bool),
    /// Nil/undefined.
    Nil,
}

impl ReactiveValue {
    /// Convert to string for DOM text content.
    pub fn to_display_string(&self) -> String {
        match self {
            ReactiveValue::Str(s) => s.clone(),
            ReactiveValue::Num(n) => {
                if *n == (*n as i64) as f64 {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            ReactiveValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            ReactiveValue::Nil => String::new(),
        }
    }
}

/// Unique ID for a reactive variable.
pub type VarId = u16;

/// A subscriber that should be notified when a variable changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriberId(pub u16);

/// The kind of DOM update a subscriber needs.
#[derive(Debug, Clone)]
pub enum SubscriberKind {
    /// Update a text node's content.
    TextContent {
        /// The Lua expression to re-evaluate.
        expr: String,
    },
    /// Update an element's attribute.
    Attribute {
        /// The attribute name.
        attr: String,
        /// The Lua expression for the value.
        expr: String,
    },
}

/// A registered subscriber bound to a DOM node.
#[derive(Debug, Clone)]
pub struct Subscriber {
    /// The kind of update needed.
    pub kind: SubscriberKind,
    /// Which reactive variables this subscriber depends on.
    pub deps: Vec<String>,
}

/// A derived (computed) variable definition.
#[derive(Debug, Clone)]
pub struct DerivedVar {
    pub name: String,
    pub expression: String,
    pub deps: Vec<String>,
}

/// The reactive state store.
///
/// Holds all reactive variable values and tracks which subscribers
/// depend on which variables.
pub struct ReactiveStore {
    /// Current values of all reactive variables, keyed by name.
    values: HashMap<String, ReactiveValue>,
    /// Map from variable name to set of subscriber IDs that depend on it.
    dependents: HashMap<String, HashSet<SubscriberId>>,
    /// All registered subscribers.
    subscribers: HashMap<SubscriberId, Subscriber>,
    /// Next subscriber ID.
    next_id: u16,
    /// Registered derived variables.
    derived_vars: Vec<DerivedVar>,
}

impl ReactiveStore {
    /// Creates a new empty reactive store.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            dependents: HashMap::new(),
            subscribers: HashMap::new(),
            next_id: 0,
            derived_vars: Vec::new(),
        }
    }

    /// Sets the initial value of a reactive variable.
    pub fn set_initial(&mut self, name: &str, value: ReactiveValue) {
        self.values.insert(name.to_string(), value);
    }

    /// Gets the current value of a reactive variable.
    pub fn get(&self, name: &str) -> Option<&ReactiveValue> {
        self.values.get(name)
    }

    /// Updates a reactive variable's value.
    /// Returns the set of subscriber IDs that need to be notified.
    pub fn set(&mut self, name: &str, value: ReactiveValue) -> Vec<SubscriberId> {
        self.values.insert(name.to_string(), value);

        // Collect all subscribers that depend on this variable
        self.dependents
            .get(name)
            .map(|deps| deps.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Registers a subscriber and returns its ID.
    pub fn subscribe(&mut self, subscriber: Subscriber) -> SubscriberId {
        let id = SubscriberId(self.next_id);
        self.next_id += 1;

        // Register this subscriber as dependent on each of its deps
        for dep in &subscriber.deps {
            self.dependents
                .entry(dep.clone())
                .or_default()
                .insert(id);
        }

        self.subscribers.insert(id, subscriber);
        id
    }

    /// Registers a subscriber with a specific ID (assigned by JS).
    pub fn subscribe_with_id(&mut self, id: SubscriberId, subscriber: Subscriber) {
        // Register this subscriber as dependent on each of its deps
        for dep in &subscriber.deps {
            self.dependents
                .entry(dep.clone())
                .or_default()
                .insert(id);
        }

        self.subscribers.insert(id, subscriber);
    }

    /// Gets a subscriber by ID.
    pub fn get_subscriber(&self, id: SubscriberId) -> Option<&Subscriber> {
        self.subscribers.get(&id)
    }

    /// Gets all variable names and their current values.
    pub fn all_values(&self) -> &HashMap<String, ReactiveValue> {
        &self.values
    }

    /// Register a derived variable.
    pub fn register_derived(&mut self, name: String, expression: String, deps: Vec<String>) {
        self.derived_vars.push(DerivedVar { name, expression, deps });
    }

    /// Get all derived variables that depend on the given variable name.
    pub fn get_derived_dependents(&self, var_name: &str) -> Vec<&DerivedVar> {
        self.derived_vars
            .iter()
            .filter(|d| d.deps.iter().any(|dep| dep == var_name))
            .collect()
    }

    /// Get all derived vars.
    pub fn all_derived(&self) -> &[DerivedVar] {
        &self.derived_vars
    }
}

impl Default for ReactiveStore {
    fn default() -> Self {
        Self::new()
    }
}
