//! Canonicalize a freshly-parsed structural graph: drop self-loops, deduplicate
//! edges on `(source, target, kind)`, and prune external nodes that nothing
//! references. Structural edges (e.g. `contains`) are preserved — they carry no
//! flow but are kept for display and ownership.

use code_ranker_plugin_api::graph::Graph;
use std::collections::HashSet;

use crate::attrs::is_external;

pub fn finalize_graph(graph: &mut Graph) {
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut edges = Vec::with_capacity(graph.edges.len());
    for e in std::mem::take(&mut graph.edges) {
        if e.source == e.target {
            continue;
        }
        if seen.insert((e.source.clone(), e.target.clone(), e.kind.clone())) {
            edges.push(e);
        }
    }

    // Keep external nodes only if some edge targets them.
    let referenced: HashSet<&str> = edges.iter().map(|e| e.target.as_str()).collect();
    graph
        .nodes
        .retain(|n| !is_external(n) || referenced.contains(n.id.as_str()));

    graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.kind.cmp(&b.kind))
    });
    graph.edges = edges;
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_ranker_plugin_api::{edge::Edge, node::Node};

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            source: from.into(),
            target: to.into(),
            kind: "uses".into(),
            line: None,
            attrs: Default::default(),
        }
    }

    #[test]
    fn dedups_and_drops_self_loops_and_unused_externals() {
        let mut g = Graph {
            nodes: vec![
                Node {
                    id: "a".into(),
                    kind: "file".into(),
                    name: "a".into(),
                    parent: None,
                    attrs: Default::default(),
                },
                Node {
                    id: "ext:unused".into(),
                    kind: "external".into(),
                    name: "unused".into(),
                    parent: None,
                    attrs: Default::default(),
                },
            ],
            edges: vec![edge("a", "b"), edge("a", "b"), edge("a", "a")],
        };
        finalize_graph(&mut g);
        assert_eq!(g.edges.len(), 1);
        assert!(g.nodes.iter().all(|n| n.id != "ext:unused"));
    }
}
