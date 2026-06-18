//! # code-ranker-plugin-api
//!
//! The contract everything in Code Ranker builds on: a **generic property-graph
//! model** plus the [`LanguagePlugin`](plugin::LanguagePlugin) trait. This crate is the foundation — it
//! depends on **nothing** else from Code Ranker and re-exports nothing. Every
//! other crate (graph operations, complexity, language plugins, viewer, cli)
//! depends on *this*.
//!
//! ## Model
//!
//! Analysis produces a [`Graph`](graph::Graph) of **[`Node`](node::Node)**s connected by **[`Edge`](edge::Edge)**s.
//! A node is *anything we analyze*: today a source file (`kind == "file"`),
//! tomorrow a folder, module, function, variable or line — with **no model
//! change**. `kind` is a free-form [`String`] (the plugin's own vocabulary);
//! the core never interprets it, it only stores and projects.
//!
//! Both nodes and edges carry free-form **[`Attributes`](attrs::Attributes)** (string key →
//! scalar [`AttrValue`](attrs::AttrValue)). There is no fixed, file/language-specific field set:
//! the plugin chooses keys (`path`, `loc`, `visibility`, `version`, or
//! language-specific ones), the orchestrator adds computed keys (metrics,
//! cycle), and the core reads only the keys it understands. Each level describes
//! its keys with an [`AttributeSpec`](level::AttributeSpec) dictionary (type + label/hint), so the UI
//! knows what each key means and what it can do with it.
//!
//! ## Responsibilities
//!
//! A [`LanguagePlugin`](plugin::LanguagePlugin) is a **pure parser**: it turns a workspace into nodes +
//! edges at a requested level (by name; see [`Level`](level::Level)). It does **not**
//! compute metrics — complexity / cycles / Henry-Kafura / stats are filled in
//! centrally, for all languages, by the orchestrator. The plugin also describes
//! its edge kinds ([`EdgeKindSpec`](level::EdgeKindSpec)) and attribute keys
//! ([`AttributeSpec`](level::AttributeSpec)), so the core scores, draws and labels unknown
//! kinds/keys without hardcoding their names.

pub mod attrs;
pub mod detection;
pub mod edge;
pub mod graph;
pub mod level;
pub mod list_override;
pub mod log;
pub mod metrics;
pub mod node;
pub mod plugin;
pub mod preset;
pub mod report;
pub mod toml_merge;

pub use attrs::{AttrValue, Attributes, ValueType};
pub use detection::detect_with_marker;
pub use edge::Edge;
pub use graph::Graph;
pub use level::{
    AttributeGroup, AttributeSpec, CycleKindSpec, Direction, EdgeKindSpec, Level, NodeKindSpec,
    SpecRow, Thresholds, attr_dict, group,
};
pub use metrics::{FunctionUnit, MetricInputs};
pub use node::{Node, NodeId};
pub use plugin::{LanguagePlugin, PluginInput, PluginRegistration, registry};
pub use preset::{Preset, PromptTemplate};
pub use report::{ListPatch, ReportOverride};

use std::collections::BTreeMap;

/// The generic node-kind palette every file-based plugin seeds its level with:
/// `file` (a project source unit, blue) and `external` (a third-party library,
/// amber, flagged external). A plugin may recolor or add kinds.
pub fn default_node_kinds() -> BTreeMap<String, NodeKindSpec> {
    BTreeMap::from([
        (
            node::FILE.to_string(),
            NodeKindSpec {
                label: Some("File".into()),
                plural: Some("Files".into()),
                fill: Some("#dbe9f4".into()),
                stroke: Some("#4d6f9c".into()),
                external: None,
            },
        ),
        (
            node::EXTERNAL.to_string(),
            NodeKindSpec {
                label: Some("Library".into()),
                plural: Some("Libraries".into()),
                fill: Some("#f6e2c0".into()),
                stroke: Some("#b3801f".into()),
                external: Some(true),
            },
        ),
    ])
}

/// The generic cycle-kind keys (`mutual` / `chain`) a file-based plugin's level
/// declares. The diagnostic vocabulary (label / `description` = why /
/// `remediation` = fix) is **data, not code**: it is filled centrally by the
/// orchestrator from `code-ranker-graph`'s `cycle_specs` (the `builtin.toml`
/// `[cycles.*]` catalog), so no cycle prose lives here.
pub fn default_cycle_kinds() -> BTreeMap<String, CycleKindSpec> {
    ["mutual", "chain"]
        .into_iter()
        .map(|k| {
            (
                k.to_string(),
                CycleKindSpec {
                    label: None,
                    description: None,
                    remediation: None,
                },
            )
        })
        .collect()
}
