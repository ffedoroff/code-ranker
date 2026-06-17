//! Shared ECMAScript engine for Code Ranker.
//!
//! The grammar-agnostic import/module-graph walker, resolver, level descriptor
//! and metric helper that the JavaScript and TypeScript plugins are built on.
//! The concrete tree-sitter grammar is **injected by the caller** (the JS / TS
//! plugins), so this crate names no language — both plugins depend on it as
//! peers, and neither plugin depends on the other.
//!
//! `mod.rs` is the thin wiring: the level descriptors and metric helpers that
//! read [`cfg::CONFIG`] and delegate to [`dialect`] / [`structure`]. The
//! import/dependency-graph structure builder lives in [`structure`]; its public
//! helpers are re-exported here so the JS / TS plugins reach them at the
//! `ecmascript::` path.

use code_ranker_plugin_api::graph::Graph;
use code_ranker_plugin_api::metrics::MetricInputs;
use code_ranker_plugin_api::{default_cycle_kinds, default_node_kinds, level::Level, node::Node};
use std::collections::BTreeMap;
use std::path::Path;

mod cfg;
mod dialect;
mod structure;

pub use structure::{analyze_ecmascript, ecmascript_is_test_path, external_package};

// ─────────────────────────────────────────────────────────────────────────────
// Public shared helpers (used by the TypeScript plugin)
// ─────────────────────────────────────────────────────────────────────────────

/// Build the single "files" [`Level`] that both JS and TS plugins expose.
///
/// `name` is the level name (pass `"files"` — kept as a parameter so tests can
/// verify the returned value without hard-coding a string twice). `cfg` is the
/// caller's merged config (`<lang>.toml` over `defaults.toml`); the `uses`
/// edge-kind vocabulary is read from its `[edge_kinds]` (both JS and TS inherit
/// the shared `uses` from `defaults.toml`).
pub fn ecmascript_level(name: &str, cfg: &toml::Table) -> Level {
    let edge_kinds = crate::config::edge_kinds(cfg);

    // Structural node-attribute display specs are DATA: js/ts inherit the shared
    // `path`/`loc`/`visibility`/`external` from `defaults.toml`, read via config.
    let node_attributes = crate::config::node_attributes(cfg);

    Level {
        name: name.to_string(),
        edge_kinds,
        node_attributes,
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds: default_node_kinds(),
        cycle_kinds: default_cycle_kinds(),
        grouping: None,
    }
}

/// Apply the shared ECMAScript `[specs.<key>]` description overrides (from
/// `ecmascript/config.toml`) over the central builtin metric specs — used by both
/// the JS and TS plugins, since they share the same `[halstead]` operator/operand
/// vocabulary, so the exact-tokens descriptions live in one place.
pub fn ecmascript_metric_specs(
    defaults: BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
) -> BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec> {
    crate::config::apply_spec_overrides(defaults, &cfg::CONFIG)
}

/// Measure ECMAScript complexity metrics for every `file` node, shared by the
/// JavaScript and TypeScript plugins, returning a [`MetricInputs`] keyed by file
/// node id (the orchestrator writes them). For each file the caller's
/// `engine_for_ext` maps the file's extension to the tree-sitter `Language` and
/// the `else_if_via_else_clause` flag (true for TypeScript, false for JS/TSX);
/// files whose extension maps to `None`, or that cannot be read/parsed, are skipped.
pub fn ecmascript_metrics(
    graph: &Graph,
    engine_for_ext: impl Fn(&str) -> Option<(tree_sitter::Language, bool)>,
) -> Vec<(String, MetricInputs)> {
    let mut out = Vec::new();
    for node in &graph.nodes {
        if node.kind != code_ranker_plugin_api::node::FILE {
            continue;
        }
        let ext = Path::new(&node.id)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let Some((lang, else_if_via_else_clause)) = engine_for_ext(ext) else {
            continue;
        };
        let Ok(src) = std::fs::read(&node.id) else {
            continue;
        };
        if let Some(m) = dialect::compute(&src, &lang, else_if_via_else_clause) {
            out.push((node.id.clone(), m));
        }
    }
    out
}

/// The optional `functions` [`Level`] both JS and TS plugins expose (off by
/// default; emitted only when `[levels] functions` is on). Declares per-language
/// unit kinds; metric attribute specs are merged centrally by the orchestrator.
///
/// `cfg` is the caller's merged config. The shared `function` / `method` kinds
/// come from its `[node_kinds]` (inherited from `defaults.toml`); the
/// ECMAScript-only `arrow` / `generator` kinds are read from THIS shared module's
/// own `ecmascript/config.toml` `[node_kinds]` (via [`cfg::CONFIG`]) — kept there,
/// the single home for ECMAScript vocab, so they are not copy-pasted into both
/// `javascript/config.toml` and `typescript/config.toml`, nor leaked into the
/// non-ECMAScript languages' configs via `defaults.toml`.
pub fn ecmascript_functions_level(cfg: &toml::Table) -> Level {
    let mut node_kinds = crate::config::node_kinds(cfg);
    // arrow / generator are ECMAScript-only; pull just those from the shared
    // ecmascript config (its [node_kinds] also re-lists the inherited
    // function/method, which simply overwrite identical entries — a no-op).
    node_kinds.extend(crate::config::node_kinds(&cfg::CONFIG));
    Level {
        name: "functions".to_string(),
        edge_kinds: BTreeMap::new(),
        node_attributes: BTreeMap::new(),
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds,
        cycle_kinds: default_cycle_kinds(),
        grouping: None,
    }
}

/// Function-level metric units for every file node (one per function-like unit),
/// for the optional `functions` level. Mirrors [`ecmascript_metrics`]: the grammar
/// is injected per extension. Each pair is the unit's node (`parent` = file id, id
/// `<file>#<name>@<start_line>`, **no metrics yet**) plus its measured inputs (the
/// orchestrator writes them).
pub fn ecmascript_function_units(
    graph: &Graph,
    engine_for_ext: impl Fn(&str) -> Option<(tree_sitter::Language, bool)>,
) -> Vec<(Node, MetricInputs)> {
    let mut out = Vec::new();
    for node in &graph.nodes {
        if node.kind != code_ranker_plugin_api::node::FILE {
            continue;
        }
        let ext = Path::new(&node.id)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let Some((lang, else_if)) = engine_for_ext(ext) else {
            continue;
        };
        let Ok(src) = std::fs::read(&node.id) else {
            continue;
        };
        for u in dialect::compute_functions(&src, &lang, else_if) {
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
