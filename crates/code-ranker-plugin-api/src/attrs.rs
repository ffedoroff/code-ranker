//! Free-form attributes for nodes and edges — a string-keyed map of scalar
//! values — plus the value-type tag used by the attribute dictionaries.
//!
//! This is what keeps the model **generic**: the plugin chooses the keys it
//! knows (`"path"`, `"loc"`, `"visibility"`, `"version"`, or language-specific
//! ones), the orchestrator adds computed keys (metrics), and consumers read the
//! keys they understand via an [`AttributeSpec`](crate::AttributeSpec) dictionary
//! (label/hint/type) carried in the snapshot.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Attribute bag. `BTreeMap` for deterministic (alphabetical) key order, so
/// snapshots stay byte-stable.
pub type Attributes = BTreeMap<String, AttrValue>;

/// A scalar attribute value, serialized to its natural JSON form (no wrapper).
///
/// Numeric rounding (e.g. 3-significant-digit truncation for metrics) is applied
/// by the producer *before* inserting a `Float`, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttrValue {
    // Order matters for untagged deserialization: bool before the numerics so
    // `true`/`false` aren't misread, integers before floats, string last.
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

/// The kind of value an attribute holds — tells the UI what it can DO with the
/// field (numbers: sum/average; strings: concatenate/count; bools: count). Used
/// in [`AttributeSpec`](crate::AttributeSpec) to describe a key independently of
/// whether any value is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Bool,
    Int,
    Float,
    Str,
}

impl AttrValue {
    /// The [`ValueType`] tag for this value — single source of truth for the
    /// value/type mapping.
    pub fn value_type(&self) -> ValueType {
        match self {
            AttrValue::Bool(_) => ValueType::Bool,
            AttrValue::Int(_) => ValueType::Int,
            AttrValue::Float(_) => ValueType::Float,
            AttrValue::Str(_) => ValueType::Str,
        }
    }
}

impl From<bool> for AttrValue {
    fn from(v: bool) -> Self {
        AttrValue::Bool(v)
    }
}
impl From<i64> for AttrValue {
    fn from(v: i64) -> Self {
        AttrValue::Int(v)
    }
}
impl From<u32> for AttrValue {
    fn from(v: u32) -> Self {
        AttrValue::Int(v as i64)
    }
}
impl From<f64> for AttrValue {
    fn from(v: f64) -> Self {
        AttrValue::Float(v)
    }
}
impl From<String> for AttrValue {
    fn from(v: String) -> Self {
        AttrValue::Str(v)
    }
}
impl From<&str> for AttrValue {
    fn from(v: &str) -> Self {
        AttrValue::Str(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_type_maps_each_variant() {
        assert_eq!(AttrValue::Bool(true).value_type(), ValueType::Bool);
        assert_eq!(AttrValue::Int(1).value_type(), ValueType::Int);
        assert_eq!(AttrValue::Float(1.5).value_type(), ValueType::Float);
        assert_eq!(AttrValue::Str("x".into()).value_type(), ValueType::Str);
    }

    #[test]
    fn from_impls_cover_each_scalar() {
        assert_eq!(AttrValue::from(true), AttrValue::Bool(true));
        assert_eq!(AttrValue::from(7_i64), AttrValue::Int(7));
        assert_eq!(AttrValue::from(7_u32), AttrValue::Int(7));
        assert_eq!(AttrValue::from(2.5_f64), AttrValue::Float(2.5));
        assert_eq!(AttrValue::from("s".to_string()), AttrValue::Str("s".into()));
        assert_eq!(AttrValue::from("s"), AttrValue::Str("s".into()));
    }
}
