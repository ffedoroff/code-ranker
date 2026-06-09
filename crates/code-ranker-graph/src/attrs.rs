//! Shared attribute helpers used by every enrichment pass: numeric rounding,
//! the `f64 → AttrValue` bridge, typed attribute reads, and the external-node
//! predicate. This is a leaf module — it depends only on the plugin API, never
//! on the crate root, so the enrichment passes can pull helpers from here
//! without creating a `submodule → crate-root` back-edge.

use code_ranker_plugin_api::{attrs::AttrValue, node::Node};

/// Truncate to 3 significant digits (matching the historical `sig3` serializer):
/// values ≥ 1 are truncated to 3 decimals, values < 1 to 3 significant figures.
/// Non-finite values collapse to 0 (JSON has no NaN/Inf).
pub fn round_sig3(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return 0.0;
    }
    let abs = x.abs();
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let truncated = if abs >= 1.0 {
        (abs * 1000.0).floor() / 1000.0
    } else {
        let d = abs.log10().floor() as i32;
        let factor = 10f64.powi(2 - d);
        (abs * factor).floor() / factor
    };
    truncated * sign
}

/// Round a metric and pick the natural JSON scalar: an integral value becomes
/// an `Int` (so `1.0` serializes as `1`), otherwise a `Float`. This is the
/// single bridge metric producers use before inserting into `attrs`.
pub fn num_attr(x: f64) -> AttrValue {
    let r = round_sig3(x);
    if r.fract() == 0.0 && r.abs() < i64::MAX as f64 {
        AttrValue::Int(r as i64)
    } else {
        AttrValue::Float(r)
    }
}

/// Read a numeric node attribute as `f64` (from either `Int` or `Float`).
pub(crate) fn attr_f64(node: &Node, key: &str) -> Option<f64> {
    match node.attrs.get(key) {
        Some(AttrValue::Int(i)) => Some(*i as f64),
        Some(AttrValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// Is this node an external dependency (a library node, not a project file)?
/// Derived from `kind == "external"` or an explicit `external: true` attribute.
pub(crate) fn is_external(node: &Node) -> bool {
    node.kind == "external" || matches!(node.attrs.get("external"), Some(AttrValue::Bool(true)))
}
