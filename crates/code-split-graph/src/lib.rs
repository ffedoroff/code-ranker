//! Operations over the generic property-graph model defined in
//! `code-split-plugin-api`: cycle detection, Henry-Kafura coupling, aggregate
//! stats, id relativization, and the serializable [`Snapshot`] artifact.
//!
//! Everything here is language-agnostic. Plugins emit a pure
//! [`api::Graph`](code_split_plugin_api::Graph) (structure only); this crate
//! and the orchestrator enrich it (writing computed values into node `attrs`
//! by id) and assemble the snapshot. Which edge kinds count as information
//! flow is read from the level's `edge_kinds` (`EdgeKindSpec.flow`), passed in
//! as a `flow_kinds` set — there is no hardcoded `uses`/`contains` knowledge.

pub mod cycles;
pub mod diff;
pub mod finalize;
pub mod hk;
pub mod snapshot;
pub mod stats;

pub use cycles::annotate_cycles;
pub use diff::compare_snapshots;
pub use finalize::finalize_graph;
pub use hk::annotate_hk;
pub use snapshot::{
    CycleGroup, GitInfo, LevelGraph, LevelUi, Snapshot, StageTime, relativize_graph,
    relativize_level, to_canonical_string, to_canonical_string_pretty,
};
pub use stats::compute_stats;

use code_split_plugin_api::{AttrValue, AttributeGroup, AttributeSpec, Node, ValueType};
use std::collections::BTreeMap;

/// The coupling/cycle attribute dictionary produced by [`annotate_hk`] /
/// [`annotate_cycles`], plus the `coupling` group. The orchestrator merges these
/// into each level's `node_attributes` / `attribute_groups`.
pub fn coupling_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    let mut specs = BTreeMap::new();

    let mut fan_in = AttributeSpec::new(ValueType::Int, "Fan-in");
    fan_in.group = Some("coupling".into());
    fan_in.name = Some("Fan-in".into());
    fan_in.short = Some("Fan-in".into());
    fan_in.description =
        Some("Number of nodes that depend on this one. High fan-in means broadly reused.".into());
    fan_in.direction = Some("higher_better".into());
    specs.insert("fan_in".to_string(), fan_in);

    let mut fan_out = AttributeSpec::new(ValueType::Int, "Fan-out");
    fan_out.group = Some("coupling".into());
    fan_out.name = Some("Fan-out".into());
    fan_out.short = Some("Fan-out".into());
    fan_out.description = Some(
        "Number of nodes this one depends on. High fan-out means many dependencies. \
         External-library edges are counted separately."
            .into(),
    );
    fan_out.direction = Some("higher_better".into());
    specs.insert("fan_out".to_string(), fan_out);

    let mut foe = AttributeSpec::new(ValueType::Int, "Fan-out (external)");
    foe.group = Some("coupling".into());
    foe.description = Some("Number of distinct external libraries this node depends on.".into());
    specs.insert("fan_out_external".to_string(), foe);

    let mut hk = AttributeSpec::new(ValueType::Float, "HK");
    hk.group = Some("coupling".into());
    hk.name = Some("Henry–Kafura (HK)".into());
    hk.short = Some("HK".into());
    hk.description = Some(
        "Henry–Kafura — combines unit size with incoming/outgoing coupling (internal edges only)."
            .into(),
    );
    hk.formula = Some("sloc × (fan_in × fan_out)²".into());
    hk.calc = Some("sloc * (fan_in * fan_out) ** 2".into());
    hk.direction = Some("lower_better".into());
    hk.abbreviate = Some(true);
    specs.insert("hk".to_string(), hk);

    let mut cycle = AttributeSpec::new(ValueType::Str, "Cycle");
    cycle.short = Some("Cycle".into());
    cycle.description = Some("Cycle kind this node participates in.".into());
    specs.insert("cycle".to_string(), cycle);

    let mut groups = BTreeMap::new();
    groups.insert(
        "coupling".to_string(),
        AttributeGroup {
            label: Some("Coupling".to_string()),
            description: Some("Internal coupling (Henry-Kafura)".to_string()),
        },
    );
    (specs, groups)
}

// ---------------------------------------------------------------------------
// Numeric helpers shared by the enrichment passes
// ---------------------------------------------------------------------------

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
