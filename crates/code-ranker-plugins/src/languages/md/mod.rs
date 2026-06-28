//! Markdown language plugin for Code Ranker.
//!
//! Documentation, not code: no Halstead / complexity metrics. Each `.md` file is
//! a node with `loc`, linked by `uses` edges over its Markdown links to other
//! local `.md` files (see `structure.rs`); the orchestrator derives coupling and
//! cycles from that link graph.

use anyhow::Result;
use code_ranker_plugin_api::{
    Principle, default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::Level,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod structure;

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&MdPlugin)
}

/// The Markdown language plugin (registered by the CLI).
pub struct MdPlugin;

impl LanguagePlugin for MdPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "md"
    }

    fn detect(&self, _cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> bool {
        structure::detect(workspace, &crate::walk::ignore_from(input))
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        vec![Level {
            name: "files".into(),
            edge_kinds: crate::config::edge_kinds(cfg),
            node_attributes: crate::config::node_attributes(cfg),
            edge_attributes: BTreeMap::new(),
            attribute_groups: BTreeMap::new(),
            node_kinds: default_node_kinds(),
            cycle_kinds: default_cycle_kinds(),
            grouping: None,
        }]
    }

    fn analyze(&self, _cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        structure::analyze(workspace, &crate::walk::ignore_from(input))
    }

    // No `metrics` / `function_units`: Markdown emits only the structural `loc`
    // (set in `analyze`) plus the orchestrator-derived coupling over the links.

    fn principles(&self, _cfg: &toml::Table, _input: &PluginInput) -> Vec<Principle> {
        // The common catalog is a set of code-refactoring lenses — not meaningful
        // for prose — so Markdown ships none.
        Vec::new()
    }

    fn report_overrides(
        &self,
        cfg: &toml::Table,
    ) -> code_ranker_plugin_api::report::ReportOverride {
        code_ranker_plugin_api::list_override::report_override(cfg)
    }
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
