mod config;
mod git;
mod logger;
mod plugin;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use code_split_core::Snapshot;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Base URL for the published docs. Diagnostics pointers (`ref` lines, SARIF
/// `helpUri`) use absolute URLs so they're clickable from a terminal, a CI log,
/// or a report — not just from a repo checkout.
const DOCS_URL: &str = "https://github.com/ffedoroff/code-split/blob/main/docs";

#[derive(Parser, Debug)]
#[command(
    name = "code-split",
    version,
    about = "Pluggable multi-language structural analysis platform"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Diagnostics format for `check`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Human,
    Json,
    Github,
    Sarif,
}

/// Common input + analysis options shared by `check` and `report`.
#[derive(clap::Args, Debug)]
struct AnalyzeArgs {
    /// Input: a directory (source tree → analyze) or a `.json`/`.html` snapshot
    /// (read, no analysis). Default: current directory.
    #[arg(default_value = ".")]
    input: PathBuf,

    /// Plugin: rust | python | javascript | auto. Default: auto (detect by markers).
    /// Only applies when the input is a directory.
    #[arg(long)]
    plugin: Option<String>,

    /// Config file path, or inline `KEY=VALUE` override (repeatable; inline wins).
    #[arg(long, value_name = "PATH | KEY=VALUE")]
    config: Vec<String>,

    /// Ignore paths matching these globs (repeatable). Merged with config file.
    /// Only applies when the input is a directory.
    #[arg(long = "ignore", value_name = "GLOB")]
    ignore_paths: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Lint: evaluate rules (and, with --baseline, regressions); exit non-zero on violation.
    Check {
        #[command(flatten)]
        analyze: AnalyzeArgs,

        /// Cycle check: KIND=on|off|N. on = any cycle fails; off = ignored; N =
        /// allow up to N cycles of that kind (e.g. chain=7 forbids a new one).
        #[arg(long = "cycle-rule", value_name = "KIND=on|off|N")]
        cycle_rules: Vec<String>,

        /// Metric threshold: file.METRIC=N. N accepts `_` separators and
        /// K/M/G suffixes (e.g. file.cognitive=25, file.hk=5M, file.loc=1_500).
        #[arg(long = "threshold", value_name = "file.METRIC=N")]
        thresholds: Vec<String>,

        /// Baseline snapshot (`.json`/`.html`). Switches the gate to relative mode:
        /// fail only on regressions (new violations) against the baseline, not on
        /// pre-existing ones.
        #[arg(long, value_name = "SNAPSHOT")]
        baseline: Option<PathBuf>,

        /// Diagnostics format.
        #[arg(long = "output-format", value_enum, default_value_t = OutputFormat::Human)]
        output_format: OutputFormat,

        /// Report only the N worst violations (ranked worst-first). Does not change the exit code.
        #[arg(long)]
        top: Option<usize>,

        /// Exit 0 even when violations are found (collect-only mode).
        #[arg(long)]
        exit_zero: bool,

        /// Also print the project's current values as a ready-to-paste
        /// code-split.toml baseline (cycle counts + per-file thresholds).
        #[arg(long)]
        suggest_config: bool,
    },

    /// Write artifacts (HTML viewer and/or JSON snapshot). With --baseline, the HTML is a diff.
    Report {
        #[command(flatten)]
        analyze: AnalyzeArgs,

        /// Baseline snapshot (`.json`/`.html`). Turns the HTML into a baseline↔current
        /// diff with a verdict and names it `…-diff.html`.
        #[arg(long, value_name = "SNAPSHOT")]
        baseline: Option<PathBuf>,

        /// Emit the JSON snapshot (path from --output.json.path / config / default).
        #[arg(long = "output.json")]
        output_json: bool,

        /// Emit the HTML viewer (path from --output.html.path / config / default).
        #[arg(long = "output.html")]
        output_html: bool,

        /// JSON snapshot destination: a path or name template, or `stdout`/`-`.
        /// Placeholders: {project-dir}, {ts}, {git-hash}, {git-hash-N}. Selects JSON.
        #[arg(long = "output.json.path", value_name = "PATH")]
        output_json_path: Option<String>,

        /// HTML viewer destination: a path or name template, or `stdout`/`-`.
        /// Placeholders: {project-dir}, {ts}, {git-hash}, {git-hash-N}. Selects HTML.
        #[arg(long = "output.html.path", value_name = "PATH")]
        output_html_path: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let t = logger::Timer::start(&format!(
        "code-split {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    ));
    let res = match cli.command {
        Command::Check {
            analyze,
            cycle_rules,
            thresholds,
            baseline,
            output_format,
            top,
            exit_zero,
            suggest_config,
        } => run_check(
            &analyze,
            &cycle_rules,
            &thresholds,
            baseline.as_deref(),
            output_format,
            top,
            exit_zero,
            suggest_config,
        ),
        Command::Report {
            analyze,
            baseline,
            output_json,
            output_html,
            output_json_path,
            output_html_path,
        } => run_report(
            &analyze,
            baseline.as_deref(),
            output_json,
            output_html,
            output_json_path.as_deref(),
            output_html_path.as_deref(),
        ),
    };
    match &res {
        Ok(_) => {
            t.finish();
        }
        Err(e) => logger::info(&format!("error: {e:#}")),
    }
    res
}

/// Result of the shared analysis core, consumed by `check` and `report`. The
/// snapshot is either freshly analyzed (directory input) or loaded (snapshot input).
struct Analyzed {
    snapshot: Snapshot,
    violations: Vec<config::Violation>,
    /// Effective cycle-rule policy (for the current-values config dump).
    cycles: config::CycleRules,
    /// Effective rules (to recompute baseline violations for the regression gate).
    rules: config::RulesConfig,
    /// `[output.<fmt>]` config: per-format `path` template and `enabled` flag
    /// (CLI flags still win — resolved in `run_report`).
    output: config::OutputConfig,
}

/// Built-in artifact path templates, used when neither a `--output.<fmt>` flag,
/// a `--output.<fmt>.path`, nor the `[output.<fmt>]` config section sets one.
const DEFAULT_JSON_PATH: &str = ".code-split/{ts}-{git-hash-3}.json";
const DEFAULT_HTML_PATH: &str = ".code-split/{ts}-{git-hash-3}.html";

/// Does this input path denote a snapshot artifact (read directly) rather than a
/// source directory to analyze?
fn is_snapshot_input(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("json" | "html" | "htm")
    )
}

/// Produce the analysis result for `[input]`: analyze a directory, or read a
/// `.json`/`.html` snapshot. `check` and `report` decide what to do with it.
fn analyze_input(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<Analyzed> {
    if is_snapshot_input(&args.input) {
        analyze_from_snapshot(args, cycle_rules, thresholds)
    } else {
        analyze_directory(args, cycle_rules, thresholds)
    }
}

/// Snapshot input: read the embedded snapshot and evaluate the current rules
/// against it — no source tree or toolchain required. Analysis-only flags
/// (`--plugin` / `--ignore`) are rejected because there is nothing to analyze.
fn analyze_from_snapshot(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<Analyzed> {
    if args.plugin.is_some() {
        anyhow::bail!(
            "--plugin does not apply to a snapshot input ({}): there is nothing to analyze",
            args.input.display()
        );
    }
    if !args.ignore_paths.is_empty() {
        anyhow::bail!(
            "--ignore does not apply to a snapshot input ({}): there is nothing to analyze",
            args.input.display()
        );
    }
    let snapshot = load_snapshot_any(&args.input)?;
    // Config (rules + output) is located from the cwd for a snapshot input.
    let cwd = std::env::current_dir()?;
    let loaded = config::load(&cwd, &args.config, &[], cycle_rules, thresholds)
        .context("configuration error")?;
    let cfg = loaded.config;

    let mut graphs = snapshot.graphs.clone();
    config::apply_cycle_rules(&mut graphs, &cfg.rules.cycles);
    let violations = config::check_violations(&graphs, &cfg.rules);

    Ok(Analyzed {
        snapshot,
        violations,
        cycles: cfg.rules.cycles,
        rules: cfg.rules,
        output: cfg.output,
    })
}

/// Directory input: load config, run the plugin, annotate the graphs, collect
/// violations, and assemble the snapshot. Writes nothing.
fn analyze_directory(
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
        "code-split {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    let (mut plugin_graphs, timings) = plugin::run(&plugin_name, &target)
        .with_context(|| format!("plugin '{plugin_name}' failed"))?;

    let mut roots = detect_roots();
    roots.insert("target".to_string(), target.display().to_string());
    code_split_core::relativize_graphs(&mut plugin_graphs, &target, &roots);
    code_split_core::rewrite_ids(&mut plugin_graphs, &target, &roots);
    // Drop roots that did not shorten any path — e.g. the Rust `cargo`/`rustup`/
    // `registry` roots are irrelevant to a JS/TS/Python project and would
    // otherwise leak machine-specific paths into the snapshot header.
    prune_unused_roots(&plugin_graphs, &mut roots);

    config::apply_ignore(&mut plugin_graphs, &cfg.ignore, &target)?;

    code_split_core::annotate_all_cycles(&mut plugin_graphs);
    config::apply_cycle_rules(&mut plugin_graphs, &cfg.rules.cycles);
    code_split_core::annotate_hk(&mut plugin_graphs);
    code_split_core::annotate_stats(&mut plugin_graphs.files);

    let violations = config::check_violations(&plugin_graphs, &cfg.rules);

    let git = git::collect(&target);

    let mut versions = HashMap::new();
    versions.insert(
        "code-split".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    if plugin_name == "rust" {
        versions.insert(
            "code_split_plugin_rust".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        if let Some(rv) = plugin::rust::version_string() {
            versions.insert("rustc".to_string(), rv);
        }
    }

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
        plugin_graphs,
    );

    Ok(Analyzed {
        snapshot,
        violations,
        cycles: cfg.rules.cycles,
        rules: cfg.rules,
        output: cfg.output,
    })
}

/// Project label for diagnostics — the basename of the analyzed target.
fn project_name(target: &str) -> String {
    Path::new(target)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// `check` — the linter. Evaluate rules (and, with `--baseline`, regressions);
/// exit non-zero on any violation that fails the gate.
#[allow(clippy::too_many_arguments)]
fn run_check(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
    baseline: Option<&Path>,
    output_format: OutputFormat,
    top: Option<usize>,
    exit_zero: bool,
    suggest_config: bool,
) -> Result<()> {
    let a = analyze_input(args, cycle_rules, thresholds)?;
    let project = project_name(&a.snapshot.target);
    let plugin = a.snapshot.plugin.clone();

    // Without --baseline the gate is absolute: every violation counts. With
    // --baseline it is relative: only violations not already present in the
    // baseline (under the same rules) count — pre-existing ones are tolerated.
    let (mut findings, verdict) = match baseline {
        None => (a.violations, None),
        Some(bpath) => {
            let base = load_snapshot_any(bpath)?;
            let mut bgraphs = base.graphs.clone();
            config::apply_cycle_rules(&mut bgraphs, &a.rules.cycles);
            let base_v = config::check_violations(&bgraphs, &a.rules);
            let sig = |v: &config::Violation| (v.rule.clone(), v.location.clone());
            let base_sigs: HashSet<(String, String)> = base_v.iter().map(sig).collect();
            let cur_sigs: HashSet<(String, String)> = a.violations.iter().map(sig).collect();
            let resolved = base_sigs.iter().filter(|s| !cur_sigs.contains(*s)).count();
            let new_v: Vec<config::Violation> = a
                .violations
                .into_iter()
                .filter(|v| !base_sigs.contains(&sig(v)))
                .collect();
            let verdict = if !new_v.is_empty() {
                "degraded"
            } else if resolved > 0 {
                "improved"
            } else {
                "neutral"
            };
            (new_v, Some(verdict))
        }
    };

    let total = findings.len();
    // Rank worst-first by breach magnitude; `--top` limits only what is
    // reported, never the exit code.
    findings.sort_by(|x, y| y.weight.total_cmp(&x.weight));
    let shown = match top {
        Some(n) => &findings[..n.min(findings.len())],
        None => &findings[..],
    };

    emit_diagnostics(shown, total, &plugin, &project, output_format, verdict);

    // Surface the current measured values as ready-to-paste config blocks only on
    // request (`--suggest-config`), human output only — machine formats stay pure.
    if suggest_config && matches!(output_format, OutputFormat::Human) {
        print_current_values(&a.snapshot.graphs, &a.cycles);
    }

    if total > 0 && !exit_zero {
        let what = if baseline.is_some() {
            "new violation(s) vs baseline"
        } else {
            "violation(s) found"
        };
        anyhow::bail!("{total} {what}");
    }
    Ok(())
}

/// Render check diagnostics to stdout in the requested format. With a baseline,
/// `verdict` (improved/degraded/neutral) is included: a trailing line in `human`,
/// a wrapping object in `json`.
fn emit_diagnostics(
    violations: &[config::Violation],
    total: usize,
    plugin: &str,
    project: &str,
    format: OutputFormat,
    verdict: Option<&str>,
) {
    match format {
        OutputFormat::Human => {
            print_human_diagnostics(violations, total, plugin, project);
            if let Some(v) = verdict {
                println!("\nBaseline verdict: {v}");
            }
        }
        OutputFormat::Json => {
            let json = match verdict {
                Some(v) => serde_json::to_string_pretty(&serde_json::json!({
                    "verdict": v,
                    "violations": violations,
                }))
                .unwrap_or_else(|_| "{}".into()),
                None => serde_json::to_string_pretty(violations).unwrap_or_else(|_| "[]".into()),
            };
            println!("{json}");
        }
        OutputFormat::Github => {
            for v in violations {
                // GitHub Actions workflow-command annotation (rule id in the title).
                println!(
                    "::error title=code-split {} ({})::{}",
                    v.rule,
                    v.graph,
                    v.summary()
                );
            }
        }
        OutputFormat::Sarif => println!("{}", sarif_document(violations)),
    }
}

/// Human diagnostics: a short, self-contained block per violation — rule id,
/// group, where, the measurement, why it matters, how to fix it, and how to tune
/// it — so any single block can be pasted into an AI assistant as a complete prompt.
fn print_human_diagnostics(
    violations: &[config::Violation],
    total: usize,
    plugin: &str,
    project: &str,
) {
    if total == 0 {
        println!("✓ code-split check: no violations in {project} ({plugin} plugin).");
        return;
    }

    println!("code-split check — {total} violation(s) in {project} ({plugin} plugin)");
    if violations.len() < total {
        println!(
            "  showing the {} worst by severity; run without --top to see all",
            violations.len()
        );
    }
    println!(
        "Each finding below is self-contained — copy a block into an AI assistant to act on it."
    );
    println!("Full rule reference: {DOCS_URL}/ERRORS.md\n");

    for v in violations {
        let doc = config::rule_doc(&v.rule);
        println!("{}  ·  {}  ·  {} graph", v.rule, v.group, v.graph);
        if !v.location.is_empty() {
            println!("  where  {}", v.location);
        }
        println!("  issue  {}", v.message);
        if let Some(d) = doc {
            println!("  why    {}", d.why);
            println!("  fix    {}", d.fix);
        }
        let tune = config::rule_tuning(&v.rule);
        if !tune.is_empty() {
            println!("  tune   {tune}");
        }
        println!(
            "  ref    {DOCS_URL}/ERRORS.md#group-{}",
            v.group.to_lowercase()
        );
        println!();
    }

    // Tail breakdown by concern group so the end of the output summarizes at a glance.
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for v in violations {
        match counts.iter_mut().find(|(g, _)| *g == v.group) {
            Some((_, n)) => *n += 1,
            None => counts.push((v.group, 1)),
        }
    }
    let breakdown = counts
        .iter()
        .map(|(g, n)| format!("{g}×{n}"))
        .collect::<Vec<_>>()
        .join("  ");
    let scope = if violations.len() < total {
        "shown"
    } else {
        "total"
    };
    println!("Summary ({scope}): {breakdown}");
}

/// The six threshold metrics, in display order.
const METRICS: [&str; 6] = ["cyclomatic", "cognitive", "hk", "fan_in", "fan_out", "loc"];

/// Print the current measured values per scope as ready-to-paste `code-split.toml`
/// threshold blocks: the per-unit worst value (`single`) and the graph-wide
/// average (`avg`). Lets a user pin today's numbers as a baseline that passes.
fn print_current_values(graphs: &code_split_core::PluginGraphs, cycles: &config::CycleRules) {
    println!();
    println!("Current config — copy the blocks below into code-split.toml:");

    // Cycle budgets: today's count per kind (paste to forbid adding more), or
    // `false` for a disabled kind. `0` = strict — no cycles of that kind allowed.
    println!();
    println!(
        "# cycles: max allowed count per kind (today's count — raise only to allow more; false = off)"
    );
    println!("[rules.cycles]");
    for (key, kind, rule) in [
        (
            "test-embed",
            code_split_core::CycleKind::TestEmbed,
            cycles.test_embed,
        ),
        ("mutual", code_split_core::CycleKind::Mutual, cycles.mutual),
        ("chain", code_split_core::CycleKind::Chain, cycles.chain),
    ] {
        if rule.is_off() {
            println!("{key:<12}= false");
        } else {
            let n = graphs
                .files
                .cycles
                .iter()
                .filter(|c| c.kind == kind)
                .count();
            println!("{key:<12}= {n}");
        }
    }

    // Thresholds: measured per-file maxima to pin as a baseline.
    println!();
    println!("# thresholds: the worst single file (max) per metric");
    print_scope_values("file", &graphs.files);
}

/// Emit a `[rules.thresholds.<scope>]` block with the per-file metric maxima.
fn print_scope_values(scope: &str, graph: &code_split_core::graph::Graph) {
    // Per-file maxima across the graph's nodes.
    let mut max = [0f64; 6];
    let mut any = false;
    for n in &graph.nodes {
        let Some(cx) = &n.complexity else { continue };
        any = true;
        max[0] = max[0].max(cx.cyclomatic);
        max[1] = max[1].max(cx.cognitive);
        if let Some(c) = &cx.coupling {
            max[2] = max[2].max(c.hk);
            max[3] = max[3].max(c.fan_in as f64);
            max[4] = max[4].max(c.fan_out as f64);
        }
        if let Some(l) = &cx.loc {
            max[5] = max[5].max(l.source);
        }
    }
    if !any {
        return; // no metrics
    }
    // Per-file limits use the exact max (a strict `>` check then passes today).
    print_toml_block(&format!("[rules.thresholds.{scope}]"), &max, false);
}

/// Print one TOML table, one `metric = value` line per non-zero metric. With
/// `round_up`, fractional values (averages) are ceiled so a strict `>` check
/// still passes at the printed limit.
fn print_toml_block(header: &str, vals: &[f64; 6], round_up: bool) {
    let rows: Vec<(&str, u64)> = METRICS
        .iter()
        .zip(vals)
        .filter_map(|(name, &v)| {
            let n = if round_up { v.ceil() } else { v.round() } as u64;
            (n > 0).then_some((*name, n))
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    println!();
    println!("{header}");
    for (name, v) in rows {
        println!("{name:<12}= {}", group_digits(v));
    }
}

/// Format an integer with `_` thousands separators (e.g. 512712 → "512_712"),
/// matching the human number syntax accepted by `--threshold` / the config.
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let len = s.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(ch);
    }
    out
}

/// Minimal SARIF 2.1.0 document. `ruleId` is the dotted rule id (e.g.
/// `threshold.file.loc`); the rules that actually fired are described under
/// `tool.driver.rules` (id, group, rationale, helpUri) so the report is self-documenting.
fn sarif_document(violations: &[config::Violation]) -> String {
    // Distinct fired rule ids, first-seen order, so each results.ruleId resolves.
    let mut seen: Vec<&config::Violation> = Vec::new();
    for v in violations {
        if !seen.iter().any(|s| s.rule == v.rule) {
            seen.push(v);
        }
    }
    let rules: Vec<serde_json::Value> = seen
        .iter()
        .map(|v| {
            let doc = config::rule_doc(&v.rule);
            serde_json::json!({
                "id": v.rule,
                "shortDescription": { "text": doc.map(|d| d.title).unwrap_or(v.rule.as_str()) },
                "fullDescription": { "text": doc.map(|d| d.why).unwrap_or("") },
                "helpUri": format!(
                    "{DOCS_URL}/ERRORS.md#group-{}",
                    v.group.to_lowercase()
                ),
                "properties": { "group": v.group },
            })
        })
        .collect();
    let results: Vec<serde_json::Value> = violations
        .iter()
        .map(|v| {
            serde_json::json!({
                "ruleId": v.rule,
                "level": "error",
                "message": { "text": v.summary() },
                "properties": { "group": v.group, "graph": v.graph, "weight": v.weight },
            })
        })
        .collect();
    let doc = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "code-split",
                "informationUri": "https://github.com/ffedoroff/code-split",
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules,
            }},
            "results": results,
        }],
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

/// `report` — analyze (or read) the input and write artifacts. Which formats are
/// written, and where, follows the `--output.<fmt>[.path]` flags and the
/// `[output.<fmt>]` config (see [`want_format`]).
fn run_report(
    args: &AnalyzeArgs,
    baseline: Option<&Path>,
    output_json: bool,
    output_html: bool,
    json_path: Option<&str>,
    html_path: Option<&str>,
) -> Result<()> {
    let a = analyze_input(args, &[], &[])?;
    let snap = &a.snapshot;
    let target = PathBuf::from(&snap.target);
    let commit = snap.git.as_ref().map(|g| g.commit.as_str());

    // Decide which formats to write. A format is selected by a CLI flag
    // (`--output.<fmt>` / `--output.<fmt>.path`), or by config (`enabled`, else a
    // configured `path`). If nothing selects anything, write both by default.
    let mut want_json = want_format(output_json, json_path, &a.output.json);
    let mut want_html = want_format(output_html, html_path, &a.output.html);
    if !want_json && !want_html {
        want_json = true;
        want_html = true;
    }

    let baseline_snap = match baseline {
        Some(p) => Some(load_snapshot_any(p)?),
        None => None,
    };

    if want_json {
        let tpl = json_path
            .or(a.output.json.path.as_deref())
            .unwrap_or(DEFAULT_JSON_PATH);
        let dest = render_name(tpl, &target, commit);
        let mut json = code_split_core::to_canonical_string_pretty(snap)?;
        json.push('\n');
        write_artifact(&dest, &json, "json")?;
    }

    if want_html {
        let tpl = html_path
            .or(a.output.html.path.as_deref())
            .unwrap_or(DEFAULT_HTML_PATH);
        let mut dest = render_name(tpl, &target, commit);
        // A baseline turns the HTML into a diff; mark the filename `…-diff.html`
        // (unless it goes to the stdout stream).
        if baseline_snap.is_some() && !is_stream(&dest) {
            dest = match dest.strip_suffix(".html") {
                Some(stem) => format!("{stem}-diff.html"),
                None => format!("{dest}-diff"),
            };
        }
        let html = render_html_viewer(baseline_snap.as_ref(), Some(snap));
        write_artifact(&dest, &html, "html")?;
    }

    Ok(())
}

/// Whether an artifact format is written: a CLI flag/path forces it on; otherwise
/// the config `enabled` flag decides; otherwise a configured `path` implies on.
fn want_format(cli_flag: bool, cli_path: Option<&str>, cfg: &config::OutputArtifact) -> bool {
    if cli_flag || cli_path.is_some() {
        return true;
    }
    cfg.enabled.unwrap_or_else(|| cfg.path.is_some())
}

/// Is this destination the stdout stream rather than a file?
fn is_stream(dest: &str) -> bool {
    dest == "stdout" || dest == "-"
}

/// Write one artifact to its destination: the stdout stream for `stdout`/`-`,
/// otherwise a file (creating parent directories).
fn write_artifact(dest: &str, content: &str, kind: &str) -> Result<()> {
    if is_stream(dest) {
        print!("{content}");
        return Ok(());
    }
    let path = Path::new(dest);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("writing {kind} to {}", path.display()))?;
    logger::info(&format!("{kind}-report={}", path.display()));
    Ok(())
}

/// Expand filename-template placeholders:
/// `{project-dir}` (slugified target dir name), `{ts}` (local timestamp),
/// `{git-hash}` (full short commit) and `{git-hash-N}` (first N chars of it).
/// When there is no git commit, the hash falls back to zeros.
fn render_name(template: &str, target: &Path, commit: Option<&str>) -> String {
    let project = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("snapshot");
    let slug: String = project
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let hash = commit.unwrap_or("000000000000");
    let mut out = template
        .replace("{project-dir}", &slug)
        .replace("{ts}", &ts)
        .replace("{git-hash}", hash);
    // `{git-hash-N}` → first N chars of the commit hash.
    while let Some(start) = out.find("{git-hash-") {
        let rest = &out[start + "{git-hash-".len()..];
        let Some(end_rel) = rest.find('}') else { break };
        let Ok(n) = rest[..end_rel].parse::<usize>() else {
            break;
        };
        let take: String = hash.chars().take(n).collect();
        let token_end = start + "{git-hash-".len() + end_rel + 1;
        out.replace_range(start..token_end, &take);
    }
    out
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
    detect_plugin(workspace)
}

/// Detect the plugin from project markers in the workspace root.
fn detect_plugin(workspace: &Path) -> Result<String> {
    let mut found: Vec<&str> = Vec::new();
    if workspace.join("Cargo.toml").exists() {
        found.push("rust");
    }
    if workspace.join("pyproject.toml").exists()
        || workspace.join("setup.py").exists()
        || workspace.join("setup.cfg").exists()
    {
        found.push("python");
    }
    if workspace.join("package.json").exists() || workspace.join("tsconfig.json").exists() {
        found.push("javascript");
    }
    match found.as_slice() {
        [one] => Ok((*one).to_string()),
        [] => anyhow::bail!(
            "could not auto-detect a plugin in {}: no project marker found — \
             pass --plugin rust|python|javascript",
            workspace.display()
        ),
        many => anyhow::bail!(
            "multiple project markers found ({}) — pass --plugin to choose",
            many.join(", ")
        ),
    }
}

fn detect_roots() -> HashMap<String, String> {
    let mut roots = HashMap::new();
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
        if let Ok(out) = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            && out.status.success()
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
fn prune_unused_roots(graphs: &code_split_core::PluginGraphs, roots: &mut HashMap<String, String>) {
    let mut used: HashSet<String> = HashSet::new();
    used.insert("target".to_string());
    for node in &graphs.files.nodes {
        for name in roots.keys() {
            let token = format!("{{{name}}}");
            if node.id.contains(&token) || node.path.contains(&token) {
                used.insert(name.clone());
            }
        }
    }
    roots.retain(|name, _| used.contains(name));
}

/// Load a snapshot from a `.json` file, or extract the one embedded in a `.html` report.
/// For an HTML report the `cs-current` snapshot is preferred (the state it represents),
/// falling back to `cs-baseline` (single-snapshot review reports).
fn load_snapshot_any(path: &Path) -> Result<Snapshot> {
    let is_html = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if !is_html {
        return load_snapshot(path);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    extract_embedded_snapshot(&text, "cs-current")
        .or_else(|| extract_embedded_snapshot(&text, "cs-baseline"))
        .with_context(|| format!("no embedded snapshot found in {}", path.display()))?
}

/// Pull the JSON out of `<script type="application/json" id="{id}">…</script>` and parse
/// it into a `Snapshot`. Returns `None` if the tag is absent or holds `null`.
fn extract_embedded_snapshot(html: &str, id: &str) -> Option<Result<Snapshot>> {
    let needle = format!("id=\"{id}\">");
    let start = html.find(&needle)? + needle.len();
    let end = start + html[start..].find("</script>")?;
    let body = html[start..end].trim();
    if body.is_empty() || body == "null" {
        return None;
    }
    // Undo the `</` → `<\/` escaping applied when embedding.
    let json = body.replace("<\\/", "</");
    Some(serde_json::from_str(&json).with_context(|| format!("parsing embedded snapshot `{id}`")))
}

fn load_snapshot(path: &Path) -> Result<Snapshot> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading snapshot {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing snapshot {}", path.display()))
}

// ── Assets embedded at compile time ──────────────────────────────────────────
const ASSET_CSS: &str = include_str!("assets/index.css");
const ASSET_GV: &str = include_str!("assets/graphviz.umd.js");
const ASSET_SNARKDOWN: &str = include_str!("assets/snarkdown.umd.js");
const ASSET_DIFF: &str = include_str!("assets/diff.js");
const ASSET_LAYOUT: &str = include_str!("assets/layout.js");
const ASSET_UTILS: &str = include_str!("assets/utils.js");
const ASSET_MODAL: &str = include_str!("assets/modal.js");
const ASSET_PANZOOM: &str = include_str!("assets/panzoom.js");
const ASSET_DIAGRAM: &str = include_str!("assets/diagram.js");
const ASSET_UI: &str = include_str!("assets/ui.js");
const ASSET_SUMMARY: &str = include_str!("assets/summary.js");
const ASSET_EXPORT_POPUP: &str = include_str!("assets/export-popup.js");
const ASSET_NODE_TABLE: &str = include_str!("assets/node-table.js");
const ASSET_NAV: &str = include_str!("assets/nav.js");
const ASSET_APP: &str = include_str!("assets/app.js");
const ASSET_HTML: &str = include_str!("assets/index.html");

/// Render a self-contained viewer with the snapshot data embedded inline. The snapshots
/// are stored in `<script type="application/json">` tags (`cs-baseline` / `cs-current`) so
/// they can be both read by the viewer and extracted from the HTML later (see
/// [`load_snapshot_any`]). `current` only → review; both → diff.
fn render_html_viewer(baseline: Option<&Snapshot>, current: Option<&Snapshot>) -> String {
    // Embed as JSON in a typed script tag. Escape `</` so an embedded string can never
    // close the tag early; `JSON.parse` and serde both read `<\/` back as `</`.
    let embed = |id: &str, snap: Option<&Snapshot>| {
        let json = match snap {
            Some(s) => code_split_core::to_canonical_string(s).expect("serialize snapshot"),
            None => "null".to_string(),
        };
        format!(
            "<script type=\"application/json\" id=\"{id}\">{}</script>",
            json.replace("</", "<\\/")
        )
    };
    let data_script = format!(
        "{}\n{}",
        embed("cs-baseline", baseline),
        embed("cs-current", current),
    );

    ASSET_HTML
        .replace(
            r#"<link rel="stylesheet" href="./index.css">"#,
            &format!("<style>{}</style>", ASSET_CSS),
        )
        .replace(
            r#"<script src="./graphviz.umd.js"></script>"#,
            &format!("<script>{}</script>", ASSET_GV),
        )
        .replace(
            r#"<script src="./snarkdown.umd.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SNARKDOWN),
        )
        .replace(r#"<script src="./data.js"></script>"#, &data_script)
        .replace(
            r#"<script src="./diff.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIFF),
        )
        .replace(
            r#"<script src="./layout.js"></script>"#,
            &format!("<script>{}</script>", ASSET_LAYOUT),
        )
        .replace(
            r#"<script src="./utils.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UTILS),
        )
        .replace(
            r#"<script src="./modal.js"></script>"#,
            &format!("<script>{}</script>", ASSET_MODAL),
        )
        .replace(
            r#"<script src="./panzoom.js"></script>"#,
            &format!("<script>{}</script>", ASSET_PANZOOM),
        )
        .replace(
            r#"<script src="./diagram.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIAGRAM),
        )
        .replace(
            r#"<script src="./ui.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UI),
        )
        .replace(
            r#"<script src="./summary.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SUMMARY),
        )
        .replace(
            r#"<script src="./export-popup.js"></script>"#,
            &format!("<script>{}</script>", ASSET_EXPORT_POPUP),
        )
        .replace(
            r#"<script src="./node-table.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NODE_TABLE),
        )
        .replace(
            r#"<script src="./nav.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NAV),
        )
        .replace(
            r#"<script src="./app.js"></script>"#,
            &format!("<script>{}</script>", ASSET_APP),
        )
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn render_name_expands_placeholders_and_slugifies() {
        let out = render_name("{project-dir}-{ts}.json", Path::new("/x/My_Project"), None);
        assert!(out.starts_with("my-project-"), "slugified prefix: {out}");
        assert!(out.ends_with(".json"), "extension preserved: {out}");
        assert!(
            !out.contains('{') && !out.contains('}'),
            "no unexpanded placeholders: {out}"
        );
        let stamp = out
            .trim_start_matches("my-project-")
            .trim_end_matches(".json");
        assert_eq!(stamp.len(), 15, "ts is YYYYMMDD-HHMMSS: {stamp:?}");
        assert!(
            stamp.chars().all(|c| c.is_ascii_digit() || c == '-'),
            "ts is digits and a dash: {stamp:?}"
        );
    }

    #[test]
    fn render_name_expands_git_hash() {
        let t = Path::new("/x/proj");
        // Default-style template: `{ts}-{git-hash-3}.json`.
        let out = render_name("{ts}-{git-hash-3}.json", t, Some("69aa698abcde"));
        assert!(out.ends_with("-69a.json"), "first 3 hash chars: {out}");
        // Full short hash.
        let full = render_name("{git-hash}.json", t, Some("69aa698abcde"));
        assert_eq!(full, "69aa698abcde.json");
        // No git → zero fallback, still no leftover placeholder.
        let none = render_name("{git-hash-3}.json", t, None);
        assert_eq!(none, "000.json");
    }

    #[test]
    fn detect_plugin_by_single_marker() {
        let cases = vec![
            ("Cargo.toml", "rust"),
            ("pyproject.toml", "python"),
            ("setup.py", "python"),
            ("package.json", "javascript"),
            ("tsconfig.json", "javascript"),
        ];
        for (marker, expected) in cases {
            let d = tempfile::tempdir().unwrap();
            fs::write(d.path().join(marker), "").unwrap();
            assert_eq!(
                detect_plugin(d.path()).unwrap(),
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
        let err = format!("{:#}", detect_plugin(amb.path()).unwrap_err());
        assert!(err.contains("multiple"), "ambiguous error: {err}");

        let empty = tempfile::tempdir().unwrap();
        let err = format!("{:#}", detect_plugin(empty.path()).unwrap_err());
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

    fn mk_snap() -> Snapshot {
        Snapshot::new(
            "cmd".into(),
            "ws".into(),
            "tgt".into(),
            "rust".into(),
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
            Vec::new(),
            code_split_core::PluginGraphs::default(),
        )
    }

    #[test]
    fn viewer_embeds_snapshot_inline_and_round_trips() {
        let snap = mk_snap();
        // review: current = snapshot, baseline = null
        let html = render_html_viewer(None, Some(&snap));
        assert!(
            html.contains(r#"<script type="application/json" id="cs-current">"#),
            "embeds current snapshot inline"
        );
        assert!(
            html.contains(r#"id="cs-baseline">null</script>"#),
            "baseline is null in review mode"
        );
        let back = extract_embedded_snapshot(&html, "cs-current")
            .expect("cs-current present")
            .unwrap();
        assert_eq!(back.plugin, "rust", "round-trips through embed/extract");
        assert!(
            extract_embedded_snapshot(&html, "cs-baseline").is_none(),
            "null baseline extracts to None"
        );
    }

    #[test]
    fn load_snapshot_any_reads_json_and_html() {
        let snap = mk_snap();
        let d = tempfile::tempdir().unwrap();

        let jp = d.path().join("s.json");
        fs::write(&jp, serde_json::to_string(&snap).unwrap()).unwrap();
        assert_eq!(load_snapshot_any(&jp).unwrap().plugin, "rust", "from .json");

        let hp = d.path().join("r.html");
        fs::write(&hp, render_html_viewer(None, Some(&snap))).unwrap();
        assert_eq!(
            load_snapshot_any(&hp).unwrap().plugin,
            "rust",
            "from embedded .html"
        );
    }
}
