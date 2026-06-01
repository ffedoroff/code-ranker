use anyhow::Result;
use code_split_core::{Edge, EdgeKind, Graph, GraphBuilder, Node, NodeKind, PluginGraphs, StageTime};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::logger;

pub fn run(workspace: &Path, local_only: bool) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings: Vec<StageTime> = Vec::new();
    let mut builder = GraphBuilder::new();

    {
        let t = logger::Timer::start("syn: parsing modules and files");
        if local_only {
            code_split_syn::analyze_local_only(workspace, &mut builder)?;
        } else {
            code_split_syn::analyze(workspace, &mut builder)?;
        }
        let n = builder.node_count();
        let detail = format!("{n} nodes");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "syn".into(),
            ms,
            detail,
        });
    }

    {
        let t = logger::Timer::start("complexity: cyclomatic / cognitive / halstead / MI / LOC");
        let annotated = match code_split_complexity::analyze(workspace, &mut builder) {
            Ok(n) => n,
            Err(e) => {
                logger::info(&format!("complexity skipped: {e:#}"));
                0
            }
        };
        let detail = format!("{annotated} nodes annotated");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "complexity".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting file graph");
    let files = collapse_to_files(builder.build());
    let detail = format!("files={} edges={}", files.nodes.len(), files.edges.len());
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "projection".into(),
        ms,
        detail,
    });

    Ok((PluginGraphs { files }, timings))
}

/// Collapse the Rust module graph (`Crate` / `Module` / `Trait` nodes plus
/// `Contains` / `Uses` / `Reexports` edges) into a single file-level graph.
///
/// - Every `Module` node maps to a `File` node keyed by its source path; the
///   file-backed module (`line == None`) carries the file's metrics, inline
///   modules collapse into it. This preserves all file→file connections even
///   though Rust expresses dependencies via module paths, not file paths.
/// - External crate nodes become one `External` library node each (depth 1).
/// - `use`/`pub use` edges are re-pointed to files; self-edges (a `use` within
///   the same file) are dropped, and edges into libraries are flagged external.
fn collapse_to_files(full: Graph) -> Graph {
    // 1. Map each source node id → its file/external id, building the target nodes.
    let mut id_map: HashMap<String, String> = HashMap::new();
    let mut file_nodes: HashMap<String, Node> = HashMap::new();
    let mut ext_nodes: HashMap<String, Node> = HashMap::new();

    // Pre-pass: map each LOCAL crate to its crate-root source file (lib.rs /
    // main.rs), via the crate→root-module `Contains` edge. This lets a
    // cross-crate `use other_crate::…` (or a captured bare-path reference)
    // become a file→file edge to that crate's root, instead of pointing at a
    // crate node that has no file and would be dropped. `lib` wins over `bin`.
    let node_by_id: HashMap<&str, &Node> = full.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
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
        let (Some(from), Some(to)) =
            (node_by_id.get(e.from.as_str()), node_by_id.get(e.to.as_str()))
        else {
            continue;
        };
        if from.kind == NodeKind::Crate && to.kind == NodeKind::Module && !to.path.is_empty() {
            let file = format!("file:{}", to.path);
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
                let fid = format!("file:{}", node.path);
                id_map.insert(node.id.clone(), fid.clone());
                let name = Path::new(&node.path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| node.name.clone());
                match file_nodes.entry(fid.clone()) {
                    Entry::Vacant(v) => {
                        v.insert(Node {
                            id: fid,
                            kind: NodeKind::File,
                            name,
                            path: node.path.clone(),
                            parent: None,
                            external: None,
                            visibility: node.visibility.clone(),
                            loc: node.loc,
                            line: None,
                            item_count: node.item_count,
                            method_count: None,
                            complexity: node.complexity.clone(),
                            cycle_kind: None,
                        });
                    }
                    Entry::Occupied(mut o) => {
                        // The file-backed module (`line == None`) is the source of
                        // truth for the file's metrics; inline modules add nothing.
                        if node.line.is_none() {
                            let n = o.get_mut();
                            if node.complexity.is_some() {
                                n.complexity = node.complexity.clone();
                            }
                            if node.loc.is_some() {
                                n.loc = node.loc;
                            }
                            if node.item_count.is_some() {
                                n.item_count = node.item_count;
                            }
                        }
                    }
                }
            }
            NodeKind::Crate if node.external.unwrap_or(false) => {
                let eid = format!("ext:{}", node.name);
                id_map.insert(node.id.clone(), eid.clone());
                ext_nodes.entry(eid.clone()).or_insert_with(|| Node {
                    id: eid,
                    kind: NodeKind::External,
                    name: node.name.clone(),
                    path: String::new(),
                    parent: None,
                    external: Some(true),
                    visibility: None,
                    loc: None,
                    line: None,
                    item_count: None,
                    method_count: None,
                    complexity: None,
                    cycle_kind: None,
                });
            }
            // A local workspace crate maps to its root file, so cross-crate
            // dependencies on it become file→file edges to that root.
            NodeKind::Crate => {
                if let Some(file) = crate_root_file.get(&node.id) {
                    id_map.insert(node.id.clone(), file.clone());
                }
            }
            // `Trait` (and anything else) are dropped: no mapping, so edges
            // touching them fall away below.
            _ => {}
        }
    }

    // 2. Re-point edges to file/external granularity.
    let mut seen: HashSet<(String, String, EdgeKind)> = HashSet::new();
    let mut edges: Vec<Edge> = Vec::new();
    for e in &full.edges {
        // Drop crate→crate dependency edges (crate-level meta from `cargo
        // metadata`); the precise file→file edges come from module-level `use`
        // statements and captured bare-path references.
        if crate_ids.contains(e.from.as_str()) && crate_ids.contains(e.to.as_str()) {
            continue;
        }
        let (Some(from), Some(to)) = (id_map.get(&e.from), id_map.get(&e.to)) else {
            continue;
        };
        if from == to {
            continue; // within the same file (inline module / self-use) — not a connection
        }
        // Cross-file `Contains` edges (a `mod foo;` declaration, parent → child)
        // are KEPT in the snapshot as structural metadata, but consumers treat
        // them as ownership, not information flow: they are not drawn on the main
        // map, not counted in fan_in/HK, and excluded from cycle detection.
        let kind = e.kind;
        let to_external = ext_nodes.contains_key(to);
        if !seen.insert((from.clone(), to.clone(), kind)) {
            continue;
        }
        edges.push(Edge {
            from: from.clone(),
            to: to.clone(),
            kind,
            unresolved: None,
            external: to_external.then_some(true),
            visibility: e.visibility.clone(),
        });
    }

    // 3. Assemble nodes: all files + only the libraries actually referenced.
    let referenced_ext: HashSet<&str> = edges
        .iter()
        .filter(|e| e.external.unwrap_or(false))
        .map(|e| e.to.as_str())
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
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
            .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
    });

    Graph {
        nodes,
        edges,
        cycles: Vec::new(),
        stats: None,
    }
}

pub fn version_string() -> Option<String> {
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
