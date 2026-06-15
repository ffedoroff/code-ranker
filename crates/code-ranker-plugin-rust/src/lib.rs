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
mod rust_ts;

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

    fn metrics(&self, graph: &mut Graph) -> usize {
        // Each `.rs` file node is re-read (by its absolute-path `id`) and measured
        // by our `tree-sitter-rust` engine; `#[cfg(test)]` / `#[test]` items are
        // stripped first so metrics reflect production code only (their lines
        // become `tloc`).
        let mut annotated = 0;
        for node in &mut graph.nodes {
            if node.kind != "file" {
                continue;
            }
            let Ok(src) = std::fs::read(&node.id) else {
                continue;
            };
            if rust_file_metrics(node, &src) {
                annotated += 1;
            }
        }
        annotated
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

    fn roots(&self, _workspace: &Path) -> Vec<(String, String)> {
        rust_toolchain_roots()
    }

    fn metric_specs(
        &self,
        mut defaults: BTreeMap<String, AttributeSpec>,
    ) -> BTreeMap<String, AttributeSpec> {
        // Rust strips inline `#[cfg(test)]` / `#[test]` / `#[bench]` items before
        // measuring, so the LOC metrics count production code only — a nuance the
        // language-neutral default descriptions omit. Refine them for Rust.
        let rust_loc_note: &[(&str, &str)] = &[
            (
                "sloc",
                "Source lines of code — lines with at least one non-whitespace, non-comment character. Blank and comment-only lines are not counted. In Rust, lines inside `#[cfg(test)]` / `#[test]` items are excluded too, so this counts production code only (unlike `loc`, the raw file line count).",
            ),
            (
                "lloc",
                "Logical lines — counts statements, not physical lines. In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            ),
            (
                "cloc",
                "Comment-only lines (inline comments on code lines are not counted). In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            ),
            (
                "blank",
                "Empty or whitespace-only lines. In Rust, measured on production code only (inline `#[cfg(test)]` / `#[test]` tests are excluded, like `sloc`; their lines are `tloc`).",
            ),
        ];
        for (key, desc) in rust_loc_note {
            if let Some(spec) = defaults.get_mut(*key) {
                spec.description = Some((*desc).to_string());
            }
        }
        defaults
    }
}

/// The Rust/Cargo toolchain path roots used to shorten external node ids in the
/// snapshot: `cargo` (`$CARGO_HOME`), `registry` (the crates.io source dir),
/// `rustup` (`$RUSTUP_HOME`), and `rust-src` (the stdlib source under the active
/// sysroot). These are Rust-specific, so they live here in the Rust plugin rather
/// than in the language-agnostic orchestrator.
fn rust_toolchain_roots() -> Vec<(String, String)> {
    let mut roots = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();

    let cargo = std::env::var("CARGO_HOME").unwrap_or_else(|_| format!("{home}/.cargo"));
    let rustup = std::env::var("RUSTUP_HOME").unwrap_or_else(|_| format!("{home}/.rustup"));

    if !cargo.is_empty() {
        // Auto-detect crates.io registry hash dir (e.g. index.crates.io-<hash>).
        let registry_src = format!("{cargo}/registry/src");
        if let Ok(entries) = std::fs::read_dir(&registry_src) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("index.crates.io") {
                    roots.push(("registry".to_string(), format!("{registry_src}/{name}")));
                    break;
                }
            }
        }
        roots.push(("cargo".to_string(), cargo));
    }
    if !rustup.is_empty() {
        // Add rust-src root: sysroot/lib/rustlib/src/rust/library — shortens stdlib
        // paths from {rustup}/toolchains/.../library/... to {rust-src}/...
        if which::which("rustc").is_ok()
            && let Ok(out) = log::timed("rustc --print sysroot", || {
                std::process::Command::new("rustc")
                    .args(["--print", "sysroot"])
                    .output()
            })
            && out.status.success()
        {
            let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let rust_lib = format!("{sysroot}/lib/rustlib/src/rust/library");
            if std::path::Path::new(&rust_lib).exists() {
                roots.push(("rust-src".to_string(), rust_lib));
            }
        }
        roots.push(("rustup".to_string(), rustup));
    }
    roots
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

// ─────────────────────────────────────────────────────────────────────────────
// Complexity: strip inline tests, run the tree-sitter-rust engine, write metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Compute and write Rust complexity metrics for one file node from its source
/// bytes. `#[cfg(test)]` / `#[test]` / `#[bench]` items are stripped first (their
/// lines become `tloc`), then the in-tree `rust_ts` engine runs. Returns `true`
/// if metrics were written (`false` if the source did not parse).
fn rust_file_metrics(node: &mut Node, src: &[u8]) -> bool {
    let (prod, tloc) = strip_cfg_test(src);
    let Some(mut m) = rust_ts::compute(&prod) else {
        return false;
    };
    m.tloc = tloc as f64;
    code_ranker_graph::write_metrics(node, &m);
    true
}

/// True if any attribute gates an item to tests: `#[test]`, `#[bench]`, or
/// `#[cfg(test)]` / `#[cfg(all(test, …))]` / `#[cfg(any(test, …))]`. A `test`
/// **identifier** inside `cfg(...)` is what matches — `cfg(feature = "test")`
/// (a string literal) does not.
fn is_test_attr(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("test") || attr.path().is_ident("bench") {
        return true;
    }
    if attr.path().is_ident("cfg")
        && let syn::Meta::List(list) = &attr.meta
    {
        return tokens_have_test_ident(list.tokens.clone());
    }
    false
}

/// Recursively scan a token stream for a bare `test` identifier (descends into
/// `all(...)` / `any(...)` groups).
fn tokens_have_test_ident(ts: proc_macro2::TokenStream) -> bool {
    ts.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == "test",
        proc_macro2::TokenTree::Group(g) => tokens_have_test_ident(g.stream()),
        _ => false,
    })
}

/// Visitor collecting the 1-based, inclusive line ranges of test-only items
/// (`#[cfg(test)]` modules, `#[test]`/`#[cfg(test)]` fns), attribute line
/// included. It recurses into ordinary modules to catch nested test modules but
/// not into a test item it already captured.
#[derive(Default)]
struct TestSpans {
    ranges: Vec<(usize, usize)>,
}

impl TestSpans {
    fn record(&mut self, attrs: &[syn::Attribute], span: proc_macro2::Span) {
        use syn::spanned::Spanned;
        let start = attrs
            .iter()
            .map(|a| a.span().start().line)
            .chain(std::iter::once(span.start().line))
            .min()
            .unwrap_or(0);
        self.ranges.push((start, span.end().line));
    }
}

impl<'ast> syn::visit::Visit<'ast> for TestSpans {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        use syn::spanned::Spanned;
        if m.attrs.iter().any(is_test_attr) {
            self.record(&m.attrs, m.span());
        } else {
            syn::visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        use syn::spanned::Spanned;
        if f.attrs.iter().any(is_test_attr) {
            self.record(&f.attrs, f.span());
        }
    }
}

/// Step 1 of the Rust line accounting: remove `#[cfg(test)]` / `#[test]` /
/// `#[bench]` items so the production metrics (`sloc` / `cloc` / `blank` / `hk` /
/// complexity) are then measured on production code only. Returns the production
/// source **and** `tloc` — the number of test lines removed (the whole test
/// region: attribute, body, braces). Parse failures or no test items return the
/// source unchanged with `tloc = 0`.
fn strip_cfg_test(src: &[u8]) -> (Vec<u8>, usize) {
    use syn::visit::Visit;
    let Ok(text) = std::str::from_utf8(src) else {
        return (src.to_vec(), 0);
    };
    let Ok(file) = syn::parse_file(text) else {
        return (src.to_vec(), 0);
    };
    let mut spans = TestSpans::default();
    spans.visit_file(&file);
    if spans.ranges.is_empty() {
        return (src.to_vec(), 0);
    }
    let drop: std::collections::HashSet<usize> =
        spans.ranges.iter().flat_map(|&(s, e)| s..=e).collect();
    let tloc = drop.len();
    let mut out: String = text
        .lines()
        .enumerate()
        .filter(|(i, _)| !drop.contains(&(i + 1)))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    (out.into_bytes(), tloc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(src: &str) -> String {
        String::from_utf8(strip_cfg_test(src.as_bytes()).0).unwrap()
    }

    #[test]
    fn strips_cfg_test_module_with_its_attribute() {
        let out = strip(
            "pub fn prod() -> i32 {\n    1\n}\n\n\
             #[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t() { assert_eq!(prod(), 1); }\n}\n",
        );
        assert!(out.contains("pub fn prod"), "production kept: {out}");
        assert!(!out.contains("mod tests"), "test mod removed: {out}");
        assert!(
            !out.contains("#[cfg(test)]"),
            "the cfg attr line removed too: {out}"
        );
        assert!(!out.contains("fn t()"), "test fn removed: {out}");
    }

    #[test]
    fn strips_standalone_test_and_bench_fns() {
        let out = strip("fn prod() {}\n#[test]\nfn it_works() {}\n#[bench]\nfn b(_: &mut ()) {}\n");
        assert!(out.contains("fn prod"));
        assert!(
            !out.contains("it_works") && !out.contains("fn b("),
            "test/bench fns removed: {out}"
        );
    }

    #[test]
    fn keeps_non_test_cfg_and_similarly_named_items() {
        // `cfg(feature = "test")` is a string literal, not a `test` ident; a
        // `mod tests_data` is not gated. Both stay.
        let out = strip("#[cfg(feature = \"test\")]\npub mod gated {}\npub mod tests_data {}\n");
        assert!(out.contains("pub mod gated"), "feature-cfg kept: {out}");
        assert!(
            out.contains("tests_data"),
            "non-gated lookalike kept: {out}"
        );
    }

    #[test]
    fn strips_cfg_all_test_combinations() {
        let out = strip("fn p() {}\n#[cfg(all(test, feature = \"x\"))]\nmod t {}\n");
        assert!(out.contains("fn p"));
        assert!(!out.contains("mod t"), "cfg(all(test,…)) removed: {out}");
    }

    #[test]
    fn unchanged_without_tests_or_on_parse_error() {
        let prod = "pub fn a() {}\n";
        assert_eq!(
            strip_cfg_test(prod.as_bytes()),
            (prod.as_bytes().to_vec(), 0)
        );
        let broken = "@@@ not rust @@@";
        assert_eq!(
            strip_cfg_test(broken.as_bytes()),
            (broken.as_bytes().to_vec(), 0)
        );
    }

    #[test]
    fn tloc_counts_the_whole_removed_test_region() {
        // 4 lines removed: the #[cfg(test)] attr, `mod tests {`, the body line,
        // and the closing `}`.
        let src = "pub fn p() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let (_prod, tloc) = strip_cfg_test(src.as_bytes());
        assert_eq!(tloc, 4);
    }

    fn metric(node: &code_ranker_plugin_api::node::Node, key: &str) -> Option<f64> {
        match node.attrs.get(key) {
            Some(code_ranker_plugin_api::attrs::AttrValue::Int(v)) => Some(*v as f64),
            Some(code_ranker_plugin_api::attrs::AttrValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    /// Strip inline tests from `src`, run the in-tree Rust engine, write the
    /// metrics onto a fresh file node, and read one metric — the in-process
    /// building block for the metamorphic tests below. Handles `.rs` only.
    fn metric_of(_path: &str, src: &str, key: &str) -> Option<f64> {
        let (prod, tloc) = strip_cfg_test(src.as_bytes());
        let mut m = rust_ts::compute(&prod)?;
        m.tloc = tloc as f64;
        let mut node = code_ranker_plugin_api::node::Node {
            id: "t.rs".into(),
            kind: "file".into(),
            name: "t.rs".into(),
            parent: None,
            attrs: Default::default(),
        };
        code_ranker_graph::write_metrics(&mut node, &m);
        metric(&node, key)
    }

    // ---- Layer 1: metamorphic FP / FN matrix (see docs/metric-correctness.md) --
    //
    // Asserts the AST-Accurate principle across `metric × language × lexical
    // position × direction`: a control-flow / exit keyword appearing only as a
    // look-alike must NOT move the per-function metrics (no false positive); every
    // real construct form MUST be counted (no false negative). Pure in-process
    // parses — ~0 cost against the 20s budget. (LOC / Halstead are intentionally
    // NOT in the keyword-invariance set: a real comment line legitimately changes
    // `cloc`, a string legitimately adds Halstead operands — that is not an FP.)

    /// A Rust function carrying real branching (so all five per-function metrics
    /// are non-zero), with an optional doc-comment prefix and an optional
    /// statement injected into the body. Used to build FP-matrix variants.
    fn rs_src(doc: &str, body_inject: &str) -> String {
        format!(
            "{doc}fn f(a: i32, b: i32) -> i32 {{\n\
             {body_inject}    let g = |x: i32| x + 1;\n\
                 if a > 0 {{ return g(b); }}\n\
                 a + b\n\
             }}\n"
        )
    }

    // Per-language keyword look-alike guard set — the construct keywords/operators
    // a complexity (or `unsafe`) metric can key on. The FP matrix injects these
    // *only* as look-alikes and asserts no metric moves. This mirrors the
    // "Keyword look-alike guard set" in principles/rust/metrics.md, and
    // `rust_trigger_set_documented_in_spec` asserts the spec documents every entry
    // — so the two cannot drift. A superset of the analyzer's real triggers is
    // fine.
    const RUST_TRIGGERS: &[&str] = &[
        "if", "else", "match", "while", "for", "loop", "return", "unsafe", "&&", "||", "?",
    ];

    #[test]
    fn rust_complexity_fp_matrix() {
        // Every lexical position that could smuggle a keyword in as text. None may
        // change cyclomatic / cognitive / exits / args / closures vs the base.
        let base = rs_src("", "");
        let kw = RUST_TRIGGERS.join(" ");
        let positions: &[(&str, String)] = &[
            (
                "line comment",
                rs_src("", &format!("    // {kw} && || ?\n")),
            ),
            (
                "block comment",
                rs_src("", &format!("    /* {kw} && || ? */\n")),
            ),
            ("doc comment", rs_src(&format!("/// {kw}\n"), "")),
            (
                "string",
                rs_src("", &format!("    let _s = \"{kw} && || ?\";\n")),
            ),
            (
                "raw string",
                rs_src("", &format!("    let _r = r#\"{kw} && ||\"#;\n")),
            ),
            (
                "identifier",
                rs_src(
                    "",
                    "    let if_match_return_loop = 0; let _ = if_match_return_loop;\n",
                ),
            ),
            (
                "format string",
                rs_src("", "    let _f = format!(\"if {} while\", a);\n"),
            ),
            (
                "macro body",
                rs_src("", "    let _m = vec![\"if\", \"match\", \"while\"];\n"),
            ),
            (
                "raw identifier",
                rs_src("", "    let r#match = 1; let _ = r#match;\n"),
            ),
        ];
        for key in ["cyclomatic", "cognitive", "exits", "args", "closures"] {
            let want = metric_of("t.rs", &base, key);
            for (pos, src) in positions {
                assert_eq!(
                    metric_of("t.rs", src, key),
                    want,
                    "metric `{key}` moved when a keyword appeared only in: {pos}"
                );
            }
        }
    }

    #[test]
    fn cyclomatic_counts_every_branch_form() {
        // FN guard: every branch form the analyzer recognizes must raise
        // cyclomatic above a branch-free baseline. (Exact per-form increments are
        // the analyzer's rule — layer 4; here we only assert "detected".)
        let baseline =
            metric_of("t.rs", "fn f() -> i32 { 0 }\n", "cyclomatic").expect("baseline cyclomatic");
        let forms: &[(&str, &str)] = &[
            ("if", "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n"),
            (
                "else-if",
                "fn f(a: i32) -> i32 { if a > 0 { 1 } else if a < 0 { 2 } else { 3 } }\n",
            ),
            (
                "match",
                "fn f(a: i32) -> i32 { match a { 0 => 1, _ => 2 } }\n",
            ),
            (
                "while",
                "fn f(mut a: i32) -> i32 { while a > 0 { a -= 1; } a }\n",
            ),
            (
                "for",
                "fn f(a: i32) -> i32 { let mut s = 0; for i in 0..a { s += i; } s }\n",
            ),
            ("loop", "fn f() -> i32 { loop { break; } 0 }\n"),
            (
                "&&",
                "fn f(a: i32, b: i32) -> i32 { let _ = a > 0 && b > 0; 0 }\n",
            ),
            (
                "||",
                "fn f(a: i32, b: i32) -> i32 { let _ = a > 0 || b > 0; 0 }\n",
            ),
            ("?", "fn f() -> Option<i32> { let x = Some(1)?; Some(x) }\n"),
            (
                "if let",
                "fn f() -> i32 { if let Some(x) = Some(1) { x } else { 0 } }\n",
            ),
            (
                "while let",
                "fn f() -> i32 { let mut it = [1].into_iter(); let mut n = 0; while let Some(_) = it.next() { n += 1; } n }\n",
            ),
        ];
        for (name, src) in forms {
            let c = metric_of("t.rs", src, "cyclomatic")
                .unwrap_or_else(|| panic!("cyclomatic missing for `{name}`"));
            assert!(
                c > baseline,
                "branch form `{name}` not counted (cyclomatic {c} <= baseline {baseline})"
            );
        }
        // Magnitude anchor: one extra `if` adds exactly 1.
        let one = metric_of(
            "t.rs",
            "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n",
            "cyclomatic",
        )
        .unwrap();
        let two = metric_of(
            "t.rs",
            "fn f(a: i32) -> i32 { if a > 0 { 1 } else if a < 0 { 2 } else { 3 } }\n",
            "cyclomatic",
        )
        .unwrap();
        assert_eq!(two - one, 1.0, "one extra real `if` must add exactly 1");
    }

    #[test]
    fn rust_complexity_fn_per_metric() {
        // FN guard for the non-cyclomatic per-function metrics: a real construct
        // must surface the metric.
        let cognitive = metric_of(
            "t.rs",
            "fn f(a: i32, b: i32) -> i32 { if a > 0 { if b > 0 { 1 } else { 2 } } else { 3 } }\n",
            "cognitive",
        )
        .expect("cognitive present");
        assert!(cognitive > 0.0, "nested branches must raise cognitive");

        let exits = metric_of("t.rs", "fn f(a: i32) -> i32 { return a; }\n", "exits")
            .expect("exits present");
        assert!(exits >= 1.0, "a real `return` must be counted as an exit");

        let args = metric_of(
            "t.rs",
            "fn f(a: i32, b: i32, c: i32) -> i32 { a + b + c }\n",
            "args",
        )
        .expect("args present");
        assert!(
            args >= 3.0,
            "three parameters must count as >=3 args, got {args}"
        );

        let closures = metric_of(
            "t.rs",
            "fn f() -> i32 { let g = |x: i32| x + 1; g(1) }\n",
            "closures",
        )
        .expect("closures present");
        assert!(closures >= 1.0, "a real closure must be counted");
    }

    #[test]
    fn rust_only_complexity_fp_matrix() {
        // FP invariance for cyclomatic / cognitive, driven by Rust's documented
        // trigger set injected into comment / string positions.
        let check = |path: &str, base: &str, traps: &[String]| {
            for key in ["cyclomatic", "cognitive"] {
                let want = metric_of(path, base, key);
                for trap in traps {
                    assert_eq!(
                        metric_of(path, trap, key),
                        want,
                        "{path} metric `{key}` moved on a keyword look-alike"
                    );
                }
            }
        };

        let kw = RUST_TRIGGERS.join(" ");
        let base = "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n";
        check(
            "t.rs",
            base,
            &[
                format!("// {kw}\n{base}"),
                format!(
                    "fn f(a: i32) -> i32 {{ let _ = \"{kw}\"; if a > 0 {{ 1 }} else {{ 2 }} }}\n"
                ),
            ],
        );
    }

    #[test]
    fn rust_trigger_set_documented_in_spec() {
        // Lock-step guard: every keyword the FP matrix injects must be documented
        // in Rust's metrics spec, so the trigger list and the spec's "Keyword
        // look-alike guard set" cannot drift apart.
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let path = format!("{root}/principles/rust/metrics.md");
        let spec = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        for kw in RUST_TRIGGERS {
            assert!(
                spec.contains(&format!("`{kw}`")),
                "trigger `{kw}` is not documented in principles/rust/metrics.md — spec and FP test drifted"
            );
        }
    }

    // ---- Layer 2: generative tests (see docs/metric-correctness.md) ------------
    //
    // Generate programs with a KNOWN construct count, then assert the metric
    // equals ground truth across a combinatorial grid. Deterministic (no random
    // dependency, no flakiness) — proptest-style randomized fuzz is a later
    // nightly extension. Still pure in-process parses; the whole grid is ~ms.

    /// A Rust function with `noise` keyword-laden look-alike lines (a comment plus
    /// a string binding, neither a real construct) followed by `branches` real,
    /// independent `if` statements (each adds exactly 1 to cyclomatic).
    fn gen_rs(branches: usize, noise: usize) -> String {
        let mut body = String::new();
        for i in 0..noise {
            body.push_str(&format!(
                "    // if match while for loop return && || ? noise {i}\n"
            ));
            body.push_str(&format!(
                "    let _n{i} = \"if match while return && ||\";\n"
            ));
        }
        for i in 0..branches {
            body.push_str(&format!("    if x > {i} {{ let _ = {i}; }}\n"));
        }
        format!("fn f(x: i32) -> i32 {{\n{body}    0\n}}\n")
    }

    #[test]
    fn generative_cyclomatic_counts_branches_not_noise() {
        // Ground truth by construction: cyclomatic = baseline + (real `if` count),
        // independent of how many keyword look-alike lines surround it. Sweeps an
        // 8×8 grid of (branches, noise) — 64 generated programs.
        for noise in 0..8 {
            let base =
                metric_of("t.rs", &gen_rs(0, noise), "cyclomatic").expect("cyclomatic present");
            for branches in 0..8 {
                let cyc = metric_of("t.rs", &gen_rs(branches, noise), "cyclomatic")
                    .expect("cyclomatic present");
                assert_eq!(
                    cyc,
                    base + branches as f64,
                    "cyclomatic must add exactly 1 per real `if` and 0 per noise line \
                     (branches={branches}, noise={noise})"
                );
            }
        }
    }

    #[test]
    fn generative_complexity_invariant_to_noise() {
        // A fixed real structure (2 args, a closure, a branch, a `return`) with a
        // growing pile of keyword look-alikes around it. Every per-function metric
        // must stay exactly at its noise-free value — no false positive at any
        // noise level.
        let mk = |noise: usize| -> String {
            let mut body = String::new();
            for i in 0..noise {
                body.push_str(&format!("    // if match return unsafe && || {i}\n"));
                body.push_str(&format!("    let _n{i} = \"if match return && ||\";\n"));
            }
            format!(
                "fn f(a: i32, b: i32) -> i32 {{\n\
                 {body}    let g = |x: i32| x + 1;\n\
                     if a > 0 {{ return g(b); }}\n\
                     a + b\n\
                 }}\n"
            )
        };
        for key in ["cyclomatic", "cognitive", "exits", "args", "closures"] {
            let want = metric_of("t.rs", &mk(0), key);
            for noise in 1..10 {
                assert_eq!(
                    metric_of("t.rs", &mk(noise), key),
                    want,
                    "metric `{key}` moved at noise={noise} — keyword look-alikes leaked in"
                );
            }
        }
    }

    #[test]
    fn per_function_metrics_aggregate_over_child_functions() {
        // Regression for the whole "root-vs-sum" class: `write_metrics` once read
        // the ROOT space value for `cyclomatic` / `cognitive` / `exits` / `args` /
        // `closures`, which for a file is the vacuous root count (0, or 1 for
        // cyclomatic) — every file looked identical. The real signal lives in the
        // child function spaces, so each must be the SUM over them.
        //
        // `a` takes 2 args, nests two `if`s, and `return`s; `b` defines a 1-arg
        // closure. So the file must surface: cyclomatic (summed branches), a
        // non-zero cognitive (nesting), exits (the `return`), args (2 fn + 1
        // closure = 3), and closures (1).
        let src = "fn a(x: i32, y: i32) -> i32 { if x > 0 { if x > 1 { return x; } y } else { 3 } }\n\
                   fn b() -> i32 { let f = |z: i32| z + 1; f(2) }\n";
        // Each is summed over the child functions — well above the vacuous root
        // value, proving aggregation rather than a root-only read.
        let cyc = metric_of("t.rs", src, "cyclomatic").expect("cyclomatic present");
        assert!(cyc > 1.0, "cyclomatic should be summed, got {cyc}");
        let cog = metric_of("t.rs", src, "cognitive").expect("cognitive present");
        assert!(cog > 0.0, "cognitive should be summed, got {cog}");
        let exits = metric_of("t.rs", src, "exits").expect("exits present");
        assert!(exits >= 1.0, "exits should count the `return`, got {exits}");
        let args = metric_of("t.rs", src, "args").expect("args present");
        assert!(
            args >= 3.0,
            "args should sum fn (2) + closure (1), got {args}"
        );
        let closures = metric_of("t.rs", src, "closures").expect("closures present");
        assert!(
            closures >= 1.0,
            "closures should count the closure, got {closures}"
        );
    }

    // ---- Layer 3: asserted anchors (see docs/metric-correctness.md) -----------
    //
    // Layers 1 & 2 prove RELATIVE behaviour (noise-invariance, +1 per construct)
    // but never pin an ABSOLUTE value, so a uniform offset/scale bug (every count
    // shifted by +1, or doubled) would pass green. These anchors pin exact values
    // hand-derived from principles/rust/metrics.md, catching that scale class.

    #[test]
    fn complexity_absolute_anchors_hand_derived() {
        // Integer counting metrics, pinned to EXACT file-level values, hand-derived
        // from the spec's rules (metrics.md §cyclomatic / §exits,args,closures).
        //
        // These pin the analyzer-of-record's whole-file values (what we emit):
        //   • `cyclomatic` = the file unit's base path (1) + Σ over functions of
        //     (1 + branch points). Per-function McCabe (`V(G)=E−N+2P` = Σ over
        //     functions) is the theory; the analyzer adds the file unit on top and
        //     we emit it verbatim (it is also the value `mi` is computed from).
        //     `classify` = file 1 + fn 4 (base1+if+else-if+||) = 5.
        //   • `exits` = Σ over functions of (a value-returning `-> T` exit +
        //     explicit return/?). "Exit points" has no canonical theory, so the
        //     analyzer's rule is the source of truth (metrics.md §exits). The
        //     `-> i32` snippets below read 2 (the explicit return + the `-> T` exit).
        //   • `args` / `closures` / `cognitive` have no file-unit offset.
        // All pinned so any drift from the analyzer's output is caught.
        let classify = "fn classify(n: i32) -> &'static str {\n\
            \x20   if n < 0 { \"neg\" } else if n == 0 || n == 1 { \"small\" } else { \"big\" }\n\
            }\n";
        let two_closures =
            "fn f() { let g = |x: i32| x + 1; let h = |y: i32| y; let _ = (g, h); }\n";
        // (label, path, src, key, exact_expected)
        let cases: &[(&str, &str, &str, &str, f64)] = &[
            // file unit 1 + fn(base1 + if + else-if + ||) = 1 + 4 = 5.
            ("classify", "t.rs", classify, "cyclomatic", 5.0),
            // file unit 1 + fn(base1 + 1 if) = 1 + 2 = 3 (else is free).
            (
                "single if",
                "t.rs",
                "fn f(a: i32) -> i32 { if a > 0 { 1 } else { 2 } }\n",
                "cyclomatic",
                3.0,
            ),
            // 1 explicit return + 1 value-returning exit (`-> i32`) → 2.
            (
                "one return",
                "t.rs",
                "fn f() -> i32 { return 1; }\n",
                "exits",
                2.0,
            ),
            // 1 `?` + 1 value-returning exit (`-> Option`) → 2.
            (
                "one try op",
                "t.rs",
                "fn f() -> Option<i32> { let x = Some(1)?; Some(x) }\n",
                "exits",
                2.0,
            ),
            (
                "three params",
                "t.rs",
                "fn f(a: i32, b: i32, c: i32) -> i32 { a + b + c }\n",
                "args",
                3.0,
            ),
            ("two closures", "t.rs", two_closures, "closures", 2.0),
            ("two closure args", "t.rs", two_closures, "args", 2.0),
        ];
        let mut fails = Vec::new();
        for (label, path, src, key, want) in cases {
            match metric_of(path, src, key) {
                Some(got) if got == *want => {}
                other => fails.push(format!("{label}: {key} want {want}, got {other:?}")),
            }
        }
        assert!(
            fails.is_empty(),
            "failing integer anchors:\n{}",
            fails.join("\n")
        );
    }

    #[test]
    fn complexity_frozen_scale_anchors() {
        // Algorithm-specific metrics (cognitive nesting weights, Halstead
        // dictionaries, MI) cannot be hand-derived reliably, so they are FROZEN
        // anchors: values produced by `rust-code-analysis` for one fixed snippet,
        // verified once. Their job is to catch a uniform offset/scale regression
        // (a library bump that doubles `volume`, an MI formula edit) — not to
        // claim an independent ground truth. They change only when the underlying
        // algorithm changes, and that change should be deliberate.
        let classify = "fn classify(n: i32) -> &'static str {\n\
            \x20   if n < 0 { \"neg\" } else if n == 0 || n == 1 { \"small\" } else { \"big\" }\n\
            }\n";
        // (key, expected, abs_tolerance)
        let cases: &[(&str, f64, f64)] = &[
            ("cognitive", 4.0, 0.0),   // exact integer
            ("vocabulary", 18.0, 0.0), // η₁ + η₂, exact integer
            ("length", 28.0, 0.0),     // N₁ + N₂, exact integer
            ("volume", 116.757, 0.01), // length × log₂(vocabulary)
            ("effort", 875.684, 0.01), // difficulty × volume
            ("mi", 127.299, 0.01),     // maintainability index
            ("mi_sei", 108.463, 0.01), // SEI variant
        ];
        let mut fails = Vec::new();
        for (key, want, tol) in cases {
            match metric_of("t.rs", classify, key) {
                Some(got) if (got - *want).abs() <= *tol => {}
                other => fails.push(format!("{key}: want {want} (±{tol}), got {other:?}")),
            }
        }
        assert!(
            fails.is_empty(),
            "failing scale anchors:\n{}",
            fails.join("\n")
        );
    }

    #[test]
    fn declaration_only_file_emits_no_complexity() {
        // No functions → only the file unit space → cyclomatic is a vacuous 1 and
        // cognitive is 0. Both must be dropped (not shown as a meaningless "1"),
        // matching how `put` already drops cognitive's 0. Mirrors real files like
        // a clap CLI model or a type-definitions module.
        let src = "pub struct Cli { pub verbose: bool }\n\
                   pub enum Mode { A, B }\n";
        assert_eq!(
            metric_of("t.rs", src, "cyclomatic"),
            None,
            "a declaration-only file must not emit a vacuous cyclomatic"
        );
        assert_eq!(
            metric_of("t.rs", src, "cognitive"),
            None,
            "a declaration-only file must not emit cognitive"
        );
    }

    #[test]
    fn metric_specs_override_adds_rust_cfg_test_note() {
        // The neutral default descriptions carry no language nuance; the Rust
        // plugin re-adds the `#[cfg(test)]` LOC-exclusion note for sloc/lloc/
        // cloc/blank — so it appears only in Rust snapshots, never in py/js/ts.
        let defaults = code_ranker_graph::metric_specs().0;
        // sanity: the shared default is language-neutral
        assert!(
            !defaults["blank"]
                .description
                .as_deref()
                .unwrap_or("")
                .contains("#[cfg(test)]"),
            "the shared default must stay language-neutral"
        );

        let refined = RustPlugin.metric_specs(defaults);
        for key in ["sloc", "lloc", "cloc", "blank"] {
            let desc = refined[key].description.as_deref().unwrap_or("");
            assert!(
                desc.contains("#[cfg(test)]"),
                "Rust `{key}` description should note the cfg(test) exclusion"
            );
        }
    }
}
