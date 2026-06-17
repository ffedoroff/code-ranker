use anyhow::Result;
use code_ranker_plugin_api::{
    default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{Level, NodeKindSpec},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput, Preset},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

mod dialect;
mod structure;

/// The Python config: `python.toml` deep-merged over the shared `defaults.toml`,
/// used to build the preset list (the common catalog + Python's `doc_lang`).
static CONFIG: LazyLock<toml::Table> =
    LazyLock::new(|| crate::config::load(include_str!("config.toml")));

/// The Python language plugin (registered by the CLI).
pub struct PythonPlugin;

impl LanguagePlugin for PythonPlugin {
    fn name(&self) -> &str {
        "python"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the detect logic stays in Rust).
        crate::config::string_list(&CONFIG, "detect_markers")
            .iter()
            .any(|f| workspace.join(f).exists())
    }

    fn levels(&self) -> Vec<Level> {
        // The `uses` edge kind is shared vocab: read it from `[edge_kinds]` in
        // the merged config (Python inherits it verbatim from `defaults.toml`).
        let edge_kinds = crate::config::edge_kinds(&CONFIG);

        // Structural node-attribute display specs are DATA: Python inherits the
        // shared `path`/`loc`/`visibility`/`external` from `defaults.toml` (it
        // adds none of its own), read via the merged config.
        let node_attributes = crate::config::node_attributes(&CONFIG);

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
                node_kinds: function_node_kinds(),
                cycle_kinds: default_cycle_kinds(),
                grouping: None,
            },
        ]
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        structure::analyze(workspace, input.ignore_tests)
    }

    fn metrics(&self, graph: &Graph) -> Vec<(String, MetricInputs)> {
        file_metrics(graph)
    }

    fn function_units(&self, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        function_nodes(graph)
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        structure::py_is_test_path(rel_path)
    }

    fn presets(&self, _input: &PluginInput) -> Vec<Preset> {
        // The common catalog from `defaults.toml`, with `doc_url` resolved to
        // `{doc_base}/python/<slug>.md` (Python adds no presets of its own).
        crate::config::resolved_presets(&CONFIG)
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
fn function_node_kinds() -> BTreeMap<String, NodeKindSpec> {
    crate::config::node_kinds(&CONFIG)
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
