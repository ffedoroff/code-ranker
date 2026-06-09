//! Per-level payload types: [`LevelGraph`] (one analysis level's structural
//! graph + semantics dictionaries + computed cycles/stats/UI), [`LevelUi`]
//! (computed UI hints), and [`CycleGroup`] (one classified SCC). These are the
//! widely-imported model types; keeping them in their own module spreads their
//! fan-in off the crate's `snapshot` artifact.

use code_ranker_plugin_api::{
    attrs::AttrValue,
    edge::Edge,
    level::{AttributeGroup, AttributeSpec, CycleKindSpec, EdgeKindSpec, Grouping, NodeKindSpec},
    node::{Node, NodeId},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// UI hints for a level: which metrics to offer as table columns, summary rows,
/// sort/size keys, and the default sort — computed by the orchestrator from the
/// attributes actually present, so the viewer hardcodes none of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LevelUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_sort: Option<String>,
    pub sort_metrics: Vec<String>,
    pub size_metrics: Vec<String>,
    pub card_metrics: Vec<String>,
    pub columns: Vec<String>,
    pub summary_metrics: Vec<String>,
    /// How the viewer should cluster nodes (group by attribute `key`, or a named
    /// `function`). Carried through from the plugin's level spec, pruned to a
    /// valid attribute. Absent → the viewer uses its default `dir` grouper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<Grouping>,
}

/// One strongly-connected component with ≥ 2 nodes, plus its classification
/// (`"mutual"` for a 2-node SCC, `"chain"` for 3+). Node ids match the level graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleGroup {
    pub kind: String,
    pub nodes: Vec<NodeId>,
}

/// Everything for one analysis level: the structural graph, the semantics
/// dictionaries that describe its vocabulary, and the computed cycles + stats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LevelGraph {
    /// Edge kinds present at this level (keyed by kind), with `flow` semantics.
    pub edge_kinds: BTreeMap<String, EdgeKindSpec>,
    /// Node attribute dictionary (structural keys + appended computed metrics).
    pub node_attributes: BTreeMap<String, AttributeSpec>,
    /// Edge attribute dictionary.
    pub edge_attributes: BTreeMap<String, AttributeSpec>,
    /// Attribute group definitions referenced by `AttributeSpec.group`.
    pub attribute_groups: BTreeMap<String, AttributeGroup>,
    /// Node-kind vocabulary (label/colour/external).
    pub node_kinds: BTreeMap<String, NodeKindSpec>,
    /// Cycle-kind vocabulary.
    pub cycle_kinds: BTreeMap<String, CycleKindSpec>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// SCCs with ≥ 2 members, classified by kind.
    pub cycles: Vec<CycleGroup>,
    /// Per-graph averages of numeric node attributes (flat, keyed by attr name).
    pub stats: BTreeMap<String, AttrValue>,
    /// Computed UI hints (column/sort/size/card ordering).
    pub ui: LevelUi,
}
