//! Cycle detection over information-flow edges (Kosaraju SCC). Edges count iff
//! their kind is in `flow_kinds` (derived from `EdgeKindSpec.flow`); structural
//! kinds like `contains` are excluded, so a `mod foo;` parent/child pair is not
//! flagged as a false cycle.

use crate::CycleGroup;
use code_split_plugin_api::{AttrValue, Graph};
use std::collections::HashMap;
use std::collections::HashSet;

/// Detect SCCs (≥ 2 members) over flow edges, write a `cycle` attribute on each
/// participating node (`"mutual"` / `"chain"` / `"test_embed"`), and return the
/// cycle groups.
pub fn annotate_cycles(graph: &mut Graph, flow_kinds: &HashSet<String>) -> Vec<CycleGroup> {
    let n = graph.nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();

    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in &graph.edges {
        if !flow_kinds.contains(&edge.kind) {
            continue;
        }
        if let (Some(&fi), Some(&ti)) = (
            id_to_idx.get(edge.source.as_str()),
            id_to_idx.get(edge.target.as_str()),
        ) && fi != ti
        {
            adj[fi].push(ti);
        }
    }

    let sccs = kosaraju_sccs(n, &adj);

    let mut node_kind: Vec<Option<&'static str>> = vec![None; n];
    let mut groups: Vec<CycleGroup> = Vec::new();
    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }
        let kind = classify_scc(scc, graph);
        for &idx in scc {
            node_kind[idx] = Some(kind);
        }
        groups.push(CycleGroup {
            kind: kind.to_string(),
            nodes: scc.iter().map(|&i| graph.nodes[i].id.clone()).collect(),
        });
    }

    for (i, node) in graph.nodes.iter_mut().enumerate() {
        match node_kind[i] {
            Some(k) => {
                node.attrs
                    .insert("cycle".to_string(), AttrValue::Str(k.to_string()));
            }
            None => {
                node.attrs.remove("cycle");
            }
        }
    }
    groups
}

fn classify_scc(scc: &[usize], graph: &Graph) -> &'static str {
    if scc.iter().any(|&i| is_test_node(graph, i)) {
        return "test_embed";
    }
    if scc.len() == 2 { "mutual" } else { "chain" }
}

fn is_test_node(graph: &Graph, idx: usize) -> bool {
    let node = &graph.nodes[idx];
    let mut name = node.name.to_ascii_lowercase();
    for ext in [".rs", ".py", ".ts", ".tsx", ".js", ".jsx"] {
        if let Some(stem) = name.strip_suffix(ext) {
            name = stem.to_string();
            break;
        }
    }
    if matches!(name.as_str(), "tests" | "test" | "benches" | "bench") {
        return true;
    }
    if name.ends_with("_tests")
        || name.ends_with("_test")
        || name.ends_with("_bench")
        || name.starts_with("test_")
    {
        return true;
    }
    let id = &node.id;
    id.contains("::tests")
        || id.contains("::test::")
        || id.ends_with("::test")
        || id.contains("/tests/")
        || id.contains("/__tests__/")
}

// ── Kosaraju's SCC (iterative, O(V+E)) ─────────────────────────────────────

fn kosaraju_sccs(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut visited = vec![false; n];
    let mut finish_order = Vec::with_capacity(n);
    for i in 0..n {
        if !visited[i] {
            dfs_finish(i, adj, &mut visited, &mut finish_order);
        }
    }
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, neighbors) in adj.iter().enumerate() {
        for &v in neighbors {
            radj[v].push(u);
        }
    }
    let mut visited2 = vec![false; n];
    let mut sccs: Vec<Vec<usize>> = Vec::new();
    for &start in finish_order.iter().rev() {
        if !visited2[start] {
            let mut scc = Vec::new();
            dfs_collect(start, &radj, &mut visited2, &mut scc);
            sccs.push(scc);
        }
    }
    sccs
}

fn dfs_finish(start: usize, adj: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    visited[start] = true;
    while let Some((u, ni)) = stack.last_mut() {
        let u = *u;
        if *ni < adj[u].len() {
            let v = adj[u][*ni];
            *ni += 1;
            if !visited[v] {
                visited[v] = true;
                stack.push((v, 0));
            }
        } else {
            stack.pop();
            order.push(u);
        }
    }
}

fn dfs_collect(start: usize, adj: &[Vec<usize>], visited: &mut [bool], scc: &mut Vec<usize>) {
    let mut stack = vec![start];
    visited[start] = true;
    while let Some(u) = stack.pop() {
        scc.push(u);
        for &v in &adj[u] {
            if !visited[v] {
                visited[v] = true;
                stack.push(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_split_plugin_api::{Edge, Node};

    fn node(id: &str, name: &str) -> Node {
        Node {
            id: id.into(),
            kind: "file".into(),
            name: name.into(),
            parent: None,
            attrs: Default::default(),
        }
    }
    fn edge(from: &str, to: &str, kind: &str) -> Edge {
        Edge {
            source: from.into(),
            target: to.into(),
            kind: kind.into(),
            attrs: Default::default(),
        }
    }
    fn flow() -> HashSet<String> {
        HashSet::from(["uses".to_string(), "reexports".to_string()])
    }

    #[test]
    fn two_node_cycle_is_mutual() {
        let mut g = Graph {
            nodes: vec![node("a", "a"), node("b", "b")],
            edges: vec![edge("a", "b", "uses"), edge("b", "a", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, "mutual");
        assert_eq!(
            g.nodes[0].attrs.get("cycle"),
            Some(&AttrValue::Str("mutual".into()))
        );
    }

    #[test]
    fn contains_edge_excluded_from_cycles() {
        let mut g = Graph {
            nodes: vec![node("m", "m"), node("c", "c")],
            edges: vec![edge("m", "c", "contains"), edge("c", "m", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert!(groups.is_empty(), "contains is structural, not flow");
    }

    #[test]
    fn test_node_makes_test_embed() {
        let mut g = Graph {
            nodes: vec![node("a", "a"), node("b", "foo_tests")],
            edges: vec![edge("a", "b", "uses"), edge("b", "a", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups[0].kind, "test_embed");
    }
}
