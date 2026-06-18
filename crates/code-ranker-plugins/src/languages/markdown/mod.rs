//! Markdown language plugin for Code Ranker.
//!
//! Documentation, not code: no Halstead / complexity metrics. Each `.md` file is
//! a node with `loc`, linked by `uses` edges over its Markdown links to other
//! local `.md` files (see `structure.rs`); the orchestrator derives coupling and
//! cycles from that link graph.

use anyhow::Result;
use code_ranker_plugin_api::{
    default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::Level,
    plugin::{LanguagePlugin, PluginInput, Preset},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod structure;

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

/// The Markdown language plugin (registered by the CLI).
pub struct MarkdownPlugin;

impl LanguagePlugin for MarkdownPlugin {
    fn name(&self) -> &str {
        "markdown"
    }

    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool {
        structure::detect(workspace, &crate::walk::ignore_from(input))
    }

    fn levels(&self) -> Vec<Level> {
        vec![Level {
            name: "files".into(),
            edge_kinds: crate::config::edge_kinds(&CONFIG),
            node_attributes: crate::config::node_attributes(&CONFIG),
            edge_attributes: BTreeMap::new(),
            attribute_groups: BTreeMap::new(),
            node_kinds: default_node_kinds(),
            cycle_kinds: default_cycle_kinds(),
            grouping: None,
        }]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        structure::analyze(workspace, &crate::walk::ignore_from(input))
    }

    // No `metrics` / `function_units`: Markdown emits only the structural `loc`
    // (set in `analyze`) plus the orchestrator-derived coupling over the links.

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        // The common catalog is a set of code-refactoring lenses — not meaningful
        // for prose — so Markdown ships none.
        Vec::new()
    }

    fn report_overrides(&self) -> code_ranker_plugin_api::report::ReportOverride {
        crate::list_override::report_override(&CONFIG)
    }
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
