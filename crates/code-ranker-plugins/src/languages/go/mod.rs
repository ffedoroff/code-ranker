//! Go language plugin for Code Ranker.
//!
//! A thin adapter over the shared, grammar-agnostic engine in `crate::engine`
//! (parameterised by `go/dialect.rs`) plus a Go dependency-graph builder
//! (`structure.rs`). Detects projects by `go.mod`.

use anyhow::Result;
use code_ranker_plugin_api::{
    Principle, default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{AttributeSpec, Level, NodeKindSpec},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod dialect;
mod structure;

/// The Go config: `config.toml` deep-merged over the shared `defaults.toml`.
static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&GoPlugin)
}

/// The Go language plugin (registered by the CLI).
pub struct GoPlugin;

impl LanguagePlugin for GoPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "go"
    }

    fn detect(&self, cfg: &toml::Table, workspace: &Path, _input: &PluginInput) -> bool {
        crate::config::string_list(cfg, "detect_markers")
            .iter()
            .any(|f| workspace.join(f).exists())
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        let edge_kinds = crate::config::edge_kinds(cfg);
        let node_attributes = crate::config::node_attributes(cfg);
        vec![
            Level {
                name: "files".into(),
                edge_kinds,
                node_attributes,
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
                node_kinds: function_node_kinds(cfg),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
        ]
    }

    fn analyze(&self, _cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        structure::analyze(
            workspace,
            input.ignore_tests,
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

/// Measure Go complexity metrics for every `file` node (parsing each by its
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

/// Per-language unit kinds for the `functions` level (inherited `function` /
/// `method` from `defaults.toml`; Go adds none of its own).
fn function_node_kinds(cfg: &toml::Table) -> BTreeMap<String, NodeKindSpec> {
    crate::config::node_kinds(cfg)
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
            let fnode = Node {
                id: format!("{}#{}@{}", node.id, u.name, u.start_line),
                kind: u.kind.clone(),
                name: u.name.clone(),
                parent: Some(node.id.clone()),
                attrs: Default::default(),
            };
            out.push((fnode, u.inputs));
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
