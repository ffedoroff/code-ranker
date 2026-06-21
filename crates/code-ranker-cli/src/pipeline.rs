//! Directory-analysis pipeline: run the plugin, the central complexity /
//! coupling / cycle passes, assemble the `LevelGraph`, and build the `Snapshot`.
//! Owns [`Analyzed`] (the shared result). Called only from `analyze::analyze_input`
//! (fan-in 1), so its necessarily-high fan-out stays cheap under Henry-Kafura.

mod assemble;
mod helpers;

use crate::cli::AnalyzeArgs;
use crate::{config, git, logger, plugin};
use anyhow::{Context, Result};
use assemble::assemble_level;
use code_ranker_graph::snapshot::Snapshot;
use code_ranker_plugin_api::plugin::PluginInput;
use helpers::{flow_kinds, numeric_attrs, prune_unused_roots};
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
    /// `[templates.languages.<lang>.<ID>]` doc-corpus overrides (for `--doc`).
    pub(crate) templates: config::TemplatesConfig,
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
    // Cycles, fan-in/fan-out and the drawn map all run on the same flow edges. A
    // `pub use` re-export is a facade, not a dependency, so the Rust plugin marks
    // `reexports` non-flow (`EdgeKindSpec.flow = false`) — it never reaches any of
    // these and re-export hubs (lib.rs / mod.rs) cannot fabricate cycles.
    let mut cycles = code_ranker_graph::cycles::annotate_cycles(&mut graph, &flow_kinds);
    config::apply_cycle_rules(&mut cycles, &mut graph.nodes, &cfg.rules.cycles);
    code_ranker_graph::annotate_coupling(&mut graph, &flow_kinds);

    // Graph-derived built-in metrics (e.g. `hk`): now that the coupling pass has
    // written `fan_in`/`fan_out` onto the nodes, evaluate the `[fields.*]` formulas
    // that read them — the TIER1 → graph → TIER2 order. Pre-graph fields
    // (volume/mi/…) were already written from the raw tier-1 counts above.
    for node in &mut graph.nodes {
        if node.kind != "external" {
            code_ranker_graph::write_derived(node);
        }
    }

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
    let thresholds = gate_thresholds(&cfg);
    let level = assemble_level(
        level_spec,
        graph,
        cycles,
        stats,
        thresholds.clone(),
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
            thresholds,
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

    // Prompt-Generator scaffolding: the built-in `metrics/prompt.md`, or a
    // `[templates] prompt = "<path>"` override read from disk (same `## <field>`
    // Markdown shape).
    let prompt = match &cfg.templates.prompt {
        Some(path) => code_ranker_graph::prompt_template_from(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading [templates] prompt override {path}"))?,
        ),
        None => code_ranker_graph::prompt_template(),
    };

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
        prompt,
    );

    Ok(Analyzed {
        snapshot,
        violations,
        cycles: cfg.rules.cycles,
        rules: cfg.rules,
        output: cfg.output,
        templates: cfg.templates,
    })
}

/// Merge the project's `[presets.<ID>]` over the plugin catalog: a same-id project
/// preset replaces the plugin's (in place, keeping catalog order), a new id is
/// appended. So a project can recommend / scorecard on its own custom metric.
fn merge_project_presets(
    mut catalog: Vec<code_ranker_plugin_api::Preset>,
    project: &BTreeMap<String, config::model::PresetDef>,
) -> Vec<code_ranker_plugin_api::Preset> {
    for (id, def) in project {
        let p = def.to_preset(id);
        match catalog.iter_mut().find(|e| e.id == p.id) {
            Some(existing) => *existing = p,
            None => catalog.push(p),
        }
    }
    catalog
}

/// The advisory `info`/`warning` tiers overlaid onto the metric specs (scorecard,
/// viewer badges, prompt targeting), derived from the `check` gate so the report
/// shows exactly what fails the gate. For each `[rules.thresholds.file]` limit the
/// gate value is the authoritative `warning`; a metric's own `info` (from a
/// `[metrics.<key>]` spec) is kept only when it sits strictly below the gate,
/// otherwise it is meaningless and collapses to the gate (one effective tier).
fn gate_thresholds(
    cfg: &config::model::Config,
) -> BTreeMap<String, code_ranker_plugin_api::level::Thresholds> {
    cfg.rules
        .thresholds
        .file
        .limits
        .iter()
        .map(|(key, &warning)| {
            let declared_info = cfg.metrics.get(key).and_then(|d| d.info);
            let info = match declared_info {
                Some(i) if i < warning => i,
                Some(i) => {
                    logger::info(&format!(
                        "⚠ `[metrics.{key}]` info ({i}) ≥ gate threshold ({warning}); \
                         dropping the info tier for `{key}` (the gate wins)",
                    ));
                    warning
                }
                None => warning,
            };
            (
                key.clone(),
                code_ranker_plugin_api::level::Thresholds { info, warning },
            )
        })
        .collect()
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

#[cfg(test)]
#[path = "pipeline_test.rs"]
mod tests;
