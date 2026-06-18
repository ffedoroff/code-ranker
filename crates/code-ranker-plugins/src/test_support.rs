//! Test-only helpers shared by the language-plugin tests. Compiled only under
//! `cfg(test)`, so it ships nothing. Keeps the per-language tests free of
//! duplicated `write_file` / graph-assertion boilerplate.

use code_ranker_plugin_api::graph::Graph;
use std::path::Path;

/// All-on file-walk ignore config for structure tests. Their fixtures are
/// throwaway temp dirs with no `.gitignore` / `.ignore` / dotfiles, so the flags
/// are inert — this just satisfies the `analyze` signature.
pub(crate) const IGNORE_ALL: crate::config::IgnoreCfg = crate::config::IgnoreCfg {
    gitignore: true,
    ignore_files: true,
    hidden: true,
};

/// Write `contents` to `dir/rel`, creating any missing parent directories.
/// Panics on I/O error — these run against a throwaway temp dir in tests.
pub(crate) fn write_file(dir: &Path, rel: &str, contents: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, contents).unwrap();
}

/// True iff the graph contains a node with this id.
pub(crate) fn has_node(g: &Graph, id: &str) -> bool {
    g.nodes.iter().any(|n| n.id == id)
}

/// Number of edges leaving `source` of the given kind.
pub(crate) fn edge_count_from(g: &Graph, source: &str, kind: &str) -> usize {
    g.edges
        .iter()
        .filter(|e| e.source == source && e.kind == kind)
        .count()
}
