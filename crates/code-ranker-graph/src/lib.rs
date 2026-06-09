//! Operations over the generic property-graph model defined in
//! `code-ranker-plugin-api`: cycle detection, Henry-Kafura coupling, aggregate
//! stats, id relativization, and the serializable [`Snapshot`] artifact.
//!
//! Everything here is language-agnostic. Plugins emit a pure
//! [`api::Graph`](code_ranker_plugin_api::graph::Graph) (structure only); this crate
//! and the orchestrator enrich it (writing computed values into node `attrs`
//! by id) and assemble the snapshot. Which edge kinds count as information
//! flow is read from the level's `edge_kinds` (`EdgeKindSpec.flow`), passed in
//! as a `flow_kinds` set — there is no hardcoded `uses`/`contains` knowledge.

pub mod attrs;
pub mod cycles;
pub mod finalize;
pub mod hk;
pub mod level_graph;
pub mod relativize;
pub mod serialize;
pub mod snapshot;
pub mod stats;

pub use attrs::{num_attr, round_sig3};
pub use cycles::annotate_cycles;
pub use finalize::finalize_graph;
pub use hk::annotate_hk;
pub use level_graph::{CycleGroup, LevelGraph, LevelUi};
pub use relativize::{relativize_graph, relativize_level};
pub use serialize::{to_canonical_string, to_canonical_string_pretty};
pub use snapshot::{GitInfo, Snapshot, StageTime};
pub use stats::compute_stats;

use code_ranker_plugin_api::{
    attrs::ValueType,
    level::{AttributeGroup, AttributeSpec},
};
use std::collections::BTreeMap;

/// The coupling/cycle attribute dictionary produced by [`annotate_hk`](hk::annotate_hk) /
/// [`annotate_cycles`](cycles::annotate_cycles), plus the `coupling` group. The orchestrator merges these
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
    // No direction: raw fan-in is neutral — broad reuse (good) and bottleneck risk
    // (bad) pull opposite ways, so a growing/shrinking count carries no clear verdict.
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
    // Lower is better: outgoing dependencies are efferent coupling — a node that
    // depends on more things is harder to change in isolation (mirrors HK).
    fan_out.direction = Some("lower_better".into());
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
