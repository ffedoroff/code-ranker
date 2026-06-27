use anyhow::Result;
use code_ranker_plugin_api::{
    Principle, default_cycle_kinds, default_node_kinds,
    graph::Graph,
    level::{AttributeSpec, EdgeKindSpec, Grouping, Level, NodeKindSpec},
    metrics::MetricInputs,
    node::Node,
    plugin::{LanguagePlugin, PluginInput},
};
use std::collections::BTreeMap;
use std::path::Path;

mod analyze;
mod cfg;
mod collapse;
mod crate_graph;
mod dialect;
mod ids;
mod internal;
mod module_graph;
mod strip;
mod test_attr;
mod toolchain;

use analyze::syn_analyze;
use cfg::CONFIG;
use collapse::collapse_to_files;
use internal::GraphBuilder;
use strip::{rust_file_metrics, strip_cfg_test};
use toolchain::{rust_toolchain_roots, version_string};

// Self-register this plugin (collected by `code_ranker_plugin_api::registry`); no
// central list anywhere names a language.
inventory::submit! {
    code_ranker_plugin_api::PluginRegistration(&RustPlugin)
}

pub struct RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn config(&self) -> toml::Table {
        CONFIG.clone()
    }

    fn name(&self) -> &str {
        "rust"
    }

    fn detect(&self, cfg: &toml::Table, workspace: &Path, _input: &PluginInput) -> bool {
        // Project-detect marker filenames are DATA: read from `config.toml`'s
        // `detect_markers` (the detect logic stays in Rust). Rust detects on
        // `Cargo.toml`. (The `cargo metadata` manifest path in `syn_analyze` is
        // separate — that is cargo machinery, not a detect-marker list.)
        crate::config::string_list(cfg, "detect_markers")
            .iter()
            .any(|m| workspace.join(m).exists())
    }

    fn levels(&self, cfg: &toml::Table) -> Vec<Level> {
        // Edge-kind vocabulary (`uses` / `contains` / `reexports` / `super`) is
        // data: read it from `[edge_kinds]` in `rust/config.toml` (which
        // overrides the shared `uses` and adds the Rust-only structural kinds).
        // `collapse.rs` tags edges with the same identifiers via
        // `config::edge_kind_id`, so the spec and the tagged `kind` can't drift.
        let edge_kinds: BTreeMap<String, EdgeKindSpec> = crate::config::edge_kinds(cfg);

        // Structural node/edge attribute display specs are DATA: read from the
        // merged config (`[node_attributes]` / `[edge_attributes]`). The shared
        // `path`/`loc`/`visibility`/`external` come from `defaults.toml`; Rust's
        // `crate`/`version`/`items`/`unsafe` (and edge `visibility`) from `rust/config.toml`.
        let node_attributes = crate::config::node_attributes(cfg);
        let edge_attributes = crate::config::edge_attributes(cfg);

        vec![
            Level {
                name: "files".into(),
                edge_kinds,
                node_attributes,
                edge_attributes,
                attribute_groups: crate::config::attribute_groups(cfg),
                node_kinds: default_node_kinds(),
                cycle_kinds: default_cycle_kinds(),
                // Cluster the diagram by the owning crate (compilation unit), not by
                // the source folder. Falls back to `dir` if `crate` is ever absent.
                grouping: Some(Grouping {
                    // Group by the `crate` node attribute — its key is DATA,
                    // validated against `[node_attributes]`.
                    key: Some(
                        crate::config::attr_key(cfg, "crate")
                            .expect("rust/config.toml [node_attributes] is missing `crate`")
                            .into(),
                    ),
                    function: None,
                }),
            },
            // Optional sub-file level (off by default; `[levels] functions`).
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

    fn principles(&self, cfg: &toml::Table, _input: &PluginInput) -> Vec<Principle> {
        // The common catalog (from `defaults.toml`) plus the Rust-only metric
        // lenses (`[[principles]]` in `rust.toml`), with each `doc_url` resolved to
        // `{doc_base}/rust/<slug>.md`. All data-driven via the shared loader.
        crate::config::resolved_principles(cfg)
    }

    fn report_overrides(
        &self,
        cfg: &toml::Table,
    ) -> code_ranker_plugin_api::report::ReportOverride {
        // Rust's `[report]` patches: e.g. surface the `unsafe` column / stat.
        code_ranker_plugin_api::list_override::report_override(cfg)
    }

    fn analyze(&self, _cfg: &toml::Table, workspace: &Path, input: &PluginInput) -> Result<Graph> {
        let mut builder = GraphBuilder::new();
        syn_analyze(workspace, input.ignore_tests, &mut builder)?;
        let internal = builder.build();
        Ok(collapse_to_files(internal))
    }

    fn metrics(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(String, MetricInputs)> {
        // Each `.rs` file node is re-read (by its absolute-path `id`) and measured
        // by our `tree-sitter-rust` engine; `#[cfg(test)]` / `#[test]` items are
        // stripped first so metrics reflect production code only (their lines
        // become `tloc`). The orchestrator writes the returned inputs.
        let mut out = Vec::new();
        for node in &graph.nodes {
            if node.kind != code_ranker_plugin_api::node::FILE {
                continue;
            }
            let Ok(src) = std::fs::read(&node.id) else {
                continue;
            };
            if let Some(m) = rust_file_metrics(&src) {
                out.push((node.id.clone(), m));
            }
        }
        out
    }

    fn function_units(&self, _cfg: &toml::Table, graph: &Graph) -> Vec<(Node, MetricInputs)> {
        let mut out = Vec::new();
        for node in &graph.nodes {
            if node.kind != code_ranker_plugin_api::node::FILE {
                continue;
            }
            let Ok(src) = std::fs::read(&node.id) else {
                continue;
            };
            // Mirror file metrics: strip inline tests so test fns never appear.
            let (prod, _tloc) = strip_cfg_test(&src);
            for u in dialect::compute_functions(&prod) {
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

    fn versions(
        &self,
        _cfg: &toml::Table,
        _workspace: &Path,
        _input: &PluginInput,
    ) -> Vec<(String, String)> {
        version_string()
            .map(|rv| vec![("rustc".to_string(), rv)])
            .unwrap_or_default()
    }

    fn roots(&self, _cfg: &toml::Table, _workspace: &Path) -> Vec<(String, String)> {
        rust_toolchain_roots()
    }

    fn metric_specs(
        &self,
        cfg: &toml::Table,
        defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        // Apply the Rust `[specs.<key>]` overrides over the central builtin specs:
        // the production-only LOC nuance (`#[cfg(test)]` stripped) and the exact
        // Halstead operator/operand sets Rust counts.
        crate::config::apply_spec_overrides(defaults, cfg)
    }
}

/// Per-language unit kinds for the `functions` level (rendered via this dict —
/// the viewer hardcodes no kind by name). Read from `[node_kinds]` in the merged
/// config: the shared `method` from `defaults.toml` plus Rust's own `fn`
/// (Rust labels its free functions `fn`, not the generic `function`). The
/// inherited generic `function` entry is also published; it is harmless on this
/// off-by-default level (the dialect's `fn_kind` only ever tags `fn` / `method`).
fn function_node_kinds(cfg: &toml::Table) -> BTreeMap<String, NodeKindSpec> {
    crate::config::node_kinds(cfg)
}

#[cfg(test)]
#[path = "tests/mod_rs.rs"]
mod mod_rs_tests;
