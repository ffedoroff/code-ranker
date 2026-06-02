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

/// Output artifact kind for `report` / `diff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Json,
    Html,
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

/// Common analysis options shared by `check` and `report`.
#[derive(clap::Args, Debug)]
struct AnalyzeArgs {
    /// Workspace to analyze. Default: current directory.
    #[arg(default_value = ".")]
    workspace: PathBuf,

    /// Plugin: rust | python | javascript | auto. Default: auto (detect by markers).
    #[arg(long)]
    plugin: Option<String>,

    /// Analyze only local code — skip network-dependent steps.
    #[arg(long)]
    local_only: bool,

    /// Config file path, or inline `KEY=VALUE` override (repeatable; inline wins).
    #[arg(long, value_name = "PATH | KEY=VALUE")]
    config: Vec<String>,

    /// Ignore paths matching these globs (repeatable). Merged with config file.
    #[arg(long = "ignore", value_name = "GLOB")]
    ignore_paths: Vec<String>,

    /// Extra arguments forwarded to the plugin after `--`.
    #[arg(last = true)]
    extra: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Lint a workspace: analyze, evaluate rules, exit non-zero on any violation.
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
        /// code-split.toml baseline (cycle counts + per-scope thresholds).
        #[arg(long)]
        suggest_config: bool,
    },

    /// Analyze a workspace and write artifacts (JSON snapshot and/or HTML viewer).
    Report {
        #[command(flatten)]
        analyze: AnalyzeArgs,

        /// Artifacts to emit. Repeat or comma-separate: json,html.
        #[arg(long = "format", value_enum, num_args = 1.., value_delimiter = ',',
              default_values_t = [Format::Json, Format::Html])]
        formats: Vec<Format>,

        /// Baseline snapshot — turns the HTML into a before/after diff in one run.
        #[arg(long)]
        before: Option<PathBuf>,

        /// Output directory for artifacts.
        #[arg(long = "report-path", default_value = ".code-split")]
        report_path: PathBuf,

        /// Snapshot filename template. Placeholders: {project-dir}, {ts},
        /// {git-hash}, {git-hash-N} (first N chars of the commit). Falls back to
        /// `[output] json-name` in code-split.toml, then the built-in default.
        #[arg(long = "json-name")]
        json_name: Option<String>,

        /// HTML filename template (data embedded inline). With --before, `-diff` is
        /// inserted before `.html`. Placeholders: {project-dir}, {ts}, {git-hash},
        /// {git-hash-N}. Falls back to `[output] html-name` in code-split.toml.
        #[arg(long = "html-name")]
        html_name: Option<String>,
    },

    /// Compare two existing snapshots and write a diff report.
    Diff {
        /// Snapshot taken before the change.
        #[arg(long)]
        before: PathBuf,

        /// Snapshot taken after the change.
        #[arg(long)]
        after: PathBuf,

        /// Artifacts to emit. Repeat or comma-separate: html,json.
        #[arg(long = "format", value_enum, num_args = 1.., value_delimiter = ',',
              default_values_t = [Format::Html])]
        formats: Vec<Format>,

        /// Output directory for artifacts.
        #[arg(long = "report-path", default_value = ".code-split")]
        report_path: PathBuf,

        /// HTML diff filename.
        #[arg(long = "html-name", default_value = "index.html")]
        html_name: String,

        /// JSON diff filename.
        #[arg(long = "json-name", default_value = "diff.json")]
        json_name: String,
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
            output_format,
            top,
            exit_zero,
            suggest_config,
        } => run_check(
            &analyze,
            &cycle_rules,
            &thresholds,
            output_format,
            top,
            exit_zero,
            suggest_config,
        ),
        Command::Report {
            analyze,
            formats,
            before,
            report_path,
            json_name,
            html_name,
        } => run_report(
            &analyze,
            &formats,
            before.as_deref(),
            &report_path,
            json_name.as_deref(),
            html_name.as_deref(),
        ),
        Command::Diff {
            before,
            after,
            formats,
            report_path,
            html_name,
            json_name,
        } => run_diff(
            &before,
            &after,
            &formats,
            &report_path,
            &html_name,
            &json_name,
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

/// Result of the shared analysis core, consumed by `check` and `report`.
struct Analyzed {
    target: PathBuf,
    cwd: String,
    plugin_name: String,
    plugin_graphs: code_split_core::PluginGraphs,
    timings: Vec<code_split_core::StageTime>,
    command: String,
    source_file: Option<String>,
    local_only: bool,
    versions: HashMap<String, String>,
    roots: HashMap<String, String>,
    git: Option<code_split_core::GitInfo>,
    violations: Vec<config::Violation>,
    /// Effective cycle-rule policy (for the current-values config dump).
    cycles: config::CycleRules,
    /// Output name templates from `[output]` in config (a CLI flag, if given,
    /// still wins over these — resolved in `run_report`).
    out_json_name: Option<String>,
    out_html_name: Option<String>,
}

/// Built-in artifact name templates, used when neither a CLI flag nor the
/// `[output]` config section sets one.
const DEFAULT_JSON_NAME: &str = "{ts}-{git-hash-3}.json";
const DEFAULT_HTML_NAME: &str = "{ts}-{git-hash-3}.html";

/// Load config, run the plugin, annotate the graphs, and collect violations.
/// Writes nothing — `check` and `report` decide what to do with the result.
fn analyze_workspace(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<Analyzed> {
    let target = args
        .workspace
        .canonicalize()
        .with_context(|| format!("workspace not found: {}", args.workspace.display()))?;
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
    if let Some(f) = &loaded.source_file {
        logger::info(&format!("config: {f}"));
    }

    let plugin_name = resolve_plugin(args.plugin.as_deref(), cfg.plugin.as_deref(), &target)?;

    logger::info(&format!("target:    {}", target.display()));
    logger::info(&format!("workspace: {}", cwd.display()));
    logger::info(&format!(
        "plugin: {plugin_name}{}",
        if args.local_only { " (local-only)" } else { "" }
    ));

    let command = format!(
        "code-split {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    let (mut plugin_graphs, timings) = plugin::run(&plugin_name, &target, args.local_only)
        .with_context(|| format!("plugin '{plugin_name}' failed"))?;

    let mut roots = detect_roots();
    roots.insert("target".to_string(), target.display().to_string());
    code_split_core::relativize_graphs(&mut plugin_graphs, &target, &roots);
    code_split_core::rewrite_ids(&mut plugin_graphs, &target, &roots);
    // Drop roots that did not shorten any path — e.g. the Rust `cargo`/`rustup`/
    // `registry` roots are irrelevant to a JS/TS/Python project and would
    // otherwise leak machine-specific paths into the snapshot header.
    prune_unused_roots(&plugin_graphs, &mut roots);

    let ignored = config::apply_ignore(&mut plugin_graphs, &cfg.ignore, &target)?;
    if ignored > 0 {
        logger::info(&format!("config: {ignored} nodes filtered by ignore.paths"));
    }

    code_split_core::annotate_all_cycles(&mut plugin_graphs);
    config::apply_cycle_rules(&mut plugin_graphs, &cfg.rules.cycles);
    code_split_core::annotate_hk(&mut plugin_graphs);
    code_split_core::annotate_stats(&mut plugin_graphs.files);

    let violations = config::check_violations(&plugin_graphs, &cfg.rules);

    let git = git::collect(&target);
    if let Some(g) = &git {
        logger::info(&format!(
            "git: {} @ {} ({} dirty)",
            g.branch, g.commit, g.dirty_files
        ));
    }

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

    Ok(Analyzed {
        target,
        cwd: cwd.display().to_string(),
        plugin_name,
        plugin_graphs,
        timings,
        command,
        source_file: loaded.source_file,
        local_only: args.local_only,
        versions,
        roots,
        git,
        violations,
        cycles: cfg.rules.cycles,
        out_json_name: cfg.output.json_name,
        out_html_name: cfg.output.html_name,
    })
}

/// `check` — the linter. Analyze, report violations, exit non-zero on any.
fn run_check(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
    output_format: OutputFormat,
    top: Option<usize>,
    exit_zero: bool,
    suggest_config: bool,
) -> Result<()> {
    let mut a = analyze_workspace(args, cycle_rules, thresholds)?;
    let total = a.violations.len();

    // Rank worst-first by breach magnitude; `--top` limits only what is
    // reported, never the exit code.
    a.violations.sort_by(|x, y| y.weight.total_cmp(&x.weight));
    let shown = match top {
        Some(n) => &a.violations[..n.min(a.violations.len())],
        None => &a.violations[..],
    };

    let project = a
        .target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    emit_diagnostics(shown, total, &a.plugin_name, project, output_format);

    // Surface the current measured values as ready-to-paste config blocks only on
    // request (`--suggest-config`), human output only — machine formats stay pure.
    if suggest_config && matches!(output_format, OutputFormat::Human) {
        print_current_values(&a.plugin_graphs, &a.cycles);
    }

    if total > 0 && !exit_zero {
        anyhow::bail!("{total} violation(s) found");
    }
    Ok(())
}

/// Render check diagnostics to stdout in the requested format.
fn emit_diagnostics(
    violations: &[config::Violation],
    total: usize,
    plugin: &str,
    project: &str,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Human => print_human_diagnostics(violations, total, plugin, project),
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(violations).unwrap_or_else(|_| "[]".into());
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
        println!("  ref    {DOCS_URL}/ERRORS.md#group-{}", v.group.to_lowercase());
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

/// `report` — analyze and write artifacts (JSON snapshot and/or HTML viewer).
fn run_report(
    args: &AnalyzeArgs,
    formats: &[Format],
    before: Option<&Path>,
    report_path: &Path,
    json_name: Option<&str>,
    html_name: Option<&str>,
) -> Result<()> {
    let a = analyze_workspace(args, &[], &[])?;
    let target = a.target.clone();

    // Resolve the output name templates: explicit CLI flag wins, then the
    // `[output]` section in config, then the built-in default.
    let json_name = json_name
        .or(a.out_json_name.as_deref())
        .unwrap_or(DEFAULT_JSON_NAME)
        .to_string();
    let html_name = html_name
        .or(a.out_html_name.as_deref())
        .unwrap_or(DEFAULT_HTML_NAME)
        .to_string();

    let snap = Snapshot::new(
        a.command,
        a.cwd,
        a.target.display().to_string(),
        a.plugin_name,
        a.source_file,
        a.local_only,
        a.versions,
        a.roots,
        a.git,
        a.timings,
        a.plugin_graphs,
    );

    std::fs::create_dir_all(report_path)
        .with_context(|| format!("creating directory {}", report_path.display()))?;

    if formats.contains(&Format::Json) {
        let commit = snap.git.as_ref().map(|g| g.commit.as_str());
        let path = report_path.join(render_name(&json_name, &target, commit));
        let mut json = code_split_core::to_canonical_string_pretty(&snap)?;
        json.push('\n');
        std::fs::write(&path, json)
            .with_context(|| format!("writing snapshot to {}", path.display()))?;
        logger::info(&format!("wrote {}", path.display()));
    }

    if formats.contains(&Format::Html) {
        // `<project>-<ts>.html` for a single-snapshot review; `<project>-<ts>-diff.html`
        // when comparing against a baseline. Data is embedded inline (self-contained).
        let commit = snap.git.as_ref().map(|g| g.commit.as_str());
        let mut name = render_name(&html_name, &target, commit);
        if before.is_some() {
            let stem = name
                .strip_suffix(".html")
                .unwrap_or(name.as_str())
                .to_owned();
            name = format!("{stem}-diff.html");
        }
        let path = report_path.join(name);
        let html = match before {
            Some(p) => render_html_viewer(Some(&load_snapshot_any(p)?), Some(&snap)),
            None => render_html_viewer(Some(&snap), None),
        };
        std::fs::write(&path, html)
            .with_context(|| format!("writing report to {}", path.display()))?;
        logger::info(&format!("wrote {}", path.display()));
    }

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
        let Ok(n) = rest[..end_rel].parse::<usize>() else { break };
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

/// `diff` — compare two existing snapshots and write a diff report.
fn run_diff(
    before: &Path,
    after: &Path,
    formats: &[Format],
    report_path: &Path,
    html_name: &str,
    json_name: &str,
) -> Result<()> {
    // Either side may be a `.json` snapshot or a `.html` report (data extracted).
    let snap_before = load_snapshot_any(before)?;
    let snap_after = load_snapshot_any(after)?;

    std::fs::create_dir_all(report_path)
        .with_context(|| format!("creating directory {}", report_path.display()))?;

    if formats.contains(&Format::Html) {
        let path = report_path.join(html_name);
        std::fs::write(
            &path,
            render_html_viewer(Some(&snap_before), Some(&snap_after)),
        )
        .with_context(|| format!("writing diff to {}", path.display()))?;
        logger::info(&format!("wrote {}", path.display()));
    }

    if formats.contains(&Format::Json) {
        let path = report_path.join(json_name);
        let summary = code_split_core::compare_snapshots(&snap_before, &snap_after);
        let mut json = code_split_core::to_canonical_string_pretty(&summary)?;
        json.push('\n');
        std::fs::write(&path, json)
            .with_context(|| format!("writing diff to {}", path.display()))?;
        logger::info(&format!("wrote {}", path.display()));
    }

    Ok(())
}

/// Load a snapshot from a `.json` file, or extract the one embedded in a `.html` report.
/// For an HTML report the `cs-after` snapshot is preferred (the state it represents),
/// falling back to `cs-before` (single-snapshot review reports).
fn load_snapshot_any(path: &Path) -> Result<Snapshot> {
    let is_html = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if !is_html {
        return load_snapshot(path);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    extract_embedded_snapshot(&text, "cs-after")
        .or_else(|| extract_embedded_snapshot(&text, "cs-before"))
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
/// are stored in `<script type="application/json">` tags (`cs-before` / `cs-after`) so
/// they can be both read by the viewer and extracted from the HTML later (see
/// [`load_snapshot_any`]). `before` only → review; both → diff.
fn render_html_viewer(before: Option<&Snapshot>, after: Option<&Snapshot>) -> String {
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
        embed("cs-before", before),
        embed("cs-after", after),
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
            false,
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
        // review: before = snapshot, after = null
        let html = render_html_viewer(Some(&snap), None);
        assert!(
            html.contains(r#"<script type="application/json" id="cs-before">"#),
            "embeds before snapshot inline"
        );
        assert!(
            html.contains(r#"id="cs-after">null</script>"#),
            "after is null in review mode"
        );
        let back = extract_embedded_snapshot(&html, "cs-before")
            .expect("cs-before present")
            .unwrap();
        assert_eq!(back.plugin, "rust", "round-trips through embed/extract");
        assert!(
            extract_embedded_snapshot(&html, "cs-after").is_none(),
            "null after extracts to None"
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
