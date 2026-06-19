//! Collapse the internal module graph into a file-level `api::Graph`.
//!
//! Extracted from `lib.rs` to keep per-file complexity under the project's
//! thresholds; pure code movement, no behaviour change.

use code_ranker_plugin_api::{attrs::AttrValue, edge::Edge, graph::Graph, node::Node};
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::languages::rust::internal::{self, EdgeKind, InternalGraph, NodeKind};

/// The output `kind` string for an internal [`EdgeKind`], taken from the
/// `[edge_kinds]` table in `rust/config.toml` (the same table `levels()`
/// publishes), so the tagged edge `kind` and the level descriptor never drift.
/// The variant→key mapping stays in Rust (the enum is Rust); the identifier
/// itself is data and `config::edge_kind_id` panics if the config omits it.
fn edge_kind_id(kind: EdgeKind) -> &'static str {
    let key = match kind {
        EdgeKind::Contains => "contains",
        EdgeKind::Uses => "uses",
        EdgeKind::Reexports => "reexports",
        EdgeKind::Super => "super",
    };
    crate::config::edge_kind_id(&super::cfg::CONFIG, key)
        .unwrap_or_else(|| panic!("rust/config.toml [edge_kinds] is missing `{key}`"));
    key
}

/// The node-attribute key string, validated against `[node_attributes]` in
/// `rust/config.toml` (the same table `levels()` publishes), so an inserted attr
/// can never use a key the level descriptor does not declare. Mirrors
/// [`edge_kind_id`]: the key IS the table key (identity), validated, not invented.
fn attr_key(key: &'static str) -> &'static str {
    crate::config::attr_key(&super::cfg::CONFIG, key)
        .unwrap_or_else(|| panic!("rust/config.toml [node_attributes] is missing `{key}`"));
    key
}

/// Emit the file's syntactic fact sets (`derives`/`macros`/`attrs`/`imports`/
/// `types`/`traits`) as comma-joined string attributes. Empty sets carry no key.
fn emit_facts(
    attrs: &mut code_ranker_plugin_api::attrs::Attributes,
    facts: &super::internal::Facts,
) {
    for (key, value) in [
        ("derives", &facts.derives),
        ("macros", &facts.macros),
        ("attrs", &facts.attrs),
        ("imports", &facts.imports),
        ("types", &facts.types),
        ("traits", &facts.traits),
    ] {
        if let Some(v) = value {
            attrs.insert(attr_key(key).to_string(), AttrValue::Str(v.clone()));
        }
    }
}

/// The crate-root source filename (`lib.rs`) preferred when a crate node owns
/// several root module files — DATA from `config.toml` (`crate_root_file`); the
/// tie-break LOGIC (prefer this file) stays in [`collapse_to_files`].
fn crate_root_filename() -> &'static str {
    static NAME: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        super::cfg::CONFIG
            .get("crate_root_file")
            .and_then(|v| v.as_str())
            .expect("rust/config.toml `crate_root_file`")
            .to_string()
    });
    NAME.as_str()
}

/// Collapse the internal module graph into a file-level `api::Graph`.
///
/// - Every `Module` node maps to a `file` node keyed by its ABSOLUTE source
///   path (no `file:` prefix). Inline modules collapse into the file they live
///   in. The file-backed module (line == None) is the source of truth for
///   structural attrs.
/// - External crate nodes become one `external` node each (id `ext:{name}`).
/// - `use`/`pub use` edges are re-pointed to files; self-edges (within the same
///   file) are dropped.
/// - Crate→crate dependency edges (metadata-level) are dropped; precise
///   file→file edges come from `use` statements.
pub(crate) fn collapse_to_files(full: InternalGraph) -> Graph {
    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut file_nodes: HashMap<String, Node> = HashMap::new();
    let mut ext_nodes: HashMap<String, Node> = HashMap::new();

    // Pre-pass: map each LOCAL crate node to its crate-root source file
    // (lib.rs / main.rs) via the crate→root-module Contains edge. This lets
    // cross-crate `use other_crate::…` become file→file edges.
    let node_by_id: HashMap<&str, &internal::Node> =
        full.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let crate_ids: HashSet<&str> = full
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Crate)
        .map(|n| n.id.as_str())
        .collect();
    let mut crate_root_file: HashMap<String, String> = HashMap::new();
    for e in &full.edges {
        if e.kind != EdgeKind::Contains {
            continue;
        }
        let (Some(from), Some(to)) = (
            node_by_id.get(e.from.as_str()),
            node_by_id.get(e.to.as_str()),
        ) else {
            continue;
        };
        if from.kind == NodeKind::Crate && to.kind == NodeKind::Module && !to.path.is_empty() {
            let file = to.path.clone(); // ABSOLUTE path, no prefix
            match crate_root_file.entry(e.from.clone()) {
                Entry::Vacant(v) => {
                    v.insert(file);
                }
                Entry::Occupied(mut o) if to.path.ends_with(crate_root_filename()) => {
                    *o.get_mut() = file;
                }
                Entry::Occupied(_) => {}
            }
        }
    }

    for node in &full.nodes {
        match node.kind {
            NodeKind::Module => {
                let fid = node.path.clone(); // ABSOLUTE path
                id_map.insert(node.id.clone(), fid.clone());
                let name = Path::new(&node.path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| node.name.clone());
                match file_nodes.entry(fid.clone()) {
                    Entry::Vacant(v) => {
                        let mut attrs = BTreeMap::new();
                        if let Some(vis) = &node.visibility {
                            attrs.insert(
                                attr_key("visibility").to_string(),
                                AttrValue::Str(vis.as_str().to_string()),
                            );
                        }
                        if let Some(loc) = node.loc {
                            attrs.insert(attr_key("loc").to_string(), AttrValue::Int(loc as i64));
                        }
                        if let Some(items) = node.item_count {
                            attrs.insert(
                                attr_key("items").to_string(),
                                AttrValue::Int(items as i64),
                            );
                        }
                        // Omit when zero, like other metrics — files with no
                        // `unsafe` simply carry no key.
                        if let Some(u) = node.unsafe_count
                            && u > 0
                        {
                            attrs.insert(attr_key("unsafe").to_string(), AttrValue::Int(u as i64));
                        }
                        if let Some(krate) = &node.crate_label {
                            attrs.insert(
                                attr_key("crate").to_string(),
                                AttrValue::Str(krate.clone()),
                            );
                        }
                        emit_facts(&mut attrs, &node.facts);
                        v.insert(Node {
                            id: fid,
                            kind: code_ranker_plugin_api::node::FILE.into(),
                            name,
                            parent: None,
                            attrs,
                        });
                    }
                    Entry::Occupied(mut o) => {
                        // The file-backed module (line == None) is the source
                        // of truth for the file's structural attrs.
                        if node.line.is_none() {
                            let n = o.get_mut();
                            if let Some(vis) = &node.visibility {
                                n.attrs.insert(
                                    attr_key("visibility").to_string(),
                                    AttrValue::Str(vis.as_str().to_string()),
                                );
                            }
                            if let Some(loc) = node.loc {
                                n.attrs.insert(
                                    attr_key("loc").to_string(),
                                    AttrValue::Int(loc as i64),
                                );
                            }
                            if let Some(items) = node.item_count {
                                n.attrs.insert(
                                    attr_key("items").to_string(),
                                    AttrValue::Int(items as i64),
                                );
                            }
                            if let Some(u) = node.unsafe_count
                                && u > 0
                            {
                                n.attrs.insert(
                                    attr_key("unsafe").to_string(),
                                    AttrValue::Int(u as i64),
                                );
                            }
                            if let Some(krate) = &node.crate_label {
                                n.attrs.insert(
                                    attr_key("crate").to_string(),
                                    AttrValue::Str(krate.clone()),
                                );
                            }
                            emit_facts(&mut n.attrs, &node.facts);
                        }
                    }
                }
            }
            NodeKind::Crate if node.external.unwrap_or(false) => {
                let eid = format!("{}{}", super::cfg::ID_EXTERNAL.as_str(), node.name);
                id_map.insert(node.id.clone(), eid.clone());
                // The on-disk directory of this dependency (parent of its
                // Cargo.toml), e.g. `…/registry/src/…/serde-1.0.228`.
                let lib_path = Path::new(&node.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ext_nodes.entry(eid.clone()).or_insert_with(|| {
                    let mut attrs = BTreeMap::new();
                    attrs.insert(attr_key("external").to_string(), AttrValue::Bool(true));
                    if let Some(v) = &node.version {
                        attrs.insert(attr_key("version").to_string(), AttrValue::Str(v.clone()));
                    }
                    if !lib_path.is_empty() {
                        attrs.insert(attr_key("path").to_string(), AttrValue::Str(lib_path));
                    }
                    Node {
                        id: eid,
                        kind: code_ranker_plugin_api::node::EXTERNAL.into(),
                        name: node.name.clone(),
                        parent: None,
                        attrs,
                    }
                });
            }
            // A local workspace crate maps to its root file.
            NodeKind::Crate => {
                if let Some(file) = crate_root_file.get(&node.id) {
                    id_map.insert(node.id.clone(), file.clone());
                }
            }
        }
    }

    // Re-point edges to file/external granularity.
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut edges: Vec<Edge> = Vec::new();
    for e in &full.edges {
        // Drop crate→crate dependency edges; precise file→file edges come from
        // `use` statements.
        if crate_ids.contains(e.from.as_str()) && crate_ids.contains(e.to.as_str()) {
            continue;
        }
        let (Some(from), Some(to)) = (id_map.get(&e.from), id_map.get(&e.to)) else {
            continue;
        };
        if from == to {
            continue; // within the same file — not a connection
        }
        let kind_str = edge_kind_id(e.kind);
        if !seen.insert((from.clone(), to.clone(), kind_str.to_string())) {
            continue;
        }
        let mut attrs = BTreeMap::new();
        if e.kind == EdgeKind::Reexports
            && let Some(vis) = &e.visibility
        {
            attrs.insert(
                attr_key("visibility").to_string(),
                AttrValue::Str(vis.as_str().to_string()),
            );
        }
        edges.push(Edge {
            source: from.clone(),
            target: to.clone(),
            kind: kind_str.to_string(),
            line: e.line,
            attrs,
        });
    }

    // Assemble nodes: all files + only the libraries actually referenced.
    let referenced_ext: HashSet<&str> = edges
        .iter()
        .filter(|e| ext_nodes.contains_key(&e.target))
        .map(|e| e.target.as_str())
        .collect();
    let mut nodes: Vec<Node> = file_nodes.into_values().collect();
    nodes.extend(
        ext_nodes
            .into_iter()
            .filter(|(id, _)| referenced_ext.contains(id.as_str()))
            .map(|(_, n)| n),
    );

    // Deterministic output ordering.
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.kind.cmp(&b.kind))
    });

    Graph { nodes, edges }
}
