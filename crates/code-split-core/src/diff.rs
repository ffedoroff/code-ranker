// Import from the defining modules (not the crate-root re-exports) so this module
// does not depend "up" on the crate root, which would close a dependency cycle.
use crate::graph::Graph;
use crate::snapshot::Snapshot;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct DiffCounts {
    pub added: usize,
    pub removed: usize,
    pub affected: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelDiff {
    pub nodes: DiffCounts,
    pub edges: DiffCounts,
    /// Nodes participating in cycles (SCCs with ≥ 2 members).
    pub cycle_nodes_before: usize,
    pub cycle_nodes_after: usize,
    /// Number of SCCs.
    pub sccs_before: usize,
    pub sccs_after: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapMeta {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareSummary {
    pub schema_version: String,
    pub before: SnapMeta,
    pub after: SnapMeta,
    pub identical: bool,
    pub files: LevelDiff,
}

pub fn compare_snapshots(before: &Snapshot, after: &Snapshot) -> CompareSummary {
    let files = diff_graph(&before.graphs.files, &after.graphs.files);

    let identical = files.nodes.added == 0
        && files.nodes.removed == 0
        && files.nodes.affected == 0
        && files.edges.added == 0
        && files.edges.removed == 0
        && files.edges.affected == 0;

    CompareSummary {
        schema_version: "1".to_string(),
        before: snap_meta(before),
        after: snap_meta(after),
        identical,
        files,
    }
}

fn snap_meta(snap: &Snapshot) -> SnapMeta {
    let commit_short = snap
        .git
        .as_ref()
        .map(|g| g.commit[..8.min(g.commit.len())].to_string());
    SnapMeta {
        target: snap
            .target
            .split('/')
            .next_back()
            .unwrap_or(&snap.target)
            .to_string(),
        branch: snap.git.as_ref().map(|g| g.branch.clone()),
        commit: commit_short,
    }
}

// Mirrors computeDiff() from assets/diff.js.
fn diff_graph(before: &Graph, after: &Graph) -> LevelDiff {
    // Non-external node id sets.
    let bg: HashMap<String, ()> = before
        .nodes
        .iter()
        .filter(|n| !n.external.unwrap_or(false))
        .map(|n| (n.id.clone(), ()))
        .collect();
    let ag: HashMap<String, ()> = after
        .nodes
        .iter()
        .filter(|n| !n.external.unwrap_or(false))
        .map(|n| (n.id.clone(), ()))
        .collect();

    // Node status: 0 = unchanged, 1 = added, 2 = removed, 3 = affected.
    let mut node_status: HashMap<String, u8> = HashMap::new();
    for id in ag.keys() {
        node_status.insert(id.clone(), if bg.contains_key(id) { 0 } else { 1 });
    }
    for id in bg.keys() {
        if !ag.contains_key(id) {
            node_status.insert(id.clone(), 2);
        }
    }

    // Edge key: "from\0to\0Kind".
    let ekey = |e: &crate::Edge| format!("{}\x00{}\x00{:?}", e.from, e.to, e.kind);

    // Local edges: both endpoints present in node_status.
    let local_edges = |edges: &[crate::Edge]| -> HashMap<String, (String, String)> {
        edges
            .iter()
            .filter(|e| node_status.contains_key(&e.from) && node_status.contains_key(&e.to))
            .map(|e| (ekey(e), (e.from.clone(), e.to.clone())))
            .collect()
    };

    let bg_edges = local_edges(&before.edges);
    let ag_edges = local_edges(&after.edges);

    // Collect all edges with status.
    let mut edge_list: Vec<(String, String, u8)> = Vec::new();
    for (key, (from, to)) in &ag_edges {
        edge_list.push((
            from.clone(),
            to.clone(),
            if bg_edges.contains_key(key) { 0 } else { 1 },
        ));
    }
    for (key, (from, to)) in &bg_edges {
        if !ag_edges.contains_key(key) {
            edge_list.push((from.clone(), to.clone(), 2));
        }
    }

    // Propagate "affected" to unchanged nodes adjacent to changed edges.
    for (from, to, status) in &edge_list {
        if *status != 0 {
            if node_status.get(from.as_str()) == Some(&0) {
                node_status.insert(from.clone(), 3);
            }
            if node_status.get(to.as_str()) == Some(&0) {
                node_status.insert(to.clone(), 3);
            }
        }
    }

    // Count nodes.
    let mut nodes = DiffCounts {
        added: 0,
        removed: 0,
        affected: 0,
        unchanged: 0,
    };
    for &s in node_status.values() {
        match s {
            1 => nodes.added += 1,
            2 => nodes.removed += 1,
            3 => nodes.affected += 1,
            _ => nodes.unchanged += 1,
        }
    }

    // Count edges (unchanged edge connecting non-unchanged nodes → affected).
    let mut edges = DiffCounts {
        added: 0,
        removed: 0,
        affected: 0,
        unchanged: 0,
    };
    for (from, to, status) in &edge_list {
        let s = if *status == 0
            && (node_status.get(from.as_str()) != Some(&0)
                || node_status.get(to.as_str()) != Some(&0))
        {
            3u8
        } else {
            *status
        };
        match s {
            1 => edges.added += 1,
            2 => edges.removed += 1,
            3 => edges.affected += 1,
            _ => edges.unchanged += 1,
        }
    }

    LevelDiff {
        nodes,
        edges,
        cycle_nodes_before: before.cycles.iter().map(|c| c.nodes.len()).sum(),
        cycle_nodes_after: after.cycles.iter().map(|c| c.nodes.len()).sum(),
        sccs_before: before.cycles.len(),
        sccs_after: after.cycles.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CycleGroup, CycleKind, Edge, EdgeKind, Node, NodeKind};
    use crate::snapshot::{GitInfo, PluginGraphs};

    fn node(id: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Module,
            name: id.into(),
            path: String::new(),
            parent: None,
            external: None,
            version: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        }
    }

    fn ext_node(id: &str) -> Node {
        Node {
            external: Some(true),
            ..node(id)
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Uses,
            unresolved: None,
            external: None,
            visibility: None,
        }
    }

    fn graph(nodes: Vec<Node>, edges: Vec<Edge>) -> Graph {
        Graph {
            nodes,
            edges,
            cycles: Vec::new(),
            stats: None,
        }
    }

    // ── diff_graph: node / edge / affected / external / cycle counts ─────────

    #[test]
    fn identical_graphs_have_only_unchanged() {
        let g = graph(vec![node("a"), node("b")], vec![edge("a", "b")]);
        let d = diff_graph(&g, &g);
        assert_eq!(d.nodes.unchanged, 2);
        assert_eq!(d.nodes.added + d.nodes.removed + d.nodes.affected, 0);
        assert_eq!(d.edges.unchanged, 1);
        assert_eq!(d.edges.added + d.edges.removed + d.edges.affected, 0);
    }

    #[test]
    fn added_and_removed_nodes_are_counted() {
        let before = graph(vec![node("a"), node("b")], vec![]);
        let after = graph(vec![node("a"), node("c")], vec![]);
        let d = diff_graph(&before, &after);
        assert_eq!(d.nodes.added, 1, "c is new");
        assert_eq!(d.nodes.removed, 1, "b is gone");
        assert_eq!(d.nodes.unchanged, 1, "a persists");
    }

    #[test]
    fn external_nodes_are_ignored() {
        // The external node is filtered out — its disappearance is not a removal.
        let before = graph(vec![node("a"), ext_node("ext")], vec![]);
        let after = graph(vec![node("a")], vec![]);
        let d = diff_graph(&before, &after);
        assert_eq!(d.nodes.removed, 0, "external node is not counted");
        assert_eq!(d.nodes.unchanged, 1);
    }

    #[test]
    fn edge_change_propagates_affected_to_nodes_and_unchanged_edges() {
        // before: a→b ; after: a→b, a→c (plus the new node c).
        let before = graph(vec![node("a"), node("b")], vec![edge("a", "b")]);
        let after = graph(
            vec![node("a"), node("b"), node("c")],
            vec![edge("a", "b"), edge("a", "c")],
        );
        let d = diff_graph(&before, &after);
        // c added; a is affected (adjacent to the new edge); b is unchanged.
        assert_eq!(d.nodes.added, 1, "c");
        assert_eq!(d.nodes.affected, 1, "a touches the new edge");
        assert_eq!(d.nodes.unchanged, 1, "b");
        // a→c is added; a→b persists but its endpoint a changed → affected.
        assert_eq!(d.edges.added, 1, "a→c");
        assert_eq!(d.edges.affected, 1, "a→b is unchanged but its node changed");
        assert_eq!(d.edges.unchanged, 0);
    }

    #[test]
    fn cycle_counts_are_read_from_graph_annotations() {
        let mut before = graph(
            vec![node("a"), node("b")],
            vec![edge("a", "b"), edge("b", "a")],
        );
        before.cycles = vec![CycleGroup {
            kind: CycleKind::Mutual,
            nodes: vec!["a".into(), "b".into()],
        }];
        let after = graph(
            vec![node("a"), node("b")],
            vec![edge("a", "b"), edge("b", "a")],
        );
        let d = diff_graph(&before, &after);
        assert_eq!(d.sccs_before, 1);
        assert_eq!(d.cycle_nodes_before, 2);
        assert_eq!(d.sccs_after, 0, "after carries no cycle annotations");
        assert_eq!(d.cycle_nodes_after, 0);
    }

    // ── compare_snapshots + snap_meta ───────────────────────────────────────

    fn snap(files: Graph, git: Option<GitInfo>, target: &str) -> Snapshot {
        let graphs = PluginGraphs { files };
        Snapshot::new(
            "report".into(),
            "/w".into(),
            target.into(),
            "rust".into(),
            None,
            false,
            HashMap::new(),
            HashMap::new(),
            git,
            Vec::new(),
            graphs,
        )
    }

    #[test]
    fn compare_identical_snapshots_sets_identical_true() {
        let g = graph(vec![node("a")], vec![]);
        let s = compare_snapshots(&snap(g.clone(), None, "/x/proj"), &snap(g, None, "/x/proj"));
        assert!(s.identical, "no node/edge changes at any level");
        assert_eq!(s.schema_version, "1");
    }

    #[test]
    fn compare_differing_snapshots_sets_identical_false() {
        let before = snap(graph(vec![node("a")], vec![]), None, "/x/proj");
        let after = snap(graph(vec![node("a"), node("b")], vec![]), None, "/x/proj");
        let s = compare_snapshots(&before, &after);
        assert!(!s.identical);
        assert_eq!(s.files.nodes.added, 1);
    }

    #[test]
    fn snap_meta_shortens_target_basename_and_commit() {
        let git = Some(GitInfo {
            branch: "main".into(),
            commit: "0123456789abcdef".into(),
            dirty_files: 0,
            origin: None,
        });
        let before = snap(
            graph(vec![node("a")], vec![]),
            git.clone(),
            "/home/u/my-project",
        );
        let after = snap(graph(vec![node("a")], vec![]), git, "/home/u/my-project");
        let s = compare_snapshots(&before, &after);
        assert_eq!(s.before.target, "my-project", "basename only");
        assert_eq!(
            s.before.commit.as_deref(),
            Some("01234567"),
            "first 8 chars"
        );
        assert_eq!(s.before.branch.as_deref(), Some("main"));
    }
}
