use anyhow::Result;
use code_ranker_plugin_api::{
    attrs::{AttrValue, ValueType},
    default_cycle_kinds, default_node_kinds,
    edge::Edge,
    graph::Graph,
    level::{AttributeSpec, Direction, EdgeKindSpec, Grouping, Level, Thresholds},
    log,
    node::Node,
    plugin::{LanguagePlugin, PluginInput, Preset},
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

/// One Rust-only metric-lens preset: (id, title, sort_metric, connections,
/// doc_slug, prompt body). Same shape as the generic catalog in
/// `code-ranker-cli/src/presets.rs`, but these rank modules by a single
/// coupling/size metric rather than a design principle. Slugs resolve to
/// `principles/rust/<slug>.md`.
type MetricPreset = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
    &'static str,
);

const RUST_METRIC_PRESETS: &[MetricPreset] = &[
    (
        "HK",
        "HK — Henry-Kafura Coupling",
        "hk",
        &["in", "out"],
        "henry-kafura-coupling",
        "These modules carry heavy Henry-Kafura coupling — HK = sloc × (fan_in × fan_out)²,\n\
         where sloc is the module's source lines of code (real code lines, excluding blanks\n\
         and comment-only lines), fan_in is how many modules depend on it, and fan_out is how\n\
         many it depends on.\n\
         A high score is a large module sitting on a busy crossroads of incoming and outgoing\n\
         dependencies, so any change here ripples widely.\n\n\
         For each module below, lower the factor that dominates its HK: shrink the module by\n\
         extracting cohesive pieces, or cut fan-in/fan-out by narrowing its public surface and\n\
         depending on fewer collaborators (introduce an abstraction, move a responsibility).\n\
         Keep existing API contracts intact.",
    ),
    (
        "SLOC",
        "SLOC — Module Size",
        "sloc",
        &[],
        "module-size",
        "These are the largest modules by source lines of code. Size alone is not a defect, but\n\
         oversized files usually bundle several responsibilities and are hard to read, test and\n\
         review.\n\n\
         For each module below, identify the distinct responsibilities it holds and propose how\n\
         to split it into smaller, cohesive modules — each with a single clear purpose — without\n\
         changing external behaviour.",
    ),
    (
        "FANIN",
        "Fan-in — Afferent Coupling",
        "fan_in",
        &["in"],
        "fan-in-afferent-coupling",
        "These modules have high fan-in: many other modules depend on them. They are\n\
         load-bearing — a change here forces changes (or re-review) across every dependant, and\n\
         a bug here is widely felt.\n\n\
         For each module below, confirm its public surface is a stable, minimal contract. Narrow\n\
         the API to what callers actually need, split it if different callers use disjoint parts\n\
         (see Interface Segregation), and stabilise the abstractions the rest of the codebase\n\
         leans on.",
    ),
    (
        "FANOUT",
        "Fan-out — Efferent Coupling",
        "fan_out",
        &["out"],
        "fan-out-efferent-coupling",
        "These modules have high fan-out: they depend on many other modules. High efferent\n\
         coupling makes a module fragile (it breaks when any dependency changes) and hard to\n\
         test or reuse in isolation.\n\n\
         For each module below, reduce its direct dependencies: depend on abstractions rather\n\
         than concretes (see Dependency Inversion), collapse several fine-grained collaborators\n\
         behind one focused interface, and move logic that pulls in unrelated dependencies into\n\
         a more appropriate module.",
    ),
];

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
                description: Some(
                    "Code dependency — this file references an item the target file defines.<br>\
                     Captured from `use path::Item;`, a qualified path (`crate::a::Item`, \
                     `other_crate::Item`), or a derive (`#[derive(serde::Serialize)]`).<br>\
                     The path resolves to the file that defines the item (following `pub use` \
                     re-exports), so the edge points at the definition, not a re-export hub.<br>\
                     This is the real dependency: it counts toward fan-in / fan-out, \
                     Henry-Kafura coupling and cycles."
                        .into(),
                ),
            },
        );
        edge_kinds.insert(
            "contains".into(),
            EdgeKindSpec {
                flow: false,
                label: Some("contains".into()),
                description: Some(
                    "Module ownership — the parent declares the child module \
                     (`mod foo;` / `pub mod foo;`), so `foo.rs` (or `foo/mod.rs`) belongs to it.<br>\
                     This is the Rust module tree: structure, not a code dependency.<br>\
                     Kept in the data but not drawn on the main map, and excluded from \
                     fan-in / fan-out / HK / cycles."
                        .into(),
                ),
            },
        );
        edge_kinds.insert(
            "reexports".into(),
            EdgeKindSpec {
                flow: false,
                label: Some("reexport".into()),
                description: Some(
                    "Re-export (`pub use foo::Item;`) — re-publishes another file's item as part of \
                     this file's public API (the crate-root / prelude facade, e.g. `lib.rs` doing \
                     `pub use access_scope::AccessScope;`).<br>\
                     A facade, not a dependency: excluded from fan-in / fan-out / HK / cycles and \
                     not drawn on the main map, like `contains`.<br>\
                     A consumer's `use this_crate::Item` is attributed to the file that defines \
                     `Item`, so re-export hubs (`lib.rs` / `mod.rs`) collect no false coupling — the \
                     `pub use` is still recorded here so you can see what a file exposes."
                        .into(),
                ),
            },
        );
        edge_kinds.insert(
            "super".into(),
            EdgeKindSpec {
                flow: false,
                label: Some("super".into()),
                description: Some(
                    "Namespace pull from an enclosing module — a glob `use` that reaches \
                     *up* the module tree (`use super::*`, `use crate::<ancestor>::*`), \
                     bringing the parent's items into the child's scope.<br>\
                     Usually structural scope-sugar (a module split across files referring \
                     back to itself). But if the child actually uses a parent item brought \
                     in by the glob, it IS a real back-dependency — technically a cycle. \
                     code-ranker can't tell the two apart without name resolution, so it \
                     treats `super` as a **low-priority** cycle and leaves it non-flow: \
                     deprioritized next to obvious cross-module cycles.<br>\
                     Kept in the data but not drawn on the main map, and excluded from \
                     fan-in / fan-out / HK / cycles — like `contains`."
                        .into(),
                ),
            },
        );

        let aspec = AttributeSpec::new;

        let mut node_attributes: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        node_attributes.insert("path".into(), aspec(ValueType::Str, "Path"));
        node_attributes.insert("crate".into(), aspec(ValueType::Str, "Crate"));
        node_attributes.insert("loc".into(), aspec(ValueType::Int, "Lines"));
        node_attributes.insert("visibility".into(), aspec(ValueType::Str, "Visibility"));
        node_attributes.insert("external".into(), aspec(ValueType::Bool, "External"));
        node_attributes.insert("version".into(), aspec(ValueType::Str, "Version"));
        node_attributes.insert("items".into(), aspec(ValueType::Int, "Items"));
        let mut unsafe_spec = aspec(ValueType::Int, "Unsafe");
        unsafe_spec.short = Some("Unsafe".into());
        unsafe_spec.description = Some(
            "Count of `unsafe` blocks and `unsafe fn`/`impl`/`trait` declarations \
             in production code (test items are excluded). Syntactic count: \
             `unsafe` inside a macro body is not seen, and the figure is not \
             type-checked."
                .into(),
        );
        unsafe_spec.direction = Direction::LowerBetter;
        node_attributes.insert("unsafe".into(), unsafe_spec);

        let mut edge_attributes: BTreeMap<String, AttributeSpec> = BTreeMap::new();
        edge_attributes.insert("visibility".into(), aspec(ValueType::Str, "Visibility"));

        vec![Level {
            name: "files".into(),
            edge_kinds,
            node_attributes,
            edge_attributes,
            attribute_groups: BTreeMap::new(),
            node_kinds: default_node_kinds(),
            cycle_kinds: default_cycle_kinds(),
            // Cluster the diagram by the owning crate (compilation unit), not by
            // the source folder. Falls back to `dir` if `crate` is ever absent.
            grouping: Some(Grouping {
                key: Some("crate".into()),
                function: None,
            }),
        }]
    }

    fn thresholds(&self) -> BTreeMap<String, Thresholds> {
        // Calibrated on 21 Rust crates (≥2K SLOC). ~50% of projects breach
        // `info`, ~10% breach `warning`.
        BTreeMap::from([
            (
                "hk".into(),
                Thresholds {
                    info: 150_000.0,
                    warning: 10_000_000.0,
                },
            ),
            (
                "sloc".into(),
                Thresholds {
                    info: 800.0,
                    warning: 3_000.0,
                },
            ),
            (
                "fan_out".into(),
                Thresholds {
                    info: 8.0,
                    warning: 18.0,
                },
            ),
            (
                "items".into(),
                Thresholds {
                    info: 20.0,
                    warning: 50.0,
                },
            ),
        ])
    }

    fn presets(&self, mut defaults: Vec<Preset>, _input: &PluginInput) -> Vec<Preset> {
        // Append Rust-only metric lenses to the generic catalog. Their doc links
        // reuse the principles base directory derived from an existing default's
        // `doc_url`, so they resolve to `principles/rust/<slug>.md` without
        // duplicating the host/base constant that lives in the CLI crate.
        let base_dir = defaults
            .iter()
            .find_map(|p| p.doc_url.as_deref())
            .and_then(|u| u.rsplit_once('/').map(|(dir, _)| dir.to_string()));
        for &(id, title, sort_metric, connections, slug, prompt) in RUST_METRIC_PRESETS {
            defaults.push(Preset {
                id: id.to_string(),
                label: id.to_string(),
                title: title.to_string(),
                prompt: prompt.to_string(),
                doc_url: base_dir.as_ref().map(|d| format!("{d}/{slug}.md")),
                sort_metric: sort_metric.to_string(),
                connections: connections.iter().map(|s| (*s).to_string()).collect(),
            });
        }
        defaults
    }

    fn analyze(&self, workspace: &Path, _level: &str, input: &PluginInput) -> Result<Graph> {
        let mut builder = GraphBuilder::new();
        syn_analyze(workspace, input.ignore_tests, &mut builder)?;
        let internal = builder.build();
        Ok(collapse_to_files(internal))
    }

    fn is_test_path(&self, rel_path: &str) -> bool {
        // Cargo's integration-test / bench targets live under top-level
        // `tests/` and `benches/` dirs. (Inline `#[cfg(test)]` modules are a
        // separate, attribute-based notion handled during the syn walk.)
        matches!(rel_path.split('/').next(), Some("tests") | Some("benches"))
    }

    fn versions(&self, _workspace: &Path, _input: &PluginInput) -> Vec<(String, String)> {
        version_string()
            .map(|rv| vec![("rustc".to_string(), rv)])
            .unwrap_or_default()
    }
}

/// Syntactic stage: resolve the workspace via `cargo metadata` and build the
/// internal crate + module/use graphs.
fn syn_analyze(workspace: &Path, ignore_tests: bool, builder: &mut GraphBuilder) -> Result<()> {
    let manifest = workspace.join("Cargo.toml");
    // code-ranker is an offline tool: it never fetches from the network. See the
    // comment in the original lib.rs for the research notes on --offline vs
    // --no-deps vs full. Short version: --offline keeps external/cross-crate
    // edges AND never goes to the network; the cache must be warm.
    let metadata = log::timed("cargo metadata --offline", || {
        MetadataCommand::new()
            .manifest_path(&manifest)
            .other_options(vec!["--offline".to_string()])
            .exec()
    })
    .map_err(|err| offline_metadata_error(&manifest, err))?;

    crate_graph::contribute(&metadata, builder);
    module_graph::contribute(&metadata, ignore_tests, builder)?;
    Ok(())
}

fn offline_metadata_error(manifest: &Path, err: cargo_metadata::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "cargo metadata (offline) failed for {manifest}\n\n\
         code-ranker is an offline tool — it never downloads dependencies. It reads \
         the dependency graph from cargo's local cache, which must already be \
         populated for this project.\n\n\
         Warm the cache once (with network), then re-run code-ranker:\n    \
         cargo metadata --manifest-path {manifest} >/dev/null\n\
         (a prior `cargo build` / `cargo fetch` works too).\n\n\
         In CI: run code-ranker on the same image/cache as your build or test jobs, \
         where the cache is already warm.\n\n\
         Underlying cargo error: {err}",
        manifest = manifest.display(),
    )
}

fn version_string() -> Option<String> {
    which::which("rustc").ok()?;
    let out = log::timed("rustc --version", || {
        std::process::Command::new("rustc")
            .arg("--version")
            .output()
    })
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
                        // Omit when zero, like other metrics — files with no
                        // `unsafe` simply carry no key.
                        if let Some(u) = node.unsafe_count
                            && u > 0
                        {
                            attrs.insert("unsafe".to_string(), AttrValue::Int(u as i64));
                        }
                        if let Some(krate) = &node.crate_label {
                            attrs.insert("crate".to_string(), AttrValue::Str(krate.clone()));
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
                            if let Some(u) = node.unsafe_count
                                && u > 0
                            {
                                n.attrs
                                    .insert("unsafe".to_string(), AttrValue::Int(u as i64));
                            }
                            if let Some(krate) = &node.crate_label {
                                n.attrs
                                    .insert("crate".to_string(), AttrValue::Str(krate.clone()));
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
            EdgeKind::Super => "super",
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
