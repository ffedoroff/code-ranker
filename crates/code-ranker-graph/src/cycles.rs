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
mod tests {
    use super::*;
    use code_ranker_plugin_api::{edge::Edge, node::Node};

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
            line: None,
            attrs: Default::default(),
        }
    }
    fn flow() -> HashSet<String> {
        HashSet::from(["uses".to_string(), "reexports".to_string()])
    }
    fn node_crate(id: &str, name: &str, krate: &str) -> Node {
        let mut n = node(id, name);
        n.attrs.insert("crate".into(), AttrValue::Str(krate.into()));
        n
    }

    #[test]
    fn cross_crate_via_attr_is_dropped() {
        // deno-style ids (no `/src/`): crate identity comes from the attribute.
        let mut g = Graph {
            nodes: vec![
                node_crate("{t}/cli/a.rs", "a", "deno"),
                node_crate("{t}/runtime/b.rs", "b", "deno_runtime"),
            ],
            edges: vec![
                edge("{t}/cli/a.rs", "{t}/runtime/b.rs", "uses"),
                edge("{t}/runtime/b.rs", "{t}/cli/a.rs", "uses"),
            ],
        };
        assert!(annotate_cycles(&mut g, &flow()).is_empty());
    }

    #[test]
    fn same_crate_via_attr_is_kept() {
        // Same crate attr despite different subdirs and no `/src/` in the ids.
        let mut g = Graph {
            nodes: vec![
                node_crate("{t}/cli/a.rs", "a", "deno"),
                node_crate("{t}/cli/sub/b.rs", "b", "deno"),
            ],
            edges: vec![
                edge("{t}/cli/a.rs", "{t}/cli/sub/b.rs", "uses"),
                edge("{t}/cli/sub/b.rs", "{t}/cli/a.rs", "uses"),
            ],
        };
        assert_eq!(annotate_cycles(&mut g, &flow()).len(), 1);
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
    fn cross_crate_scc_is_dropped() {
        // A 2-cycle whose files live in different crates is impossible in Rust.
        let mut g = Graph {
            nodes: vec![
                node("{t}/crateA/src/a.rs", "a"),
                node("{t}/crateB/src/b.rs", "b"),
            ],
            edges: vec![
                edge("{t}/crateA/src/a.rs", "{t}/crateB/src/b.rs", "uses"),
                edge("{t}/crateB/src/b.rs", "{t}/crateA/src/a.rs", "uses"),
            ],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert!(groups.is_empty(), "cross-crate cycle must be dropped");
    }

    #[test]
    fn intra_crate_scc_is_kept() {
        let mut g = Graph {
            nodes: vec![
                node("{t}/crateA/src/a.rs", "a"),
                node("{t}/crateA/src/b.rs", "b"),
            ],
            edges: vec![
                edge("{t}/crateA/src/a.rs", "{t}/crateA/src/b.rs", "uses"),
                edge("{t}/crateA/src/b.rs", "{t}/crateA/src/a.rs", "uses"),
            ],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, "mutual");
    }

    #[test]
    fn three_node_scc_is_chain() {
        let mut g = Graph {
            nodes: vec![node("a", "a"), node("b", "b"), node("c", "c")],
            edges: vec![
                edge("a", "b", "uses"),
                edge("b", "c", "uses"),
                edge("c", "a", "uses"),
            ],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, "chain");
    }

    #[test]
    fn test_named_node_no_longer_special_cased() {
        // A test-named file in an SCC is classified purely by size now (`mutual`),
        // not the removed `test_embed` kind.
        let mut g = Graph {
            nodes: vec![node("a", "a"), node("b", "foo_tests")],
            edges: vec![edge("a", "b", "uses"), edge("b", "a", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups[0].kind, "mutual");
    }

    #[test]
    fn self_loop_does_not_form_cycle() {
        // A node that depends on itself (`a → a`) is not a cycle: the `fi != ti`
        // guard drops the edge before SCC detection, so no group and no `cycle`
        // attribute. (A real SCC needs ≥ 2 distinct members.)
        let mut g = Graph {
            nodes: vec![node("a", "a")],
            edges: vec![edge("a", "a", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert!(groups.is_empty(), "a self-loop is not a cycle");
        assert!(
            g.nodes[0].attrs.get("cycle").is_none(),
            "no cycle attribute on a self-looping node"
        );
    }

    #[test]
    fn cycle_among_disconnected_components_is_isolated() {
        // A 2-cycle (a ↔ b) sitting in a graph with three edge-less nodes: only
        // the cycle members are annotated; the disconnected nodes are untouched.
        let mut g = Graph {
            nodes: vec![
                node("a", "a"),
                node("b", "b"),
                node("x", "x"),
                node("y", "y"),
                node("z", "z"),
            ],
            edges: vec![edge("a", "b", "uses"), edge("b", "a", "uses")],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups.len(), 1, "exactly one cycle group");
        assert_eq!(groups[0].kind, "mutual");
        for n in &g.nodes {
            let in_cycle = n.id == "a" || n.id == "b";
            assert_eq!(
                n.attrs.get("cycle").is_some(),
                in_cycle,
                "node {} cycle-annotation should be {in_cycle}",
                n.id
            );
        }
    }

    #[test]
    fn nested_cycles_form_single_scc() {
        // A 2-cycle (a ↔ b) where b also sits in a 3-cycle (b → c → d → b): all
        // four nodes are mutually reachable, so Kosaraju collapses them into ONE
        // SCC of size 4 — nested cycles are not split into separate groups.
        let mut g = Graph {
            nodes: vec![
                node("a", "a"),
                node("b", "b"),
                node("c", "c"),
                node("d", "d"),
            ],
            edges: vec![
                edge("a", "b", "uses"),
                edge("b", "a", "uses"),
                edge("b", "c", "uses"),
                edge("c", "d", "uses"),
                edge("d", "b", "uses"),
            ],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(groups.len(), 1, "one merged SCC, not two cycles");
        assert_eq!(groups[0].kind, "chain", "4-member SCC is a chain");
        assert_eq!(groups[0].nodes.len(), 4, "all four nodes in the one group");
    }

    #[test]
    fn unknown_crate_member_keeps_cycle_conservatively() {
        // A 2-cycle where one member has neither a `crate` attribute nor a `/src/`
        // segment in its id, so `crate_of` returns None. `spans_multiple_crates`
        // must then stay conservative (return false) and KEEP the cycle, rather
        // than mis-dropping it as cross-crate.
        let mut g = Graph {
            nodes: vec![
                node_crate("{t}/cli/a.rs", "a", "deno"),
                node("opaque-id-without-src", "b"),
            ],
            edges: vec![
                edge("{t}/cli/a.rs", "opaque-id-without-src", "uses"),
                edge("opaque-id-without-src", "{t}/cli/a.rs", "uses"),
            ],
        };
        let groups = annotate_cycles(&mut g, &flow());
        assert_eq!(
            groups.len(),
            1,
            "an undeterminable crate must not drop the cycle"
        );
        assert_eq!(groups[0].kind, "mutual");
    }

    #[test]
    fn crate_of_falls_back_to_src_segment() {
        // With no `crate` attribute, crate identity is derived from the id as
        // everything before the last `/src/`. Two files under DIFFERENT such
        // prefixes form a cross-crate SCC that must be dropped — proving the
        // `/src/` fallback path of `crate_of`, distinct from the attribute path.
        let mut g = Graph {
            nodes: vec![
                node("{t}/alpha/src/a.rs", "a"),
                node("{t}/beta/src/b.rs", "b"),
            ],
            edges: vec![
                edge("{t}/alpha/src/a.rs", "{t}/beta/src/b.rs", "uses"),
                edge("{t}/beta/src/b.rs", "{t}/alpha/src/a.rs", "uses"),
            ],
        };
        assert!(
            annotate_cycles(&mut g, &flow()).is_empty(),
            "different /src/ prefixes are different crates → cycle dropped"
        );

        // Same `/src/` prefix → same crate → cycle kept (the fallback also keeps
        // genuine intra-crate cycles, not only drops cross-crate ones).
        let mut g2 = Graph {
            nodes: vec![
                node("{t}/alpha/src/a.rs", "a"),
                node("{t}/alpha/src/b.rs", "b"),
            ],
            edges: vec![
                edge("{t}/alpha/src/a.rs", "{t}/alpha/src/b.rs", "uses"),
                edge("{t}/alpha/src/b.rs", "{t}/alpha/src/a.rs", "uses"),
            ],
        };
        assert_eq!(
            annotate_cycles(&mut g2, &flow()).len(),
            1,
            "same /src/ prefix is one crate → cycle kept"
        );
    }
}
