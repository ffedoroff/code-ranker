//! Internal test helpers shared by the language-plugin crates.
//!
//! Not published (`publish = false`); pulled in only as a path dev-dependency,
//! so it is stripped from the published plugin manifests. Keeps the per-language
//! plugin tests free of the duplicated `write_file` / graph-assertion boilerplate
//! that each one previously redeclared.

use code_ranker_plugin_api::graph::Graph;
use std::path::Path;

/// Write `contents` to `dir/rel`, creating any missing parent directories.
/// Panics on I/O error — these run against a throwaway temp dir in tests.
pub fn write_file(dir: &Path, rel: &str, contents: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, contents).unwrap();
}

/// True iff the graph contains a node with this id.
pub fn has_node(g: &Graph, id: &str) -> bool {
    g.nodes.iter().any(|n| n.id == id)
}

/// True iff the graph contains an edge `source → target` of the given kind.
pub fn has_edge(g: &Graph, source: &str, target: &str, kind: &str) -> bool {
    g.edges
        .iter()
        .any(|e| e.source == source && e.target == target && e.kind == kind)
}

/// Number of edges leaving `source` of the given kind.
pub fn edge_count_from(g: &Graph, source: &str, kind: &str) -> usize {
    g.edges
        .iter()
        .filter(|e| e.source == source && e.kind == kind)
        .count()
}
