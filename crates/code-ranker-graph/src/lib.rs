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
pub mod metrics;
pub mod registry;
pub mod relativize;
pub mod serialize;
pub mod snapshot;
pub mod stats;

pub use attrs::{num_attr, round_sig3};
pub use cycles::annotate_cycles;
pub use finalize::finalize_graph;
pub use hk::annotate_hk;
pub use level_graph::{CycleGroup, LevelGraph, LevelUi};
pub use metrics::{
    FunctionUnit, MetricInputs, UiOrder, metric_specs, stat_keys, ui_order, write_metrics,
};
pub use registry::{Engine, MetricDef, Populations, RegistryError, Scope, apply_to_node};
pub use relativize::{relativize_graph, relativize_level};
pub use serialize::{to_canonical_string, to_canonical_string_pretty};
pub use snapshot::{GitInfo, Snapshot, StageTime};
pub use stats::compute_stats;

use code_ranker_plugin_api::{
    attrs::ValueType,
    level::{AttributeGroup, AttributeSpec, Direction, SpecRow, attr_dict, group},
};
use std::collections::BTreeMap;

/// The coupling/cycle attribute dictionary produced by [`annotate_hk`](hk::annotate_hk) /
/// [`annotate_cycles`](cycles::annotate_cycles), plus the `coupling` group. The orchestrator merges these
/// into each level's `node_attributes` / `attribute_groups`.
pub fn coupling_specs() -> (
    BTreeMap<String, AttributeSpec>,
    BTreeMap<String, AttributeGroup>,
) {
    let specs = attr_dict(vec![
        // No direction: raw fan-in is neutral — broad reuse (good) and bottleneck
        // risk (bad) pull opposite ways, so a growing/shrinking count carries no
        // clear verdict.
        (
            "fan_in",
            SpecRow {
                group: "coupling",
                label: "Fan-in",
                name: "Fan-in",
                short: "Fan-in",
                description: "Number of nodes that depend on this one. High fan-in means broadly reused.",
                ..Default::default()
            },
        ),
        // Also neutral: like fan-in, high fan-out is dual — a tangled unit (bad) or
        // a legitimate coordinator/composition root (fine). The directional coupling
        // signal lives in `hk`, which already folds in fan_out.
        (
            "fan_out",
            SpecRow {
                group: "coupling",
                label: "Fan-out",
                name: "Fan-out",
                short: "Fan-out",
                description: "Number of nodes this one depends on. High fan-out means many \
                              dependencies. External-library edges are counted separately.",
                ..Default::default()
            },
        ),
        (
            "fan_out_external",
            SpecRow {
                group: "coupling",
                label: "Fan-out (external)",
                description: "Number of distinct external libraries this node depends on.",
                ..Default::default()
            },
        ),
        (
            "hk",
            SpecRow {
                group: "coupling",
                value_type: ValueType::Float,
                label: "HK",
                name: "Henry–Kafura",
                short: "HK",
                description: "Henry–Kafura — combines unit size with incoming/outgoing coupling \
                              (internal edges only).",
                formula: "sloc × (fan_in × fan_out)²",
                calc: "sloc * (fan_in * fan_out) ** 2",
                direction: Direction::LowerBetter,
                abbreviate: true,
                ..Default::default()
            },
        ),
        (
            "cycle",
            SpecRow {
                value_type: ValueType::Str,
                label: "Cycle",
                short: "Cycle",
                description: "Cycle kind this node participates in.",
                ..Default::default()
            },
        ),
    ]);

    let mut groups = BTreeMap::new();
    groups.insert(
        "coupling".to_string(),
        group("Coupling", "Internal coupling (Henry-Kafura)"),
    );
    (specs, groups)
}
