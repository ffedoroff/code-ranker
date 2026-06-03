//! The serializable analysis artifact ([`Snapshot`]) and its per-level payload
//! ([`LevelGraph`]), plus canonical (deterministic) JSON serialization and id
//! relativization.
//!
//! Shape (schema version `"2"`): the snapshot keeps the historical header
//! (workspace/target/plugin/roots/versions/git/timings) and carries a `graphs`
//! map `level_name -> LevelGraph`. Each [`LevelGraph`] bundles the structural
//! graph (`nodes`/`edges`) with the level's semantics dictionaries
//! (`edge_kinds`/`node_attributes`/`edge_attributes`/`attribute_groups`) and the
//! computed `cycles` + `stats`, so the UI can render any language/metric set
//! without hardcoding names.

use chrono::{DateTime, Utc};
use code_split_plugin_api::{
    AttrValue, AttributeGroup, AttributeSpec, Edge, EdgeKindSpec, Graph, Node, NodeId,
};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::Path;

/// Per-stage timing in milliseconds, in execution order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTime {
    pub stage: String,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// One strongly-connected component with ≥ 2 nodes, plus its classification
/// (`"mutual"` / `"chain"` / `"test_embed"`). Node ids match the level graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleGroup {
    pub kind: String,
    pub nodes: Vec<NodeId>,
}

/// Everything for one analysis level: the structural graph, the semantics
/// dictionaries that describe its vocabulary, and the computed cycles + stats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LevelGraph {
    /// Edge kinds present at this level (keyed by kind), with `flow` semantics.
    pub edge_kinds: BTreeMap<String, EdgeKindSpec>,
    /// Node attribute dictionary (structural keys + appended computed metrics).
    pub node_attributes: BTreeMap<String, AttributeSpec>,
    /// Edge attribute dictionary.
    pub edge_attributes: BTreeMap<String, AttributeSpec>,
    /// Attribute group definitions referenced by `AttributeSpec.group`.
    pub attribute_groups: BTreeMap<String, AttributeGroup>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// SCCs with ≥ 2 members, classified by kind.
    pub cycles: Vec<CycleGroup>,
    /// Per-graph averages of numeric node attributes (flat, keyed by attr name).
    pub stats: BTreeMap<String, AttrValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub command: String,
    /// Directory from which `code-split` was invoked.
    pub workspace: String,
    /// The analyzed project directory (absolute path, stored once here).
    pub target: String,
    pub plugin: String,
    /// Config file used for this analysis, if any was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    pub versions: BTreeMap<String, String>,
    /// Named system roots used to shorten node paths (e.g. `{registry}`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roots: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timings: Vec<StageTime>,
    /// Analysis levels, keyed by level name. Today only `"files"` is produced.
    pub graphs: BTreeMap<String, LevelGraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub commit: String,
    pub dirty_files: u32,
    /// Remote `origin` URL (raw). Used by the HTML report for source links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl Snapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        workspace: String,
        target: String,
        plugin: String,
        config_file: Option<String>,
        versions: BTreeMap<String, String>,
        roots: BTreeMap<String, String>,
        git: Option<GitInfo>,
        timings: Vec<StageTime>,
        graphs: BTreeMap<String, LevelGraph>,
    ) -> Self {
        Self {
            schema_version: "2".to_string(),
            generated_at: Utc::now(),
            command,
            workspace,
            target,
            plugin,
            config_file,
            versions,
            roots,
            git,
            timings,
            graphs,
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical (deterministic) JSON serialization
// ---------------------------------------------------------------------------

/// Serialize to canonical pretty JSON: object keys come out alphabetically
/// (`serde_json::Value` is backed by a `BTreeMap`), and the `nodes`/`edges`
/// arrays are sorted by a stable key so unchanged input is byte-identical.
pub fn to_canonical_string_pretty<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string_pretty(&v)
}

/// Compact counterpart of [`to_canonical_string_pretty`].
pub fn to_canonical_string<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string(&v)
}

fn canonicalize_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                canonicalize_value(item);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                canonicalize_value(val);
            }
            if let Some(serde_json::Value::Array(nodes)) = map.get_mut("nodes") {
                nodes.sort_by_key(|a| json_str(a, "id"));
            }
            if let Some(serde_json::Value::Array(edges)) = map.get_mut("edges") {
                edges.sort_by(|a, b| {
                    json_str(a, "source")
                        .cmp(&json_str(b, "source"))
                        .then_with(|| json_str(a, "target").cmp(&json_str(b, "target")))
                        .then_with(|| json_str(a, "kind").cmp(&json_str(b, "kind")))
                });
            }
        }
        _ => {}
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// Id relativization
// ---------------------------------------------------------------------------

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

pub(crate) fn relativize_path(
    path: &str,
    target: &Path,
    roots: &BTreeMap<String, String>,
) -> String {
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
        use code_split_plugin_api::Edge;
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
}
