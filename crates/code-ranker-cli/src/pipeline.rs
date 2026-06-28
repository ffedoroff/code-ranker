//! Directory-analysis pipeline: run each active plugin, the central complexity /
//! coupling / cycle passes, assemble the per-language `LevelGraph`s, and build
//! the multi-language `Snapshot`. Owns [`Analyzed`] (the shared result). Called
//! only from `analyze::analyze_input` (fan-in 1), so its necessarily-high
//! fan-out stays cheap under Henry-Kafura.

mod assemble;
mod helpers;

use crate::cli::AnalyzeArgs;
use crate::{config, git, logger, plugin};
use anyhow::{Context, Result};
use assemble::assemble_level;
use code_ranker_graph::snapshot::{LanguageSnapshot, Snapshot, SnapshotInit};
use code_ranker_plugin_api::plugin::PluginInput;
use helpers::{
    flow_kinds, numeric_attrs, prune_unused_roots, prune_unused_roots_multi, registry_omit_at,
};
use std::collections::{BTreeMap, HashSet};

/// Result of the shared analysis core, consumed by `check` and `report`. The
/// snapshot is either freshly analyzed (directory input) or loaded (snapshot input).
pub(crate) struct Analyzed {
    pub(crate) snapshot: Snapshot,
    pub(crate) violations: Vec<config::Violation>,
    /// Effective per-language rules (to recompute baseline violations for the
    /// regression gate and to dump current values), keyed by language.
    pub(crate) rules_by_lang: BTreeMap<String, config::RulesConfig>,
    /// `[output.<fmt>]` config: per-format `path` template and `enabled` flag
    /// (CLI flags still win — resolved in `run_report`).
    pub(crate) output: config::OutputConfig,
}

/// The per-language analysis result before it is merged into the snapshot.
struct AnalyzedLanguage {
    graphs: BTreeMap<String, code_ranker_graph::level_graph::LevelGraph>,
    principles: Vec<code_ranker_plugin_api::Principle>,
    prompt: code_ranker_plugin_api::PromptTemplate,
    roots: BTreeMap<String, String>,
    versions: Vec<(String, String)>,
    timings: Vec<code_ranker_graph::snapshot::StageTime>,
    /// `true` when the graph produced at least one non-external node; languages
    /// with `false` here are dropped from the active set.
    had_nodes: bool,
}

/// Parse, enrich, and assemble one language's analysis output.
///
/// This is the per-language unit of work extracted from the old single-plugin
/// `analyze_directory`. `eff_cfg` is the fully-built effective plugin config
/// (static base ⊕ `[languages.base]` ⊕ `[languages.<name>]` ⊕ CLI overrides).
#[allow(clippy::too_many_arguments)]
fn analyze_one(
    plugin_name: &str,
    target: &std::path::Path,
    input: &PluginInput,
    eff_cfg: &toml::Table,
    lang_cfg: &config::model::LangConfig,
    prompt_override: Option<&str>,
) -> Result<AnalyzedLanguage> {
    let mut timings = Vec::new();

    // 1. Parse structure (absolute file-path ids).
    let t = logger::Timer::start(&format!("{plugin_name}: parse"));
    let (mut graph, levels) = plugin::analyze(plugin_name, eff_cfg, target, input)
        .with_context(|| format!("plugin '{plugin_name}' failed"))?;
    let file_count = graph.nodes.iter().filter(|n| n.kind == "file").count();
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: format!("{plugin_name}: parse"),
        ms: t.finish_quiet(),
        detail: format!("{} nodes from {} files", graph.nodes.len(), file_count),
    });

    let had_nodes = graph.nodes.iter().any(|n| n.kind != "external");

    // 2. Complexity: plugin annotates its own file nodes with per-language metrics.
    let t = logger::Timer::start(&format!("{plugin_name}: complexity"));
    let annotated = plugin::annotate_metrics(plugin_name, eff_cfg, &mut graph);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: format!("{plugin_name}: complexity"),
        ms: t.finish_quiet(),
        detail: format!("{annotated} nodes annotated"),
    });

    // 3. Canonicalize structure, then relativize ids against detected roots.
    let t = logger::Timer::start(&format!("{plugin_name}: projection"));
    code_ranker_graph::finalize::finalize_graph(&mut graph);
    let mut roots: BTreeMap<String, String> = plugin::roots(plugin_name, eff_cfg, target)
        .into_iter()
        .collect();
    roots.insert("target".to_string(), target.display().to_string());

    // Optional `functions` level: plugin builds sub-file metric nodes.
    let want_functions = lang_cfg.levels.functions;
    if want_functions {
        let fns = plugin::function_units(plugin_name, eff_cfg, &graph);
        graph.nodes.extend(fns);
    }
    code_ranker_graph::relativize::relativize_graph(&mut graph, target, &roots);
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

    // 4. Apply ignore filters (tokenized ids), then compute derived data.
    config::apply_ignore(&mut graph, &lang_cfg.ignore, target)?;

    // Drop function nodes whose file was ignored above.
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

    let mut cycles = code_ranker_graph::cycles::annotate_cycles(&mut graph, &flow_kinds);
    config::apply_cycle_rules(&mut cycles, &mut graph.nodes, &lang_cfg.rules.cycles);
    code_ranker_graph::annotate_coupling(&mut graph, &flow_kinds);

    // Graph-derived built-in metrics (e.g. `hk`).
    for node in &mut graph.nodes {
        if node.kind != "external" {
            code_ranker_graph::write_derived(node);
        }
    }

    // User-defined declarative metrics.
    let mut custom_specs: BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec> =
        BTreeMap::new();
    let engine = if lang_cfg.metrics.is_empty() {
        None
    } else {
        let engine = code_ranker_graph::registry::Engine::compile(&lang_cfg.metrics)
            .context("compiling [metrics] formulas")?;
        for node in &mut graph.nodes {
            if node.kind == "external" {
                continue;
            }
            code_ranker_graph::apply_to_node(node, &lang_cfg.metrics, &engine);
        }
        custom_specs = lang_cfg
            .metrics
            .iter()
            .filter(|(_, d)| d.scope == code_ranker_graph::Scope::Node)
            .map(|(k, d)| (k.clone(), d.to_attribute_spec()))
            .collect();
        Some(engine)
    };

    let report_overrides = [
        plugin::report_overrides(plugin_name, eff_cfg),
        code_ranker_plugin_api::list_override::report_override_section(&lang_cfg.report),
    ];

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

    // Graph-scope aggregates.
    if let Some(engine) = &engine
        && engine.has_graph_metrics()
    {
        let omit_at = registry_omit_at(plugin_name, eff_cfg, &lang_cfg.metrics);
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

    // Warn on declared metrics that produced no value.
    for (key, def) in &lang_cfg.metrics {
        let present = match def.scope {
            code_ranker_graph::Scope::Graph => stats.contains_key(key),
            code_ranker_graph::Scope::Node => graph
                .nodes
                .iter()
                .any(|n| n.kind != "external" && n.attrs.contains_key(key)),
        };
        if !present {
            logger::summary(&format!(
                "⚠ metric `{key}` produced no value on any node — check its formula \
                 (a misspelled input key?) or whether it is always at its no-signal value",
            ));
        }
    }

    let edge_count = graph.edges.len();
    let node_count = graph.nodes.len();
    let thresholds = gate_thresholds(lang_cfg);
    let level = assemble_level(
        level_spec,
        graph,
        cycles,
        stats,
        thresholds.clone(),
        &custom_specs,
        plugin_name,
        eff_cfg,
        &report_overrides,
    );
    prune_unused_roots(&level, &mut roots);
    timings.push(code_ranker_graph::snapshot::StageTime {
        stage: format!("{plugin_name}: projection"),
        ms: t.finish_quiet(),
        detail: format!("nodes={node_count} edges={edge_count}"),
    });

    let mut graphs = BTreeMap::new();
    graphs.insert("files".to_string(), level);

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
            plugin_name,
            eff_cfg,
            &report_overrides,
        );
        graphs.insert("functions".to_string(), fn_level);
    }

    // Plugin catalog principles, then the project's `[principles.<ID>]` overrides.
    let principles = config::merge_project_principles(
        plugin::principles(plugin_name, eff_cfg, input),
        &lang_cfg.principles,
    );

    // Prompt-Generator scaffolding.
    let prompt = match prompt_override {
        Some(path) => code_ranker_graph::prompt_template_from(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading [templates] prompt override {path}"))?,
        ),
        None => code_ranker_graph::prompt_template(),
    };

    let versions = plugin::versions(plugin_name, eff_cfg, target, input);

    Ok(AnalyzedLanguage {
        graphs,
        principles,
        prompt,
        roots,
        versions,
        timings,
        had_nodes,
    })
}

/// Directory input: load config, resolve active plugins, run `analyze_one` for
/// each, drop empty languages, assemble the multi-language snapshot.
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

    let command = format!(
        "code-ranker {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    // A PluginInput (ignore filters) from a language's effective `[ignore]`.
    let plugin_input = |lc: &config::model::LangConfig| PluginInput {
        ignore: lc.ignore.paths.clone(),
        ignore_tests: lc.ignore.tests,
        gitignore: lc.ignore.gitignore,
        ignore_files: lc.ignore.ignore_files,
        hidden: lc.ignore.hidden,
    };
    // Detection uses the base-language ignore (the active set is not yet known).
    let input = plugin_input(&cfg.language_config("base")?);

    // Build effective configs for EVERY registered plugin (needed for detect_all
    // to use the user's overrides when matching markers and extensions).
    let all_names: Vec<String> = plugin::registry()
        .iter()
        .map(|p| p.name().to_string())
        .collect();
    let all_eff_cfgs: BTreeMap<String, toml::Table> = all_names
        .iter()
        .map(|name| {
            let eff = plugin::effective_plugin_config(name, &cfg.plugins.languages);
            (name.clone(), eff)
        })
        .collect();

    // Resolve the active plugin list (console > config > auto-detect).
    let active_plugins = plugin::resolve_plugins(
        &args.plugins,
        &cfg.plugins.enabled,
        &all_eff_cfgs,
        &target,
        &input,
        loaded.source_file.as_deref(),
    )?;

    // Guard: no two active plugins may claim the same file extension.
    let active_eff_cfgs: BTreeMap<String, toml::Table> = active_plugins
        .iter()
        .filter_map(|name| all_eff_cfgs.get(name).map(|t| (name.clone(), t.clone())))
        .collect();
    plugin::validate_extension_uniqueness(&active_plugins, &active_eff_cfgs)?;

    // Run each active plugin through analyze_one.
    let mut languages: BTreeMap<String, LanguageSnapshot> = BTreeMap::new();
    let mut combined_roots: BTreeMap<String, String> = BTreeMap::new();
    combined_roots.insert("target".to_string(), target.display().to_string());
    let mut combined_versions: BTreeMap<String, String> = BTreeMap::new();
    combined_versions.insert(
        "code-ranker".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    let mut combined_timings: Vec<code_ranker_graph::snapshot::StageTime> = Vec::new();
    let mut active_final: Vec<String> = Vec::new();

    // Per-language effective orchestrator config (rules/ignore/metrics/levels/
    // report/principles), resolved once per active language and reused for the gate.
    let mut rules_by_lang: BTreeMap<String, config::RulesConfig> = BTreeMap::new();

    for name in &active_plugins {
        let eff_cfg = active_eff_cfgs.get(name).cloned().unwrap_or_default();
        let lang_cfg = cfg.language_config(name)?;
        let lang_input = plugin_input(&lang_cfg);
        let result = analyze_one(
            name,
            &target,
            &lang_input,
            &eff_cfg,
            &lang_cfg,
            cfg.templates.prompt.as_deref(),
        )?;

        if !result.had_nodes {
            logger::summary(&format!(
                "⚠ plugin '{name}' produced no nodes — skipping (no source files found?)"
            ));
            continue;
        }

        // Merge roots and versions (last-writer-wins for collisions).
        combined_roots.extend(result.roots);
        for (k, v) in result.versions {
            combined_versions.insert(k, v);
        }
        combined_timings.extend(result.timings);
        active_final.push(name.clone());
        rules_by_lang.insert(name.clone(), lang_cfg.rules);

        languages.insert(
            name.clone(),
            LanguageSnapshot {
                graphs: result.graphs,
                principles: result.principles,
                prompt: result.prompt,
            },
        );
    }

    if languages.is_empty() {
        anyhow::bail!(
            "all detected languages produced empty graphs in {} — \
             no source files were analysed",
            target.display()
        );
    }

    // Runtime guarantee of the one-file-one-language invariant (see helper).
    helpers::assert_disjoint_languages(&languages)?;

    // Prune roots that are not referenced by any node across all languages.
    prune_unused_roots_multi(&languages, &mut combined_roots);

    let violations = config::check_violations_all(&languages, &rules_by_lang);

    let git = git::collect(
        &target,
        &git::GitOverride {
            branch: args.git_branch.clone(),
            commit: args.git_commit.clone(),
            dirty_files: args.git_dirty_files,
            origin: args.git_origin.clone(),
        },
    );

    active_final.sort();
    let snapshot = Snapshot::new(SnapshotInit {
        command,
        workspace: cwd.display().to_string(),
        target: target.display().to_string(),
        plugins: active_final,
        config_file: loaded.source_file,
        versions: combined_versions,
        roots: combined_roots,
        git,
        timings: combined_timings,
        languages,
    });

    Ok(Analyzed {
        snapshot,
        violations,
        rules_by_lang,
        output: cfg.output,
    })
}

/// The advisory `info`/`warning` tiers overlaid onto the metric specs (scorecard,
/// viewer badges, prompt targeting), derived from the `check` gate so the report
/// shows exactly what fails the gate. For each `[rules.thresholds.file]` limit the
/// gate value is the authoritative `warning`; a metric's own `info` (from a
/// `[metrics.<key>]` spec) is kept only when it sits strictly below the gate,
/// otherwise it is meaningless and collapses to the gate (one effective tier).
fn gate_thresholds(
    lang_cfg: &config::model::LangConfig,
) -> BTreeMap<String, code_ranker_plugin_api::level::Thresholds> {
    lang_cfg
        .rules
        .thresholds
        .file
        .limits
        .iter()
        .map(|(key, &warning)| {
            let declared_info = lang_cfg.metrics.get(key).and_then(|d| d.info);
            let info = match declared_info {
                Some(i) if i < warning => i,
                Some(i) => {
                    logger::summary(&format!(
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

#[cfg(test)]
#[path = "pipeline_test.rs"]
mod tests;
