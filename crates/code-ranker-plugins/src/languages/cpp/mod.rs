//! C++ language plugin for Code Ranker.
//!
//! Metrics run through the shared engine via `cpp/dialect.rs`; the dependency
//! graph is the `#include` graph built by the shared `../cfamily/` module.

use anyhow::Result;
use code_ranker_plugin_api::{
    default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{AttributeSpec, Level},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput, Preset},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod dialect;

use crate::languages::cfamily;

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));
static CFG: LazyLock<cfamily::Cfg> = LazyLock::new(|| cfamily::Cfg::from_config(&CONFIG));

/// The C++ language plugin (registered by the CLI).
pub struct CppPlugin;

impl LanguagePlugin for CppPlugin {
    fn name(&self) -> &str {
        "cpp"
    }

    fn detect(&self, workspace: &Path, input: &PluginInput) -> bool {
        cfamily::detect(workspace, &CFG, &crate::walk::ignore_from(input))
    }

    fn levels(&self) -> Vec<Level> {
        vec![
            Level {
                name: "files".into(),
                edge_kinds: crate::config::edge_kinds(&CONFIG),
                node_attributes: crate::config::node_attributes(&CONFIG),
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
                node_kinds: crate::config::node_kinds(&CONFIG),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
        ]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        cfamily::analyze(
            workspace,
            input.ignore_tests,
            &CFG,
            &crate::walk::ignore_from(input),
        )
    }

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
        file_metrics(graph)
    }

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        function_nodes(graph)
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        cfamily::is_test_path(rel_path, &CFG)
    }

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        crate::config::resolved_presets(&CONFIG)
    }

    fn report_overrides(&self) -> code_ranker_plugin_api::report::ReportOverride {
        crate::list_override::report_override(&CONFIG)
    }

    fn metric_specs(
        &self,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        crate::config::apply_spec_overrides(defaults, &CONFIG)
    }
}

/// Measure C++ complexity metrics for every `file` node (parsing each by its
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
