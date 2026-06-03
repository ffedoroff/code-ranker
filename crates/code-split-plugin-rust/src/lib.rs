use anyhow::Result;
use code_split_plugin_api::{
    AttrValue, AttributeSpec, Edge, EdgeKindSpec, Graph, LanguagePlugin, Level, Node, PluginInput,
    ValueType,
};
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use cargo_metadata::MetadataCommand;

mod crate_graph;
mod ids;
mod internal;
mod module_graph;

use internal::{EdgeKind, GraphBuilder, InternalGraph, NodeKind};

pub struct RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn name(&self) -> &str {
        "rust"
    }

    fn detect(&self, workspace: &Path, _input: &PluginInput) -> bool {
        workspace.join("Cargo.toml").exists()
    }

    fn levels(&self) -> Vec<Level> {
        let mut edge_kinds: BTreeMap<String, EdgeKindSpec> = BTreeMap::new();
        edge_kinds.insert(
            "uses".into(),
            EdgeKindSpec {
                flow: true,
                label: Some("uses".into()),
                hint: Some("Import / use dependency — this file uses items from the other.".into()),
            },
        );
        edge_kinds.insert(
            "contains".into(),
            EdgeKindSpec {
                flow: false,
                label: Some("contains".into()),
                hint: Some(
                    "Module declaration (mod foo;) — structural ownership; excluded from fan-in / HK / cycles.".into(),
                ),
            },
        );
        edge_kinds.insert(
            "reexports".into(),
            EdgeKindSpec {
                flow: true,
                label: Some("reexport".into()),
                hint: Some(
                    "Re-export (pub use) — re-exposes the other file's items as part of its own API.".into(),
                ),
            },
        );

        let aspec = |vt: ValueType, label: &str| AttributeSpec {
            value_type: vt,
            label: Some(label.into()),
            hint: None,
            group: None,
        };

        let mut node_attributes: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        node_attributes.insert("path".into(), aspec(ValueType::Str, "Path"));
        node_attributes.insert("loc".into(), aspec(ValueType::Int, "Lines"));
        node_attributes.insert("visibility".into(), aspec(ValueType::Str, "Visibility"));
        node_attributes.insert("external".into(), aspec(ValueType::Bool, "External"));
        node_attributes.insert("version".into(), aspec(ValueType::Str, "Version"));
        node_attributes.insert("items".into(), aspec(ValueType::Int, "Items"));

        let mut edge_attributes: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        edge_attributes.insert("visibility".into(), aspec(ValueType::Str, "Visibility"));

        vec![Level {
            name: "files".into(),
            edge_kinds,
            node_attributes,
            edge_attributes,
            attribute_groups: BTreeMap::new(),
        }]
    }

    fn analyze(&self, workspace: &Path, _level: &str, _input: &PluginInput) -> Result<Graph> {
        let mut builder = GraphBuilder::new();
        syn_analyze(workspace, &mut builder)?;
        let internal = builder.build();
        Ok(collapse_to_files(internal))
    }

    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        version_string()
            .map(|rv| vec![("rustc".to_string(), rv)])
            .unwrap_or_default()
    }
}

/// Syntactic stage: resolve the workspace via `cargo metadata` and build the
/// internal crate + module/use graphs.
fn syn_analyze(workspace: &Path, builder: &mut GraphBuilder) -> Result<()> {
    let manifest = workspace.join("Cargo.toml");
    // code-split is an offline tool: it never fetches from the network. See the
    // comment in the original lib.rs for the research notes on --offline vs
    // --no-deps vs full. Short version: --offline keeps external/cross-crate
    // edges AND never goes to the network; the cache must be warm.
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .other_options(vec!["--offline".to_string()])
        .exec()
        .map_err(|err| offline_metadata_error(&manifest, err))?;

    crate_graph::contribute(&metadata, builder);
    module_graph::contribute(&metadata, builder)?;
    Ok(())
}

fn offline_metadata_error(manifest: &Path, err: cargo_metadata::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "cargo metadata (offline) failed for {manifest}\n\n\
         code-split is an offline tool — it never downloads dependencies. It reads \
         the dependency graph from cargo's local cache, which must already be \
         populated for this project.\n\n\
         Warm the cache once (with network), then re-run code-split:\n    \
         cargo metadata --manifest-path {manifest} >/dev/null\n\
         (a prior `cargo build` / `cargo fetch` works too).\n\n\
         In CI: run code-split on the same image/cache as your build or test jobs, \
         where the cache is already warm.\n\n\
         Underlying cargo error: {err}",
        manifest = manifest.display(),
    )
}

fn version_string() -> Option<String> {
    which::which("rustc").ok()?;
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()?;
    if out.status.success() {
        Some(
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .nth(1)
                .unwrap_or("unknown")
                .to_string(),
        )
    } else {
        None
    }
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
fn collapse_to_files(full: InternalGraph) -> Graph {
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
                Entry::Occupied(mut o) if to.path.ends_with("lib.rs") => {
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
                                "visibility".to_string(),
                                AttrValue::Str(vis.as_str().to_string()),
                            );
                        }
                        if let Some(loc) = node.loc {
                            attrs.insert("loc".to_string(), AttrValue::Int(loc as i64));
                        }
                        if let Some(items) = node.item_count {
                            attrs.insert("items".to_string(), AttrValue::Int(items as i64));
                        }
                        v.insert(Node {
                            id: fid,
                            kind: "file".into(),
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
                                    "visibility".to_string(),
                                    AttrValue::Str(vis.as_str().to_string()),
                                );
                            }
                            if let Some(loc) = node.loc {
                                n.attrs
                                    .insert("loc".to_string(), AttrValue::Int(loc as i64));
                            }
                            if let Some(items) = node.item_count {
                                n.attrs
                                    .insert("items".to_string(), AttrValue::Int(items as i64));
                            }
                        }
                    }
                }
            }
            NodeKind::Crate if node.external.unwrap_or(false) => {
                let eid = format!("ext:{}", node.name);
                id_map.insert(node.id.clone(), eid.clone());
                // The on-disk directory of this dependency (parent of its
                // Cargo.toml), e.g. `…/registry/src/…/serde-1.0.228`.
                let lib_path = Path::new(&node.path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ext_nodes.entry(eid.clone()).or_insert_with(|| {
                    let mut attrs = BTreeMap::new();
                    attrs.insert("external".to_string(), AttrValue::Bool(true));
                    if let Some(v) = &node.version {
                        attrs.insert("version".to_string(), AttrValue::Str(v.clone()));
                    }
                    if !lib_path.is_empty() {
                        attrs.insert("path".to_string(), AttrValue::Str(lib_path));
                    }
                    Node {
                        id: eid,
                        kind: "external".into(),
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
        let kind_str = match e.kind {
            EdgeKind::Contains => "contains",
            EdgeKind::Uses => "uses",
            EdgeKind::Reexports => "reexports",
        };
        if !seen.insert((from.clone(), to.clone(), kind_str.to_string())) {
            continue;
        }
        let mut attrs = BTreeMap::new();
        if e.kind == EdgeKind::Reexports
            && let Some(vis) = &e.visibility
        {
            attrs.insert(
                "visibility".to_string(),
                AttrValue::Str(vis.as_str().to_string()),
            );
        }
        edges.push(Edge {
            source: from.clone(),
            target: to.clone(),
            kind: kind_str.to_string(),
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
