//! Directory-analysis pipeline: run the plugin, the central complexity /
//! coupling / cycle passes, assemble the `LevelGraph`, and build the `Snapshot`.
//! Owns [`Analyzed`] (the shared result). Called only from `analyze::analyze_input`
//! (fan-in 1), so its necessarily-high fan-out stays cheap under Henry-Kafura.

use crate::cli::AnalyzeArgs;
use crate::{config, git, logger, plugin};
use anyhow::{Context, Result};
use code_ranker_graph::level_graph::{LevelGraph, LevelUi};
use code_ranker_graph::snapshot::Snapshot;
use code_ranker_plugin_api::plugin::PluginInput;
use std::collections::{BTreeMap, HashSet};

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

    let plugin_name =
        plugin::resolve_plugin(args.plugin.as_deref(), cfg.plugin.as_deref(), &target)?;

    let command = format!(
        "code-ranker {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    let input = PluginInput {
        ignore: cfg.ignore.paths.clone(),
        ignore_tests: cfg.ignore.tests,
        gitignore: cfg.ignore.gitignore,
        ignore_files: cfg.ignore.ignore_files,
        hidden: cfg.ignore.hidden,
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

    // 2. Complexity pass: the active plugin annotates its own file nodes with
    //    per-language metrics (behind the `LanguagePlugin` trait — no central
    //    by-extension dispatcher). Reads files by their absolute id.
    let t = logger::Timer::start("complexity");
    let annotated = plugin::annotate_metrics(&plugin_name, &mut graph);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: "complexity".into(),
        ms: t.finish_quiet(),
        detail: format!("{annotated} nodes annotated"),
    });

    // 3. Canonicalize structure, then relativize ids against detected roots.
    //    The active plugin contributes its own language/toolchain roots (e.g. the
    //    Rust plugin's cargo/registry/rustup/rust-src); the orchestrator only owns
    //    the generic `target` root — no language leaks into this central step.
    let t = logger::Timer::start("projection");
    code_ranker_graph::finalize::finalize_graph(&mut graph);
    let mut roots: BTreeMap<String, String> =
        plugin::roots(&plugin_name, &target).into_iter().collect();
    roots.insert("target".to_string(), target.display().to_string());

    // Optional `functions` level (off by default): the plugin builds sub-file
    // metric nodes (absolute ids) which we merge in so relativization rewrites
    // their ids/parents alongside the files, then split back out — the `files`
    // graph and its goldens stay untouched.
    let want_functions = cfg.levels.functions;
    if want_functions {
        let fns = plugin::function_units(&plugin_name, &graph);
        graph.nodes.extend(fns);
    }
    code_ranker_graph::relativize::relativize_graph(&mut graph, &target, &roots);
    let mut fn_nodes: Vec<code_ranker_plugin_api::node::Node> = Vec::new();
    if want_functions {
        graph.nodes.retain(|n| {
            if n.id.contains('#') {
                fn_nodes.push(n.clone());
                false
            } else {
                true
            }
        });
    }

    // 4. Apply ignore filters (tokenized ids), then compute the derived data.
    config::apply_ignore(&mut graph, &cfg.ignore, &target)?;

    // Drop function nodes whose file was ignored above (keep the two in step).
    if want_functions {
        let file_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        fn_nodes.retain(|n| n.parent.as_deref().is_some_and(|p| file_ids.contains(p)));
    }

    let mut levels = levels;
    let fn_level_spec = levels
        .iter()
        .position(|l| l.name == "functions")
        .map(|i| levels.remove(i));
    let level_spec = levels.into_iter().find(|l| l.name == "files");
    let flow_kinds = flow_kinds(level_spec.as_ref());
    // Cycles, fan-in/HK and the drawn map all run on the same flow edges. A
    // `pub use` re-export is a facade, not a dependency, so the Rust plugin marks
    // `reexports` non-flow (`EdgeKindSpec.flow = false`) — it never reaches any of
    // these and re-export hubs (lib.rs / mod.rs) cannot fabricate cycles.
    let mut cycles = code_ranker_graph::cycles::annotate_cycles(&mut graph, &flow_kinds);
    config::apply_cycle_rules(&mut cycles, &mut graph.nodes, &cfg.rules.cycles);
    code_ranker_graph::hk::annotate_hk(&mut graph, &flow_kinds);

    // User-defined declarative metrics: evaluate each `[metrics.<key>]` CEL
    // formula. Node-scope metrics are written onto every internal node (built-in
    // attributes — including the just-computed coupling — are inputs); graph-scope
    // (aggregate) metrics are reduced over the whole node set into `stats` below.
    // Empty registry → no-op, so the default output (and its goldens) is unchanged.
    let mut custom_specs: BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec> =
        BTreeMap::new();
    let engine = if cfg.metrics.is_empty() {
        None
    } else {
        let engine = code_ranker_graph::registry::Engine::compile(&cfg.metrics)
            .context("compiling [metrics] formulas")?;
        for node in &mut graph.nodes {
            if node.kind == "external" {
                continue;
            }
            code_ranker_graph::apply_to_node(node, &cfg.metrics, &engine);
        }
        // Only node-scope metrics become node-attribute columns; graph-scope keys
        // never sit on a node, so they would be pruned anyway.
        custom_specs = cfg
            .metrics
            .iter()
            .filter(|(_, d)| d.scope == code_ranker_graph::Scope::Node)
            .map(|(k, d)| (k.clone(), d.to_attribute_spec()))
            .collect();
        Some(engine)
    };

    // The active plugin's report-list patches (table columns / card / JSON
    // stats), applied over the global catalog lists below.
    // The report-list patches applied over the catalog lists, in order: the
    // language's `[report]` (from `<lang>.toml`), then the project's `[report]`
    // (from `code-ranker.toml`) — so a project can surface its own metrics.
    let report_overrides = [
        plugin::report_overrides(&plugin_name),
        code_ranker_plugin_api::list_override::report_override_section(&cfg.report),
    ];

    // Stat keys are data-driven: tier-2 metrics from the registry plus the
    // coupling metrics (computed by the graph passes above), then patched by the
    // language's `[report].stats` (e.g. Rust adds `unsafe`).
    let mut stat_keys = code_ranker_graph::stat_keys();
    stat_keys.extend([
        "fan_in".to_string(),
        "fan_out".to_string(),
        "hk".to_string(),
    ]);
    let stat_keys = report_overrides
        .iter()
        .fold(stat_keys, |acc, ov| ov.stats.apply(&acc));
    let mut stats = code_ranker_graph::stats::compute_stats(&graph, &stat_keys);

    // Graph-scope aggregates → merged into the stats block (e.g. a user's
    // `cyclomatic_p90 = agg('cyclomatic','p90','not_empty')`).
    if let Some(engine) = &engine
        && engine.has_graph_metrics()
    {
        let omit_at = registry_omit_at(&plugin_name, &cfg.metrics);
        let rows: Vec<BTreeMap<String, f64>> = graph
            .nodes
            .iter()
            .filter(|n| n.kind != "external")
            .map(numeric_attrs)
            .collect();
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for r in &rows {
            keys.extend(r.keys().cloned());
        }
        let keys: Vec<String> = keys.into_iter().collect();
        let pops = code_ranker_graph::Populations::build(&rows, &keys, &omit_at);
        for (k, v) in engine.eval_graph(&pops) {
            stats.insert(k, code_ranker_graph::num_attr(v));
        }
    }

    // Warn on any declared metric that produced no value across the whole
    // project. Catches the otherwise-silent failure mode — a formula that errors
    // on every node (e.g. a misspelled input key resolves to nothing) — so a
    // typo'd metric doesn't just vanish without a trace.
    for (key, def) in &cfg.metrics {
        let present = match def.scope {
            code_ranker_graph::Scope::Graph => stats.contains_key(key),
            code_ranker_graph::Scope::Node => graph
                .nodes
                .iter()
                .any(|n| n.kind != "external" && n.attrs.contains_key(key)),
        };
        if !present {
            logger::info(&format!(
                "⚠ metric `{key}` produced no value on any node — check its formula \
                 (a misspelled input key?) or whether it is always at its no-signal value",
            ));
        }
    }

    let edge_count = graph.edges.len();
    let node_count = graph.nodes.len();
    let thresholds = plugin::thresholds(&plugin_name);
    let level = assemble_level(
        level_spec,
        graph,
        cycles,
        stats,
        thresholds,
        &custom_specs,
        &plugin_name,
        &report_overrides,
    );
    prune_unused_roots(&level, &mut roots);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: "projection".into(),
        ms: t.finish_quiet(),
        detail: format!("nodes={node_count} edges={edge_count}"),
    });

    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), level);

    // Assemble the optional `functions` level from the split-out sub-file nodes.
    // Reuses the same assembler: metric specs are merged and pruned to the keys
    // present on function nodes (coupling specs drop out — functions carry none).
    if want_functions && !fn_nodes.is_empty() {
        let fn_graph = code_ranker_plugin_api::graph::Graph {
            nodes: fn_nodes,
            edges: Vec::new(),
        };
        let fn_level = assemble_level(
            fn_level_spec,
            fn_graph,
            Vec::new(),
            BTreeMap::new(),
            plugin::thresholds(&plugin_name),
            &custom_specs,
            &plugin_name,
            &report_overrides,
        );
        graphs.insert("functions".to_string(), fn_level);
    }

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

    // Plugin catalog presets, then the project's own (`[presets.<ID>]`): a
    // same-id project preset overrides the plugin's, a new id appends. So a
    // project can recommend / scorecard on its custom metric.
    let presets = merge_project_presets(plugin::presets(&plugin_name, &input), &cfg.presets);

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

/// A node's numeric attributes as `f64` (the inputs an aggregate reduces over).
fn numeric_attrs(node: &code_ranker_plugin_api::node::Node) -> BTreeMap<String, f64> {
    use code_ranker_plugin_api::attrs::AttrValue;
    node.attrs
        .iter()
        .filter_map(|(k, v)| match v {
            AttrValue::Int(i) => Some((k.clone(), *i as f64)),
            AttrValue::Float(f) => Some((k.clone(), *f)),
            _ => None,
        })
        .collect()
}

/// Merge the project's `[presets.<ID>]` over the plugin catalog: a same-id project
/// preset replaces the plugin's (in place, keeping catalog order), a new id is
/// appended. So a project can recommend / scorecard on its own custom metric.
fn merge_project_presets(
    mut catalog: Vec<code_ranker_plugin_api::plugin::Preset>,
    project: &BTreeMap<String, config::model::PresetDef>,
) -> Vec<code_ranker_plugin_api::plugin::Preset> {
    for (id, def) in project {
        let p = def.to_preset(id);
        match catalog.iter_mut().find(|e| e.id == p.id) {
            Some(existing) => *existing = p,
            None => catalog.push(p),
        }
    }
    catalog
}

/// The `omit_at` (no-signal floor) of every metric key, so an aggregate's `all`
/// population counts a missing value at the right floor (`0` for most, `1` for
/// `cyclomatic`). Built from the central + plugin-refined + coupling specs, then
/// the user's own metric defs.
fn registry_omit_at(
    plugin_name: &str,
    custom: &BTreeMap<String, code_ranker_graph::MetricDef>,
) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    let (specs, _) = code_ranker_graph::metric_specs();
    for (k, s) in plugin::metric_specs(plugin_name, specs) {
        m.insert(k, s.omit_at);
    }
    let (coupling, _) = code_ranker_graph::coupling_specs();
    for (k, s) in coupling {
        m.insert(k, s.omit_at);
    }
    for (k, d) in custom {
        m.insert(k.clone(), d.omit_at);
    }
    m
}

/// Assemble one [`LevelGraph`]: merge the plugin's structural attribute specs
/// with the centrally-produced complexity + coupling specs, prune them (and the
/// edge kinds / groups) to what is actually present, and attach the graph,
/// cycles and stats.
#[allow(clippy::too_many_arguments)]
fn assemble_level(
    level_spec: Option<code_ranker_plugin_api::level::Level>,
    graph: code_ranker_plugin_api::graph::Graph,
    cycles: Vec<code_ranker_graph::level_graph::CycleGroup>,
    stats: BTreeMap<String, code_ranker_plugin_api::attrs::AttrValue>,
    thresholds: BTreeMap<String, code_ranker_plugin_api::level::Thresholds>,
    custom_specs: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    plugin_name: &str,
    report_overrides: &[code_ranker_plugin_api::report::ReportOverride],
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
    // Language-neutral default metric specs, refined by the active plugin (e.g.
    // Rust adds the `#[cfg(test)]` nuance to the LOC descriptions).
    let (default_metric_specs, metric_groups) = code_ranker_graph::metric_specs();
    let metric_specs = plugin::metric_specs(plugin_name, default_metric_specs);
    let (coupling_specs, coupling_groups) = code_ranker_graph::coupling_specs();
    node_attributes.extend(metric_specs);
    node_attributes.extend(coupling_specs);
    // User-defined declarative metrics render as first-class columns; built-ins
    // win a key collision (a user cannot shadow a core metric's spec).
    for (key, spec) in custom_specs {
        node_attributes
            .entry(key.clone())
            .or_insert_with(|| spec.clone());
    }
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

    let ui = build_ui(
        &node_attributes,
        &present_internal_keys,
        spec.grouping,
        report_overrides,
    );

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

/// Build the `ui` block from the data-driven view sections (`[tableview]` /
/// `[cardview]`), dropping anything not present on an internal node
/// (`present_internal_keys`) — external-only keys stay in the dictionary but
/// never reach a render list. `kind` is always a column.
fn build_ui(
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    present_internal_keys: &std::collections::BTreeSet<&str>,
    grouping: Option<code_ranker_plugin_api::level::Grouping>,
    report_overrides: &[code_ranker_plugin_api::report::ReportOverride],
) -> LevelUi {
    let v = code_ranker_graph::views();
    let has = |k: &str| k == "kind" || present_internal_keys.contains(k);
    let pick =
        |list: &[String]| -> Vec<String> { list.iter().filter(|k| has(k)).cloned().collect() };

    // Apply the report patches (language then project) over the catalog lists in
    // order, then prune to keys present on an internal node.
    let cols_base = report_overrides
        .iter()
        .fold(v.columns.clone(), |acc, ov| ov.columns.apply(&acc));
    let card_base = report_overrides
        .iter()
        .fold(v.featured.clone(), |acc, ov| ov.card.apply(&acc));
    let size_base = report_overrides
        .iter()
        .fold(v.size.clone(), |acc, ov| ov.size.apply(&acc));
    let filter_base = report_overrides
        .iter()
        .fold(v.filter.clone(), |acc, ov| ov.filter.apply(&acc));
    let columns = pick(&cols_base);
    let card_metrics = pick(&card_base);
    // Map controls: prune to keys present on an internal node. `cycle` is a
    // string attribute (a cycle kind), valid as a filter but never a size mode.
    let size_metrics = pick(&size_base);
    let filter_metrics = pick(&filter_base);
    // Default sort: a signed-rank list (order = priority, leading `-` =
    // descending). Strip the sign and pick the first key present. Every column
    // stays sortable in the UI — this only sets the opening order.
    let default_sort = v
        .default_sort
        .iter()
        .map(|s| s.strip_prefix('-').unwrap_or(s))
        .find(|k| has(k))
        .map(|k| k.to_string());
    // Sortable = every column except the `kind` label.
    let sort_metrics: Vec<String> = columns.iter().filter(|k| *k != "kind").cloned().collect();
    // Summary rows = the numeric metric columns (exclude the `kind` label and the
    // categorical `cycle`).
    let summary_metrics: Vec<String> = columns
        .iter()
        .filter(|k| *k != "kind" && *k != "cycle")
        .cloned()
        .collect();

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
        size_metrics,
        filter_metrics,
        card_metrics,
        columns,
        summary_metrics,
        grouping,
    }
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
    fn project_presets_override_then_append() {
        use code_ranker_plugin_api::plugin::Preset;
        let catalog = vec![Preset {
            id: "CPX".into(),
            label: "CPX".into(),
            title: "Complexity".into(),
            prompt: "old".into(),
            doc_url: None,
            sort_metric: "cognitive".into(),
            connections: vec![],
        }];
        let mut project = BTreeMap::new();
        // Same id → replaces the catalog entry in place.
        project.insert(
            "CPX".to_string(),
            config::model::PresetDef {
                prompt: "new".into(),
                sort_metric: "cyclomatic".into(),
                ..Default::default()
            },
        );
        // New id → appended.
        project.insert(
            "TSR".to_string(),
            config::model::PresetDef {
                sort_metric: "tsr".into(),
                ..Default::default()
            },
        );
        let merged = merge_project_presets(catalog, &project);
        assert_eq!(merged.len(), 2);
        let cpx = merged.iter().find(|p| p.id == "CPX").unwrap();
        assert_eq!(cpx.sort_metric, "cyclomatic", "same id replaced in place");
        assert_eq!(cpx.prompt, "new");
        let tsr = merged.iter().find(|p| p.id == "TSR").unwrap();
        assert_eq!(tsr.sort_metric, "tsr");
        assert_eq!(tsr.title, "TSR", "title defaults to id");
    }

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
}
