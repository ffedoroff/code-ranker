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

/// The Python config: `python.toml` deep-merged over the shared `defaults.toml`,
/// used to build the principle list (the common catalog + Python's `doc_lang`).
static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&PythonPlugin)
}

/// The Python language plugin (registered by the CLI).
pub struct PythonPlugin;

impl LanguagePlugin for PythonPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "python"
    }

    fn detect(&self, cfg: &toml::Table, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the detect logic stays in Rust).
        crate::config::string_list(cfg, "detect_markers")
            .iter()
            .any(|f| workspace.join(f).exists())
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        // The `uses` edge kind is shared vocab: read it from `[edge_kinds]` in
        // the merged config (Python inherits it verbatim from `defaults.toml`).
        let edge_kinds = crate::config::edge_kinds(cfg);

        // Structural node-attribute display specs are DATA: Python inherits the
        // shared `path`/`loc`/`visibility`/`external` from `defaults.toml` (it
        // adds none of its own), read via the merged config.
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
            // Optional sub-file level (off by default; see `[levels] functions`).
            // Metric attribute specs are merged centrally; here we only declare
            // the per-language unit kinds.
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
        // The common catalog from `defaults.toml`, with `doc_url` resolved to
        // `{doc_base}/python/<slug>.md` (Python adds no principles of its own).
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
        // Python `[specs.<key>]` overrides (the exact Halstead tokens it counts).
        crate::config::apply_spec_overrides(defaults, cfg)
    }
}

/// Measure Python complexity metrics for every `file` node, parsing each file
/// (by its absolute-path `id`) with our `tree-sitter-python` engine, returning a
/// [`MetricInputs`] keyed by file node id (the orchestrator writes them). Files
/// that cannot be read or parsed are skipped.
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

/// Per-language unit kinds for the `functions` level (free-form `kind` strings,
/// rendered via this dictionary — the viewer hardcodes no kind by name). Read
/// from `[node_kinds]` in the merged config; Python inherits `function` /
/// `method` verbatim from `defaults.toml` and adds none of its own.
fn function_node_kinds(cfg: &toml::Table) -> BTreeMap<String, NodeKindSpec> {
    crate::config::node_kinds(cfg)
}

/// Build function-level units for every `file` node, parsing each file (by its
/// absolute-path `id`) and running the per-function engine. Each pair is the
/// unit's node (`parent` = file id, id `<file>#<name>@<start_line>`, stable for
/// diff / SARIF, **no metrics yet**) plus its measured inputs (the orchestrator
/// writes them). Called before relativization, so file ids are absolute and readable.
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
