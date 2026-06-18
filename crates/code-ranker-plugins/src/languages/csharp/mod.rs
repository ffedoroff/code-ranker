//! C# language plugin for Code Ranker.
//!
//! Metrics run through the shared engine via `csharp/dialect.rs`; the dependency
//! graph is the `using` / `namespace` graph built by `csharp/structure.rs`.

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
mod structure;

static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

/// The C# language plugin (registered by the CLI).
pub struct CsharpPlugin;

impl LanguagePlugin for CsharpPlugin {
    fn name(&self) -> &str {
        "csharp"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        structure::detect(workspace)
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
        structure::analyze(workspace, input.ignore_tests)
    }

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
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

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
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

    fn is_test_path(&self, rel_path: &str) -> bool {
        structure::is_test_path(rel_path)
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

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
