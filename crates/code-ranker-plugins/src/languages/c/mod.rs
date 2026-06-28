//! C language plugin for Code Ranker.
//!
//! Metrics run through the shared engine via `c/dialect.rs`; the dependency graph
//! is the `#include` graph built by the shared `../cfamily/` module.

use anyhow::Result;
use code_ranker_plugin_api::{
    Principle, default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod dialect;

use crate::languages::cfamily;

// Inheritance chain `defaults.toml ⊕ cfamily/config.toml ⊕ c/config.toml`: the
// shared C-family base carries the `#include` graph vocab and the node-kind
// entries identical for C and C++; `c/config.toml` adds only C specifics.
static CONFIG: LazyLock<toml::Table> = LazyLock::new(|| {
    crate::config::load_chain(&[
        include_str!("../cfamily/config.toml"),
        include_str!("config.toml"),
    ])
});

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&CPlugin)
}

/// The C language plugin (registered by the CLI).
pub struct CPlugin;

impl LanguagePlugin for CPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "c"
    }

    fn detect(&self, cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> bool {
        let c = cfamily::Cfg::from_config(cfg);
        cfamily::detect(
            workspace,
            &c,
            input.ignore_tests,
            &crate::walk::ignore_from(input),
        )
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        vec![
            Level {
                name: "files".into(),
                edge_kinds: crate::config::edge_kinds(cfg),
                node_attributes: crate::config::node_attributes(cfg),
                edge_attributes: BTreeMap::new(),
                attribute_groups: BTreeMap::new(),
                node_kinds: default_node_kinds(),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
            Level {
                name: "functions".into(),
                edge_kinds: BTreeMap::new(),
                node_attributes: BTreeMap::new(),
                edge_attributes: BTreeMap::new(),
                attribute_groups: BTreeMap::new(),
                node_kinds: crate::config::node_kinds(cfg),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
        ]
    }

    fn analyze(&self, cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        let c = cfamily::Cfg::from_config(cfg);
        cfamily::analyze(
            workspace,
            input.ignore_tests,
            &c,
            &crate::walk::ignore_from(input),
        )
    }

    fn metrics(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(String, MetricInputs)> {
        file_metrics(graph)
    }

    fn function_units(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        function_nodes(graph)
    }

    fn principles(&self, cfg: &toml::Table, _input: &PluginInput) -> Vec<Principle> {
        crate::config::resolved_principles(cfg)
    }

    fn report_overrides(
        &self,
        cfg: &toml::Table,
    ) -> code_ranker_plugin_api::report::ReportOverride {
        code_ranker_plugin_api::list_override::report_override(cfg)
    }

    fn metric_specs(
        &self,
        cfg: &toml::Table,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        crate::config::apply_spec_overrides(defaults, cfg)
    }
}

/// Measure C complexity metrics for every `file` node (parsing each by its
/// absolute-path `id`); files that cannot be read/parsed are skipped.
fn file_metrics(graph: &Graph) -> Vec<(String, MetricInputs)> {
    let mut out = Vec::new();
    for node in &graph.nodes {
        if node.kind != code_ranker_plugin_api::node::FILE {
            continue;
        }
        let Ok(src) = std::fs::read(&node.id) else {
            continue;
        };
        if let Some(m) = dialect::compute(&src) {
            out.push((node.id.clone(), m));
        }
    }
    out
}

/// Build function-level units for every `file` node.
fn function_nodes(graph: &Graph) -> Vec<(Node, MetricInputs)> {
    let mut out = Vec::new();
    for node in &graph.nodes {
        if node.kind != code_ranker_plugin_api::node::FILE {
            continue;
        }
        let Ok(src) = std::fs::read(&node.id) else {
            continue;
        };
        for u in dialect::compute_functions(&src) {
            out.push((
                Node {
                    id: format!("{}#{}@{}", node.id, u.name, u.start_line),
                    kind: u.kind.clone(),
                    name: u.name.clone(),
                    parent: Some(node.id.clone()),
                    attrs: Default::default(),
                },
                u.inputs,
            ));
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
