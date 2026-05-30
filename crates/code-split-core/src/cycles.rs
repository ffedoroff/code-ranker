use crate::graph::{CycleGroup, CycleKind, Graph};
use crate::snapshot::PluginGraphs;
use std::collections::HashMap;

/// Detect SCCs in every projected graph and annotate nodes + the graph's
/// `cycles` field in-place.
pub fn annotate_all_cycles(graphs: &mut PluginGraphs) {
    annotate_graph_cycles(&mut graphs.modules);
    annotate_graph_cycles(&mut graphs.files);
    annotate_graph_cycles(&mut graphs.functions);
}

fn annotate_graph_cycles(graph: &mut Graph) {
    let n = graph.nodes.len();
    if n == 0 {
        return;
    }

    // Build index map NodeId → usize.
    let id_to_idx: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();

    // Adjacency list over ALL edge kinds (contains, uses, reexports, calls).
    // We deliberately include `contains` edges so that the Rust-specific
    // test-embed pattern (parent --contains--> tests --uses--> parent) is
    // visible as a cycle and can be classified correctly.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in &graph.edges {
        if let (Some(&fi), Some(&ti)) = (
            id_to_idx.get(edge.from.as_str()),
            id_to_idx.get(edge.to.as_str()),
        ) && fi != ti
        {
            adj[fi].push(ti);
        }
    }

    let sccs = kosaraju_sccs(n, &adj);

    let mut node_kind: Vec<Option<CycleKind>> = vec![None; n];
    let mut cycle_groups: Vec<CycleGroup> = Vec::new();

    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }
        let kind = classify_scc(scc, graph);
        for &idx in scc {
            node_kind[idx] = Some(kind);
        }
        cycle_groups.push(CycleGroup {
            kind,
            nodes: scc.iter().map(|&i| graph.nodes[i].id.clone()).collect(),
        });
    }

    for (i, node) in graph.nodes.iter_mut().enumerate() {
        node.cycle_kind = node_kind[i];
    }
    graph.cycles = cycle_groups;
}

fn classify_scc(scc: &[usize], graph: &Graph) -> CycleKind {
    if scc.iter().any(|&i| is_test_node(graph, i)) {
        return CycleKind::TestEmbed;
    }
    if scc.len() == 2 {
        CycleKind::Mutual
    } else {
        CycleKind::Chain
    }
}

fn is_test_node(graph: &Graph, idx: usize) -> bool {
    let node = &graph.nodes[idx];
    let name = node.name.to_ascii_lowercase();
    // Common Rust test module names
    if matches!(name.as_str(), "tests" | "test" | "benches" | "bench") {
        return true;
    }
    if name.ends_with("_tests") || name.ends_with("_test") || name.ends_with("_bench") {
        return true;
    }
    // ID path segments — catches `::tests`, `::test::`, etc.
    let id = &node.id;
    id.contains("::tests") || id.contains("::test::") || id.ends_with("::test")
}

// ---------------------------------------------------------------------------
// Kosaraju's SCC (iterative, O(V+E))
// ---------------------------------------------------------------------------

fn kosaraju_sccs(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    // Pass 1: DFS on the original graph, collect finish order.
    let mut visited = vec![false; n];
    let mut finish_order = Vec::with_capacity(n);
    for i in 0..n {
        if !visited[i] {
            dfs_finish(i, adj, &mut visited, &mut finish_order);
        }
    }

    // Build transposed adjacency list.
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, neighbors) in adj.iter().enumerate() {
        for &v in neighbors {
            radj[v].push(u);
        }
    }

    // Pass 2: DFS on the transposed graph in reverse finish order.
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
    // Iterative DFS; (node, next_neighbor_index) on the call stack.
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
    use crate::graph::{Edge, EdgeKind, Node, NodeKind};

    fn node(id: &str, name: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            kind,
            name: name.into(),
            path: String::new(),
            parent: None,
            external: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        }
    }

    /// A plain module node whose `id` doubles as its `name` (the common,
    /// non-test case).
    fn mod_node(id: &str) -> Node {
        node(id, id, NodeKind::Module)
    }

    fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind,
            unresolved: None,
            external: None,
            visibility: None,
        }
    }

    fn graph_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph {
            nodes,
            edges,
            cycles: Vec::new(),
            stats: None,
        }
    }

    fn kind_of(g: &Graph, id: &str) -> Option<CycleKind> {
        g.nodes.iter().find(|n| n.id == id).unwrap().cycle_kind
    }

    #[test]
    fn dag_has_no_cycles() {
        // a → b → c with no back edge: no SCC of size ≥ 2.
        let mut g = graph_of(
            vec![mod_node("a"), mod_node("b"), mod_node("c")],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "c", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert!(g.cycles.is_empty(), "a DAG has no cycle groups");
        assert!(
            g.nodes.iter().all(|n| n.cycle_kind.is_none()),
            "no node in a DAG is annotated"
        );
    }

    #[test]
    fn two_node_cycle_is_mutual() {
        // a ⇄ b, no test node → Mutual.
        let mut g = graph_of(
            vec![mod_node("a"), mod_node("b")],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "a", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles.len(), 1, "one cycle group");
        assert_eq!(g.cycles[0].kind, CycleKind::Mutual);
        assert_eq!(g.cycles[0].nodes.len(), 2);
        assert_eq!(kind_of(&g, "a"), Some(CycleKind::Mutual));
        assert_eq!(kind_of(&g, "b"), Some(CycleKind::Mutual));
    }

    #[test]
    fn three_node_cycle_is_chain() {
        // a → b → c → a, no test node → Chain.
        let mut g = graph_of(
            vec![mod_node("a"), mod_node("b"), mod_node("c")],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "c", EdgeKind::Uses),
                edge("c", "a", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles.len(), 1);
        assert_eq!(g.cycles[0].kind, CycleKind::Chain);
        assert_eq!(g.cycles[0].nodes.len(), 3);
        for id in ["a", "b", "c"] {
            assert_eq!(kind_of(&g, id), Some(CycleKind::Chain), "node {id}");
        }
    }

    #[test]
    fn test_module_back_edge_is_test_embed() {
        // parent --contains--> parent::tests --uses--> parent. A test node in
        // the SCC wins over the size-based (Mutual) classification.
        let mut g = graph_of(
            vec![
                node("m", "m", NodeKind::Module),
                node("m::tests", "tests", NodeKind::Module),
            ],
            vec![
                edge("m", "m::tests", EdgeKind::Contains),
                edge("m::tests", "m", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles.len(), 1);
        assert_eq!(
            g.cycles[0].kind,
            CycleKind::TestEmbed,
            "a 2-node SCC containing a test module is TestEmbed, not Mutual"
        );
    }

    #[test]
    fn test_node_detected_by_name_suffix_overrides_chain() {
        // 3-node cycle that would be Chain, but one node's name ends in
        // `_tests` → TestEmbed.
        let mut g = graph_of(
            vec![
                node("a", "a", NodeKind::Module),
                node("b", "b", NodeKind::Module),
                node("c", "foo_tests", NodeKind::Module),
            ],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "c", EdgeKind::Uses),
                edge("c", "a", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles[0].kind, CycleKind::TestEmbed);
    }

    #[test]
    fn self_loop_is_not_a_cycle() {
        // a → a is dropped (fi == ti), so the SCC stays size 1.
        let mut g = graph_of(vec![mod_node("a")], vec![edge("a", "a", EdgeKind::Calls)]);
        annotate_graph_cycles(&mut g);
        assert!(g.cycles.is_empty(), "a self-loop is not a structural cycle");
        assert_eq!(kind_of(&g, "a"), None);
    }

    #[test]
    fn node_outside_the_cycle_stays_unannotated() {
        // a ⇄ b is a cycle; d hangs off a but is in no SCC of size ≥ 2.
        let mut g = graph_of(
            vec![mod_node("a"), mod_node("b"), mod_node("d")],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "a", EdgeKind::Uses),
                edge("a", "d", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles.len(), 1, "only the a⇄b SCC is a cycle");
        assert_eq!(kind_of(&g, "a"), Some(CycleKind::Mutual));
        assert_eq!(kind_of(&g, "b"), Some(CycleKind::Mutual));
        assert_eq!(
            kind_of(&g, "d"),
            None,
            "the dangling node is not part of a cycle"
        );
    }

    #[test]
    fn disjoint_cycles_get_independent_groups() {
        // a ⇄ b (Mutual) and c → d → e → c (Chain): two separate SCCs.
        let mut g = graph_of(
            vec![
                mod_node("a"),
                mod_node("b"),
                mod_node("c"),
                mod_node("d"),
                mod_node("e"),
            ],
            vec![
                edge("a", "b", EdgeKind::Uses),
                edge("b", "a", EdgeKind::Uses),
                edge("c", "d", EdgeKind::Uses),
                edge("d", "e", EdgeKind::Uses),
                edge("e", "c", EdgeKind::Uses),
            ],
        );
        annotate_graph_cycles(&mut g);
        assert_eq!(g.cycles.len(), 2, "two independent cycle groups");
        let kinds: Vec<CycleKind> = g.cycles.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&CycleKind::Mutual), "got {kinds:?}");
        assert!(kinds.contains(&CycleKind::Chain), "got {kinds:?}");
        assert_eq!(kind_of(&g, "a"), Some(CycleKind::Mutual));
        assert_eq!(kind_of(&g, "d"), Some(CycleKind::Chain));
    }

    #[test]
    fn empty_graph_is_a_noop() {
        let mut g = Graph::new();
        annotate_graph_cycles(&mut g);
        assert!(g.cycles.is_empty());
    }

    #[test]
    fn annotate_all_cycles_covers_every_projected_graph() {
        // A cycle in `modules` and a separate cycle in `functions`; `files`
        // stays empty. All three projections are dispatched.
        let mut graphs = PluginGraphs {
            modules: graph_of(
                vec![mod_node("a"), mod_node("b")],
                vec![
                    edge("a", "b", EdgeKind::Uses),
                    edge("b", "a", EdgeKind::Uses),
                ],
            ),
            files: Graph::new(),
            functions: graph_of(
                vec![
                    node("f", "f", NodeKind::Fn),
                    node("g", "g", NodeKind::Fn),
                    node("h", "h", NodeKind::Fn),
                ],
                vec![
                    edge("f", "g", EdgeKind::Calls),
                    edge("g", "h", EdgeKind::Calls),
                    edge("h", "f", EdgeKind::Calls),
                ],
            ),
        };
        annotate_all_cycles(&mut graphs);
        assert_eq!(graphs.modules.cycles.len(), 1);
        assert_eq!(graphs.modules.cycles[0].kind, CycleKind::Mutual);
        assert!(graphs.files.cycles.is_empty(), "empty graph left untouched");
        assert_eq!(graphs.functions.cycles.len(), 1);
        assert_eq!(graphs.functions.cycles[0].kind, CycleKind::Chain);
    }
}
