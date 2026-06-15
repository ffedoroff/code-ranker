//! Id relativization: rewrite absolute file-path node ids to portable
//! `{target}/rel` / `{root}/rel` tokens (and follow them through edges, parents,
//! cycle node lists, and external `path` attributes), so a snapshot is
//! machine-independent.

use crate::level_graph::LevelGraph;
use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::Path;

/// Rewrite a file-based level graph from absolute paths to relativized ids:
/// - file node ids (absolute paths) → `{target}/rel` or `{root}/rel`;
/// - edge endpoints, node parents and cycle node lists follow the same map;
/// - external node `path` attributes are relativized too;
/// - redundant/empty `path` attributes are dropped (a file node's id IS its
///   path, so it carries none).
pub fn relativize_level(level: &mut LevelGraph, target: &Path, roots: &BTreeMap<String, String>) {
    let id_map = relativize_graph_inner(&mut level.nodes, &mut level.edges, target, roots);
    for cycle in &mut level.cycles {
        for n in &mut cycle.nodes {
            if let Some(nn) = id_map.get(n) {
                *n = nn.clone();
            }
        }
    }
}

/// Relativize a structural [`Graph`] in place (before cycles are computed):
/// file node ids (absolute paths) become `{target}/rel` / `{root}/rel`, edge
/// endpoints and node parents follow, external `path` attributes are relativized,
/// and redundant/empty `path` attributes are dropped.
pub fn relativize_graph(graph: &mut Graph, target: &Path, roots: &BTreeMap<String, String>) {
    relativize_graph_inner(&mut graph.nodes, &mut graph.edges, target, roots);
}

fn relativize_graph_inner(
    nodes: &mut [Node],
    edges: &mut [Edge],
    target: &Path,
    roots: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut id_map: BTreeMap<String, String> = BTreeMap::new();
    for node in nodes.iter() {
        if node.kind == "external" {
            continue; // external ids (`ext:name`) are already short
        }
        let new_id = relativize_path(&node.id, target, roots);
        if new_id != node.id {
            id_map.insert(node.id.clone(), new_id);
        }
    }

    for node in nodes.iter_mut() {
        if let Some(new_id) = id_map.get(&node.id) {
            node.id = new_id.clone();
        }
        if let Some(parent) = node.parent.as_mut()
            && let Some(np) = id_map.get(parent)
        {
            *parent = np.clone();
        }
        if let Some(AttrValue::Str(p)) = node.attrs.get("path") {
            let rel = relativize_path(p, target, roots);
            if rel.is_empty() || rel == node.id {
                node.attrs.remove("path");
            } else {
                node.attrs.insert("path".to_string(), AttrValue::Str(rel));
            }
        }
    }
    for edge in edges.iter_mut() {
        if let Some(s) = id_map.get(&edge.source) {
            edge.source = s.clone();
        }
        if let Some(t) = id_map.get(&edge.target) {
            edge.target = t.clone();
        }
    }
    id_map
}

fn relativize_path(path: &str, target: &Path, roots: &BTreeMap<String, String>) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let p = Path::new(path);
    if let Ok(rel) = p.strip_prefix(target) {
        return format!("{{target}}/{}", rel.to_string_lossy());
    }
    // Longest root wins.
    let mut sorted: Vec<_> = roots.iter().collect();
    sorted.sort_by_key(|(_, root)| Reverse(root.len()));
    for (name, root) in &sorted {
        if let Ok(rel) = p.strip_prefix(root.as_str()) {
            return format!("{{{name}}}/{}", rel.to_string_lossy());
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level_graph::CycleGroup;

    #[test]
    fn relativize_path_under_target_uses_token() {
        let got = relativize_path("/p/src/main.rs", Path::new("/p"), &BTreeMap::new());
        assert_eq!(got, "{target}/src/main.rs");
    }

    #[test]
    fn relativize_path_longest_root_wins() {
        let roots = BTreeMap::from([
            ("home".to_string(), "/home/u".to_string()),
            ("registry".to_string(), "/home/u/.cargo".to_string()),
        ]);
        let got = relativize_path("/home/u/.cargo/x.rs", Path::new("/p"), &roots);
        assert_eq!(got, "{registry}/x.rs");
    }

    #[test]
    fn relativize_level_rewrites_ids_edges_and_cycles() {
        use code_ranker_plugin_api::edge::Edge;
        let mut level = LevelGraph::default();
        level.nodes.push(Node {
            id: "/p/src/a.rs".into(),
            kind: "file".into(),
            name: "a.rs".into(),
            parent: None,
            attrs: Default::default(),
        });
        level.nodes.push(Node {
            id: "ext:serde".into(),
            kind: "external".into(),
            name: "serde".into(),
            parent: None,
            attrs: Default::default(),
        });
        level.edges.push(Edge {
            source: "/p/src/a.rs".into(),
            target: "ext:serde".into(),
            kind: "uses".into(),
            line: None,
            attrs: Default::default(),
        });
        level.cycles.push(CycleGroup {
            kind: "mutual".into(),
            nodes: vec!["/p/src/a.rs".into()],
        });
        relativize_level(&mut level, Path::new("/p"), &BTreeMap::new());
        assert_eq!(level.nodes[0].id, "{target}/src/a.rs");
        assert_eq!(level.nodes[1].id, "ext:serde");
        assert_eq!(level.edges[0].source, "{target}/src/a.rs");
        assert_eq!(level.edges[0].target, "ext:serde");
        assert_eq!(level.cycles[0].nodes[0], "{target}/src/a.rs");
    }

    #[test]
    fn relativize_path_empty_and_unmatched_pass_through() {
        // empty input is returned as-is; a path under neither target nor any root
        // falls through unchanged.
        assert_eq!(relativize_path("", Path::new("/p"), &BTreeMap::new()), "");
        assert_eq!(
            relativize_path("/elsewhere/x.rs", Path::new("/p"), &BTreeMap::new()),
            "/elsewhere/x.rs"
        );
    }

    #[test]
    fn relativize_graph_remaps_parent_and_path_attr() {
        let roots = BTreeMap::from([("reg".to_string(), "/reg".to_string())]);
        let mut child_attrs = BTreeMap::new();
        // a `path` attr under a root → rewritten to `{reg}/…`
        child_attrs.insert("path".to_string(), AttrValue::Str("/reg/dep/lib.rs".into()));
        let mut redundant_attrs = BTreeMap::new();
        // a `path` attr equal to the node's own (relativized) id → dropped
        redundant_attrs.insert("path".to_string(), AttrValue::Str("/p/b.rs".into()));

        let mut graph = Graph {
            nodes: vec![
                Node {
                    id: "/p/a.rs".into(),
                    kind: "file".into(),
                    name: "a.rs".into(),
                    parent: Some("/p/mod.rs".into()),
                    attrs: child_attrs,
                },
                Node {
                    id: "/p/mod.rs".into(),
                    kind: "file".into(),
                    name: "mod.rs".into(),
                    parent: None,
                    attrs: Default::default(),
                },
                Node {
                    id: "/p/b.rs".into(),
                    kind: "file".into(),
                    name: "b.rs".into(),
                    parent: None,
                    attrs: redundant_attrs,
                },
            ],
            edges: vec![],
        };
        relativize_graph(&mut graph, Path::new("/p"), &roots);

        let a = graph.nodes.iter().find(|n| n.name == "a.rs").unwrap();
        assert_eq!(a.id, "{target}/a.rs");
        assert_eq!(a.parent.as_deref(), Some("{target}/mod.rs"), "parent remapped");
        assert_eq!(
            a.attrs.get("path"),
            Some(&AttrValue::Str("{reg}/dep/lib.rs".into())),
            "path attr relativized against the root"
        );

        let b = graph.nodes.iter().find(|n| n.name == "b.rs").unwrap();
        assert!(
            !b.attrs.contains_key("path"),
            "a path attr equal to the node's own id is dropped as redundant"
        );
    }
}
