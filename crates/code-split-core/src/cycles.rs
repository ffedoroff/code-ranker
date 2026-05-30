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
