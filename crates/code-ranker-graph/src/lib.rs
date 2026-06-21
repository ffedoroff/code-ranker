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
pub mod builtin;
pub mod checks;
pub mod cycles;
pub mod finalize;
pub mod hk;
pub mod level_graph;
pub(crate) mod nodepath;
pub mod registry;
pub mod relativize;
pub mod serialize;
pub mod snapshot;
pub mod stats;

pub use attrs::{num_attr, round_sig3};
pub use cycles::annotate_cycles;
pub use finalize::finalize_graph;
pub use hk::annotate_coupling;
pub use level_graph::{CycleGroup, LevelGraph, LevelUi};
// The metric catalog reads the `builtin.toml` schema (module [`builtin`]). The
// tier-1 input types (`MetricInputs` / `FunctionUnit`) are the plugin↔orchestrator
// contract and live in `code-ranker-plugin-api` (its `metrics` module); a plugin
// hands them back and `builtin::write_metrics` enriches the node from them.
pub use builtin::{
    Views, coupling_specs, cycle_specs, metric_specs, prompt_template, prompt_template_from,
    stat_keys, views, write_derived, write_metrics,
};
pub use checks::{CheckCompileError, CheckDef, CheckHit, CompiledCheck, GraphView};
pub use registry::{Engine, MetricDef, Populations, RegistryError, Scope, apply_to_node};
pub use relativize::{relativize_graph, relativize_level};
pub use serialize::{to_canonical_string, to_canonical_string_pretty};
pub use snapshot::{GitInfo, Snapshot, StageTime};
pub use stats::compute_stats;

// The coupling/cycle attribute specs (`fan_in` / `fan_out` / `fan_out_external` /
// `hk` / `cycle`) now live in the data-driven `builtin.toml` `[coupling.*]`
// catalog and are exposed via [`builtin::coupling_specs`] (re-exported above).
