//! Directory-analysis pipeline: run the plugin, the central complexity /
//! coupling / cycle passes, assemble the `LevelGraph`, and build the `Snapshot`.
//! Owns [`Analyzed`] (the shared result). Called only from `analyze::analyze_input`
//! (fan-in 1), so its necessarily-high fan-out stays cheap under Henry-Kafura.

use crate::cli::AnalyzeArgs;
use crate::{config, git, logger, plugin, presets};
use anyhow::{Context, Result};
use code_ranker_graph::level_graph::{LevelGraph, LevelUi};
use code_ranker_graph::snapshot::Snapshot;
use code_ranker_plugin_api::plugin::PluginInput;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Result of the shared analysis core, consumed by `check` and `report`. The
/// snapshot is either freshly analyzed (directory input) or loaded (snapshot input).
pub(crate) struct Analyzed {
    pub(crate) snapshot: Snapshot,
    pub(crate) violations: Vec<config::Violation>,
    /// Effective cycle-rule policy (for the current-values config dump).
    pub(crate) cycles: config::CycleRules,
    /// Effective rules (to recompute baseline violations for the regression gate).
    pub(crate) rules: config::RulesConfig,
    /// `[output.<fmt>]` config: per-format `path` template and `enabled` flag
    /// (CLI flags still win — resolved in `run_report`).
    pub(crate) output: config::OutputConfig,
}

/// Directory input: load config, run the plugin, annotate the graphs, collect
/// violations, and assemble the snapshot. Writes nothing.
pub(crate) fn analyze_directory(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<Analyzed> {
    let target = args
        .input
        .canonicalize()
        .with_context(|| format!("input not found: {}", args.input.display()))?;
    let cwd = std::env::current_dir()?;

    // A bad config (malformed file, unknown scope/metric, bad inline override) is a
    // hard error — silently falling back to defaults would drop the user's rules and
    // let `check` pass when it should fail (a false green for a CI gate).
    let loaded = config::load(
        &target,
        &args.config,
        &args.ignore_paths,
        cycle_rules,
        thresholds,
    )
    .context("configuration error")?;
    let cfg = loaded.config;

    let plugin_name = resolve_plugin(args.plugin.as_deref(), cfg.plugin.as_deref(), &target)?;

    let command = format!(
        "code-ranker {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    let input = PluginInput {
        ignore: cfg.ignore.paths.clone(),
        ignore_tests: cfg.ignore.tests,
        options: BTreeMap::new(),
    };

    // 1. Parse structure (absolute file-path ids).
    let mut timings = Vec::new();
    let t = logger::Timer::start("parse: structure");
    let (mut graph, levels) = plugin::analyze(&plugin_name, &target, &input)
        .with_context(|| format!("plugin '{plugin_name}' failed"))?;
    let file_count = graph.nodes.iter().filter(|n| n.kind == "file").count();
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: plugin_name.clone(),
        ms: t.finish_quiet(),
        detail: format!("{} nodes from {} files", graph.nodes.len(), file_count),
    });

    // 2. Central complexity pass (reads files by their absolute id).
    let t = logger::Timer::start("complexity");
    let annotated = code_ranker_complexity::annotate(&mut graph);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: "complexity".into(),
        ms: t.finish_quiet(),
        detail: format!("{annotated} nodes annotated"),
    });

    // 3. Canonicalize structure, then relativize ids against detected roots.
    let t = logger::Timer::start("projection");
    code_ranker_graph::finalize::finalize_graph(&mut graph);
    let mut roots = detect_roots();
    roots.insert("target".to_string(), target.display().to_string());
    code_ranker_graph::relativize::relativize_graph(&mut graph, &target, &roots);

    // 4. Apply ignore filters (tokenized ids), then compute the derived data.
    config::apply_ignore(&mut graph, &cfg.ignore, &target)?;

    let level_spec = levels.into_iter().find(|l| l.name == "files");
    let flow_kinds = flow_kinds(level_spec.as_ref());
    // Cycles, fan-in/HK and the drawn map all run on the same flow edges. A
    // `pub use` re-export is a facade, not a dependency, so the Rust plugin marks
    // `reexports` non-flow (`EdgeKindSpec.flow = false`) — it never reaches any of
    // these and re-export hubs (lib.rs / mod.rs) cannot fabricate cycles.
    let mut cycles = code_ranker_graph::cycles::annotate_cycles(&mut graph, &flow_kinds);
    config::apply_cycle_rules(&mut cycles, &mut graph.nodes, &cfg.rules.cycles);
    code_ranker_graph::hk::annotate_hk(&mut graph, &flow_kinds);
    let stats = code_ranker_graph::stats::compute_stats(&graph);

    let edge_count = graph.edges.len();
    let node_count = graph.nodes.len();
    let thresholds = plugin::thresholds(&plugin_name);
    let level = assemble_level(level_spec, graph, cycles, stats, thresholds);
    prune_unused_roots(&level, &mut roots);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: "projection".into(),
        ms: t.finish_quiet(),
        detail: format!("nodes={node_count} edges={edge_count}"),
    });

    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), level);

    let violations = config::check_violations(&graphs, &cfg.rules);

    let git = git::collect(
        &target,
        &git::GitOverride {
            branch: args.git_branch.clone(),
            commit: args.git_commit.clone(),
            dirty_files: args.git_dirty_files,
            origin: args.git_origin.clone(),
        },
    );

    let mut versions = BTreeMap::new();
    versions.insert(
        "code-ranker".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    for (k, v) in plugin::versions(&plugin_name, &target, &input) {
        versions.insert(k, v);
    }

    let lang = presets::principle_lang(&plugin_name);
    let presets = plugin::presets(&plugin_name, presets::default_presets(&lang), &input);

    let snapshot = Snapshot::new(
        command,
        cwd.display().to_string(),
        target.display().to_string(),
        plugin_name,
        loaded.source_file,
        versions,
        roots,
        git,
        timings,
        graphs,
        presets,
    );

    Ok(Analyzed {
        snapshot,
        violations,
        cycles: cfg.rules.cycles,
        rules: cfg.rules,
        output: cfg.output,
    })
}

/// The set of edge kinds that carry information flow at this level (read from
/// `EdgeKindSpec.flow`). Cycles and coupling count only these.
fn flow_kinds(level: Option<&code_ranker_plugin_api::level::Level>) -> HashSet<String> {
    match level {
        Some(l) => l
            .edge_kinds
            .iter()
            .filter(|(_, spec)| spec.flow)
            .map(|(k, _)| k.clone())
            .collect(),
        None => HashSet::new(),
    }
}

/// Assemble one [`LevelGraph`]: merge the plugin's structural attribute specs
/// with the centrally-produced complexity + coupling specs, prune them (and the
/// edge kinds / groups) to what is actually present, and attach the graph,
/// cycles and stats.
fn assemble_level(
    level_spec: Option<code_ranker_plugin_api::level::Level>,
    graph: code_ranker_plugin_api::graph::Graph,
    cycles: Vec<code_ranker_graph::level_graph::CycleGroup>,
    stats: BTreeMap<String, code_ranker_plugin_api::attrs::AttrValue>,
    thresholds: BTreeMap<String, code_ranker_plugin_api::level::Thresholds>,
) -> LevelGraph {
    use std::collections::BTreeSet;

    let spec = level_spec.unwrap_or_else(|| code_ranker_plugin_api::level::Level {
        name: "files".into(),
        edge_kinds: BTreeMap::new(),
        node_attributes: BTreeMap::new(),
        edge_attributes: BTreeMap::new(),
        attribute_groups: BTreeMap::new(),
        node_kinds: BTreeMap::new(),
        cycle_kinds: BTreeMap::new(),
        grouping: None,
    });

    // Master node-attribute dictionary = structural (plugin) + computed.
    let mut node_attributes = spec.node_attributes;
    let (metric_specs, metric_groups) = code_ranker_complexity::metric_specs();
    let (coupling_specs, coupling_groups) = code_ranker_graph::coupling_specs();
    node_attributes.extend(metric_specs);
    node_attributes.extend(coupling_specs);
    let mut attribute_groups = spec.attribute_groups;
    attribute_groups.extend(metric_groups);
    attribute_groups.extend(coupling_groups);

    // Overlay language-calibrated thresholds onto the matching specs.
    for (key, th) in thresholds {
        if let Some(s) = node_attributes.get_mut(&key) {
            s.thresholds = Some(th);
        }
    }

    // The node-attribute dictionary keeps every key that exists in the JSON —
    // present on any node, external included — so the viewer can still label it
    // (e.g. external-node `path`/`version` shown in the diagram detail panel).
    let present_node_keys: BTreeSet<&str> = graph
        .nodes
        .iter()
        .flat_map(|n| n.attrs.keys().map(String::as_str))
        .collect();
    node_attributes.retain(|k, _| present_node_keys.contains(k.as_str()));

    // The `ui` lists, however, are filtered to keys present on at least one
    // *internal* (non-external) node. Those lists drive rendering surfaces
    // (table, summary, sort) that never show external rows (see `isExternalNode`
    // in schema.js); a metric living only on external nodes would otherwise be
    // promised in a list but never rendered. A node is external when it carries
    // `external: true` or its kind spec is marked external.
    let is_external = |n: &code_ranker_plugin_api::node::Node| -> bool {
        matches!(
            n.attrs.get("external"),
            Some(code_ranker_plugin_api::attrs::AttrValue::Bool(true))
        ) || spec
            .node_kinds
            .get(&n.kind)
            .and_then(|k| k.external)
            .unwrap_or(false)
    };
    let present_internal_keys: BTreeSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| !is_external(n))
        .flat_map(|n| n.attrs.keys().map(String::as_str))
        .collect();

    // Prune edge attributes to keys present on at least one edge.
    let present_edge_keys: BTreeSet<&str> = graph
        .edges
        .iter()
        .flat_map(|e| e.attrs.keys().map(String::as_str))
        .collect();
    let mut edge_attributes = spec.edge_attributes;
    edge_attributes.retain(|k, _| present_edge_keys.contains(k.as_str()));

    // Prune edge kinds to kinds present on at least one edge.
    let present_edge_kinds: BTreeSet<&str> = graph.edges.iter().map(|e| e.kind.as_str()).collect();
    let mut edge_kinds = spec.edge_kinds;
    edge_kinds.retain(|k, _| present_edge_kinds.contains(k.as_str()));

    // Prune groups to those referenced by a surviving node attribute.
    let referenced_groups: BTreeSet<&str> = node_attributes
        .values()
        .filter_map(|s| s.group.as_deref())
        .collect();
    attribute_groups.retain(|k, _| referenced_groups.contains(k.as_str()));

    // Prune node kinds to kinds actually present on nodes.
    let present_node_kinds: BTreeSet<&str> = graph.nodes.iter().map(|n| n.kind.as_str()).collect();
    let mut node_kinds = spec.node_kinds;
    node_kinds.retain(|k, _| present_node_kinds.contains(k.as_str()));

    // Prune cycle kinds to kinds actually present in the cycle groups.
    let present_cycle_kinds: BTreeSet<&str> = cycles.iter().map(|c| c.kind.as_str()).collect();
    let mut cycle_kinds = spec.cycle_kinds;
    cycle_kinds.retain(|k, _| present_cycle_kinds.contains(k.as_str()));

    let ui = build_ui(&node_attributes, &present_internal_keys, spec.grouping);

    LevelGraph {
        edge_kinds,
        node_attributes,
        edge_attributes,
        attribute_groups,
        node_kinds,
        cycle_kinds,
        nodes: graph.nodes,
        edges: graph.edges,
        cycles,
        stats,
        ui,
    }
}

/// Curated metric orders (the historical UI vocabulary). The orchestrator
/// filters each to the attributes actually present, so the viewer reads the
/// order from data and hardcodes none of it.
const UI_COLUMNS: &[&str] = &[
    "kind",
    "cycle",
    "sloc",
    "hk",
    "fan_in",
    "fan_out",
    "volume",
    "bugs",
    "effort",
    "time",
    "length",
    "vocabulary",
    "cyclomatic",
    "cognitive",
    "mi",
    "mi_sei",
    "lloc",
    "cloc",
    "blank",
    "tloc",
];
const UI_SUMMARY: &[&str] = &[
    "cyclomatic",
    "cognitive",
    "sloc",
    "mi",
    "mi_sei",
    "volume",
    "bugs",
    "effort",
    "time",
    "length",
    "vocabulary",
    "fan_in",
    "fan_out",
    "hk",
    "lloc",
    "cloc",
    "blank",
    "tloc",
];
const UI_SORT: &[&str] = &[
    "hk",
    "sloc",
    "fan_out",
    "fan_in",
    "cyclomatic",
    "cognitive",
    "items",
    "cycle",
];
const UI_SIZE: &[&str] = &["loc", "hk"];
const UI_CARD: &[&str] = &["hk", "sloc"];

/// Build the `ui` block: keep the canonical order, drop anything not present on
/// an internal node (`present_internal_keys`) — external-only keys stay in the
/// dictionary but never reach a render list. `kind` is always a column; `cycle`
/// is a column/sort metric only when it survived pruning.
fn build_ui(
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    present_internal_keys: &std::collections::BTreeSet<&str>,
    grouping: Option<code_ranker_plugin_api::level::Grouping>,
) -> LevelUi {
    let has = |k: &str| k == "kind" || present_internal_keys.contains(k);
    let pick = |list: &[&str]| -> Vec<String> {
        list.iter()
            .filter(|k| has(k))
            .map(|k| k.to_string())
            .collect()
    };
    let sort_metrics = pick(UI_SORT);
    let default_sort = if sort_metrics.iter().any(|m| m == "hk") {
        Some("hk".to_string())
    } else {
        sort_metrics.first().cloned()
    };
    // Keep the grouping only if it is usable: a `key` must reference an attribute
    // that survived pruning; a `function` is passed through. Otherwise drop it so
    // the viewer falls back to its default `dir` grouper.
    let grouping = grouping.filter(|g| match &g.key {
        Some(k) => node_attributes.contains_key(k),
        None => g.function.is_some(),
    });
    LevelUi {
        default_sort,
        sort_metrics,
        size_metrics: pick(UI_SIZE),
        card_metrics: pick(UI_CARD),
        columns: pick(UI_COLUMNS),
        summary_metrics: pick(UI_SUMMARY),
        grouping,
    }
}

/// Resolve the plugin name: explicit `--plugin` > config `plugin` > auto-detect.
/// A value of `auto` (or absence) triggers project-marker detection.
fn resolve_plugin(arg: Option<&str>, cfg: Option<&str>, workspace: &Path) -> Result<String> {
    if let Some(p) = arg
        && p != "auto"
    {
        return Ok(p.to_string());
    }
    if let Some(p) = cfg
        && p != "auto"
    {
        return Ok(p.to_string());
    }
    plugin::detect(workspace, &PluginInput::default())
}

fn detect_roots() -> BTreeMap<String, String> {
    let mut roots = BTreeMap::new();
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
                    roots.insert("registry".to_string(), format!("{registry_src}/{name}"));
                    break;
                }
            }
        }
        roots.insert("cargo".to_string(), cargo);
    }
    if !rustup.is_empty() {
        // Add rust-src root: sysroot/lib/rustlib/src/rust/library
        // This shortens stdlib paths from {rustup}/toolchains/.../library/... to {rust-src}/...
        if let Ok(out) = logger::timed("rustc --print sysroot", || {
            std::process::Command::new("rustc")
                .args(["--print", "sysroot"])
                .output()
        }) && out.status.success()
        {
            let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let rust_lib = format!("{sysroot}/lib/rustlib/src/rust/library");
            if std::path::Path::new(&rust_lib).exists() {
                roots.insert("rust-src".to_string(), rust_lib);
            }
        }
        roots.insert("rustup".to_string(), rustup);
    }
    roots
}

/// Remove named roots whose `{name}` token does not appear in any node id or
/// path after relativization. `target` is always kept (it names the analyzed
/// project even when every node sits directly under it). This keeps the
/// snapshot header free of roots that are irrelevant to the analyzed language
/// (e.g. the Rust toolchain roots in a JS/TS/Python snapshot).
fn prune_unused_roots(level: &LevelGraph, roots: &mut BTreeMap<String, String>) {
    let mut used: HashSet<String> = HashSet::new();
    used.insert("target".to_string());
    for node in &level.nodes {
        let path_attr = match node.attrs.get("path") {
            Some(code_ranker_plugin_api::attrs::AttrValue::Str(p)) => p.as_str(),
            _ => "",
        };
        for name in roots.keys() {
            let token = format!("{{{name}}}");
            if node.id.contains(&token) || path_attr.contains(&token) {
                used.insert(name.clone());
            }
        }
    }
    roots.retain(|name, _| used.contains(name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_plugin_by_single_marker() {
        let cases = vec![
            ("Cargo.toml", "rust"),
            ("pyproject.toml", "python"),
            ("setup.py", "python"),
            ("package.json", "javascript"),
            ("tsconfig.json", "typescript"),
        ];
        for (marker, expected) in cases {
            let d = tempfile::tempdir().unwrap();
            fs::write(d.path().join(marker), "").unwrap();
            assert_eq!(
                plugin::detect(d.path(), &PluginInput::default()).unwrap(),
                expected,
                "marker {marker}"
            );
        }
    }

    #[test]
    fn detect_plugin_errors_on_ambiguous_or_empty() {
        let amb = tempfile::tempdir().unwrap();
        fs::write(amb.path().join("Cargo.toml"), "").unwrap();
        fs::write(amb.path().join("package.json"), "").unwrap();
        let err = format!(
            "{:#}",
            plugin::detect(amb.path(), &PluginInput::default()).unwrap_err()
        );
        assert!(err.contains("multiple"), "ambiguous error: {err}");

        let empty = tempfile::tempdir().unwrap();
        let err = format!(
            "{:#}",
            plugin::detect(empty.path(), &PluginInput::default()).unwrap_err()
        );
        assert!(err.contains("no project marker"), "empty error: {err}");
    }

    #[test]
    fn resolve_plugin_precedence_explicit_then_config_then_auto() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(
            resolve_plugin(Some("rust"), Some("javascript"), d.path()).unwrap(),
            "rust",
            "explicit --plugin wins"
        );
        assert_eq!(
            resolve_plugin(None, Some("rust"), d.path()).unwrap(),
            "rust",
            "config wins over auto-detect"
        );
        assert_eq!(
            resolve_plugin(Some("auto"), None, d.path()).unwrap(),
            "python",
            "explicit auto -> detect"
        );
        assert_eq!(
            resolve_plugin(None, None, d.path()).unwrap(),
            "python",
            "no plugin -> detect"
        );
    }
}
