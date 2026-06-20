//! Cycle detection over information-flow edges (Kosaraju SCC). Edges count iff
//! their kind is in `flow_kinds` (derived from `EdgeKindSpec.flow`); structural
//! kinds like `contains` are excluded, so a `mod foo;` parent/child pair is not
//! flagged as a false cycle.

use crate::level_graph::CycleGroup;
use code_ranker_plugin_api::{attrs::AttrValue, graph::Graph};
use std::collections::HashMap;
use std::collections::HashSet;

/// Detect SCCs (≥ 2 members) over flow edges, write a `cycle` attribute on each
/// participating node (`"mutual"` for a 2-node SCC, `"chain"` for 3+), and
/// return the cycle groups.
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
        // Rust forbids circular dependencies between crates, so an SCC whose
        // members span more than one crate cannot be a real cycle — it is an
        // artifact of imprecise path resolution. Drop it.
        if spans_multiple_crates(scc, graph) {
            continue;
        }
        let kind = classify_scc(scc);
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

/// The crate a node belongs to. Prefers the plugin-provided `crate` attribute
/// (the precise per-target compilation unit from `cargo metadata`); falls back
/// to deriving it from the id as everything before the last `/src/` segment for
/// nodes/plugins that don't set it. Returns `None` when neither is available, so
/// callers can stay conservative.
fn crate_of(node: &code_ranker_plugin_api::node::Node) -> Option<&str> {
    if let Some(AttrValue::Str(c)) = node.attrs.get("crate") {
        return Some(c.as_str());
    }
    node.id.rfind("/src/").map(|i| &node.id[..i])
}

/// True only when every member has a determinable crate and at least two crates
/// are present. Unknown-crate nodes make this `false` (conservative: keep the
/// cycle) so non-crate id schemes (tests, other plugins) are never mis-dropped.
fn spans_multiple_crates(scc: &[usize], graph: &Graph) -> bool {
    let mut crates = Vec::with_capacity(scc.len());
    for &i in scc {
        match crate_of(&graph.nodes[i]) {
            Some(c) => crates.push(c),
            None => return false,
        }
    }
    crates.iter().any(|c| *c != crates[0])
}

fn classify_scc(scc: &[usize]) -> &'static str {
    if scc.len() == 2 { "mutual" } else { "chain" }
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
#[path = "cycles_test.rs"]
mod tests;
