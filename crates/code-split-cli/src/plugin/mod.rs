pub mod javascript;
pub mod python;
pub mod rust;

use anyhow::{Result, bail};
use code_split_core::{Graph, NodeKind, PluginGraphs, StageTime};
use std::collections::HashSet;
use std::path::Path;

/// Run a built-in plugin for the given workspace. Returns `(graphs, timings)`.
///
/// All plugins are compiled into the binary and run in-process — there is no
/// external/dynamic plugin loading.
pub fn run(
    name: &str,
    workspace: &Path,
    local_only: bool,
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    match name {
        "rust" => rust::run(workspace, local_only),
        "python" => python::run(workspace, local_only),
        "javascript" | "typescript" | "js" | "ts" => javascript::run(workspace, local_only),
        other => bail!("unknown plugin {other:?}; built-in plugins are: rust, python, javascript"),
    }
}

/// Normalize a directly-built file graph (Python / JS plugins): drop any
/// `Contains` edges and self-loops, set the `external` flag on every edge whose
/// target is an `External` library node, deduplicate edges on `(from, to,
/// kind)`, prune unreferenced `External` nodes, and sort for deterministic
/// output.
pub fn finalize_file_graph(mut graph: Graph) -> Graph {
    use code_split_core::EdgeKind;

    let external_ids: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::External || n.external.unwrap_or(false))
        .map(|n| n.id.clone())
        .collect();

    let mut seen: HashSet<(String, String, EdgeKind)> = HashSet::new();
    let mut edges = Vec::new();
    for mut e in std::mem::take(&mut graph.edges) {
        if e.kind == EdgeKind::Contains || e.from == e.to {
            continue;
        }
        e.external = external_ids.contains(&e.to).then_some(true);
        if seen.insert((e.from.clone(), e.to.clone(), e.kind)) {
            edges.push(e);
        }
    }

    // Keep only external nodes that are actually referenced (depth-1 deps).
    let referenced_ext: HashSet<&str> = edges
        .iter()
        .filter(|e| e.external.unwrap_or(false))
        .map(|e| e.to.as_str())
        .collect();
    graph.nodes.retain(|n| {
        !(n.kind == NodeKind::External || n.external.unwrap_or(false))
            || referenced_ext.contains(n.id.as_str())
    });

    graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
            .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
    });
    graph.edges = edges;
    graph
}
