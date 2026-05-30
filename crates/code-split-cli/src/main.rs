mod config;
mod git;
mod logger;
mod plugin;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use code_split_core::Snapshot;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum GraphKind {
    Modules,
    Files,
    Functions,
}

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

    /// Which graphs to build. Repeat or comma-separate: modules,files,functions.
    #[arg(long = "graph", value_enum, num_args = 1.., value_delimiter = ',',
          default_values_t = [GraphKind::Modules, GraphKind::Files, GraphKind::Functions])]
    graphs: Vec<GraphKind>,

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

        /// Enable/disable a cycle check: KIND=on|off (e.g. test-embed=on).
        #[arg(long = "cycle-rule", value_name = "KIND=on|off")]
        cycle_rules: Vec<String>,

        /// Metric threshold: SCOPE.METRIC=N (e.g. node.cognitive=25).
        #[arg(long = "threshold", value_name = "SCOPE.METRIC=N")]
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

        /// Snapshot filename template. Placeholders: {project-dir}, {ts}.
        #[arg(long = "json-name", default_value = "{project-dir}-{ts}.json")]
        json_name: String,

        /// HTML filename template (data embedded inline). With --before, `-diff` is
        /// inserted before `.html`. Placeholders: {project-dir}, {ts}.
        #[arg(long = "html-name", default_value = "{project-dir}-{ts}.html")]
        html_name: String,
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
        } => run_check(
            &analyze,
            &cycle_rules,
            &thresholds,
            output_format,
            top,
            exit_zero,
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
            &json_name,
            &html_name,
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
}

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

    let loaded = config::load(
        &target,
        &args.config,
        &args.ignore_paths,
        cycle_rules,
        thresholds,
    )
    .unwrap_or_else(|e| {
        logger::info(&format!("config warning: {e}"));
        config::LoadedConfig {
            config: Default::default(),
            source_file: None,
        }
    });
    let cfg = loaded.config;
    if let Some(f) = &loaded.source_file {
        logger::info(&format!("config: {f}"));
    }

    let plugin_name = resolve_plugin(args.plugin.as_deref(), cfg.plugin.as_deref(), &target)?;

    let want_modules = args.graphs.contains(&GraphKind::Modules);
    let want_files = args.graphs.contains(&GraphKind::Files);
    let want_functions = args.graphs.contains(&GraphKind::Functions);

    logger::info(&format!("target:    {}", target.display()));
    logger::info(&format!("workspace: {}", cwd.display()));
    logger::info(&format!(
        "plugin: {plugin_name}{}",
        if args.local_only { " (local-only)" } else { "" }
    ));
    logger::info(&format!(
        "graphs: {}",
        args.graphs
            .iter()
            .map(|g| format!("{g:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let command = format!(
        "code-split {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );

    let (mut plugin_graphs, timings) =
        plugin::run(&plugin_name, &target, args.local_only, want_functions)
            .with_context(|| format!("plugin '{plugin_name}' failed"))?;

    if !want_modules {
        plugin_graphs.modules = Default::default();
    }
    if !want_files {
        plugin_graphs.files = Default::default();
    }
    if !want_functions {
        plugin_graphs.functions = Default::default();
    }

    let mut roots = detect_roots();
    roots.insert("target".to_string(), target.display().to_string());
    code_split_core::relativize_graphs(&mut plugin_graphs, &target, &roots);
    code_split_core::rewrite_ids(&mut plugin_graphs, &target, &roots);

    let ignored = config::apply_ignore(&mut plugin_graphs, &cfg.ignore, &target)?;
    if ignored > 0 {
        logger::info(&format!("config: {ignored} nodes filtered by ignore.paths"));
    }

    code_split_core::annotate_all_cycles(&mut plugin_graphs);
    config::apply_cycle_rules(&mut plugin_graphs, &cfg.rules.cycles);
    code_split_core::annotate_hk(&mut plugin_graphs);
    code_split_core::annotate_stats(&mut plugin_graphs.modules);
    code_split_core::annotate_stats(&mut plugin_graphs.files);
    code_split_core::annotate_stats(&mut plugin_graphs.functions);

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

    emit_diagnostics(shown, output_format);

    if total > 0 && !exit_zero {
        anyhow::bail!("{total} violation(s) found");
    }
    Ok(())
}

/// Render check diagnostics to stdout in the requested format.
fn emit_diagnostics(violations: &[config::Violation], format: OutputFormat) {
    match format {
        OutputFormat::Human => {
            for v in violations {
                println!("{}: {}", v.graph, v.message);
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(violations).unwrap_or_else(|_| "[]".into());
            println!("{json}");
        }
        OutputFormat::Github => {
            for v in violations {
                // GitHub Actions workflow-command annotation.
                println!("::error title=code-split ({})::{}", v.graph, v.message);
            }
        }
        OutputFormat::Sarif => println!("{}", sarif_document(violations)),
    }
}

/// Minimal SARIF 2.1.0 document for the violations.
fn sarif_document(violations: &[config::Violation]) -> String {
    let results: Vec<serde_json::Value> = violations
        .iter()
        .map(|v| {
            serde_json::json!({
                "ruleId": v.rule,
                "level": "error",
                "message": { "text": v.message },
                "properties": { "graph": v.graph, "weight": v.weight },
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
    json_name: &str,
    html_name: &str,
) -> Result<()> {
    let a = analyze_workspace(args, &[], &[])?;
    let target = a.target.clone();

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
        let path = report_path.join(render_name(json_name, &target));
        let mut json = serde_json::to_string_pretty(&snap)?;
        json.push('\n');
        std::fs::write(&path, json)
            .with_context(|| format!("writing snapshot to {}", path.display()))?;
        logger::info(&format!("wrote {}", path.display()));
    }

    if formats.contains(&Format::Html) {
        // `<project>-<ts>.html` for a single-snapshot review; `<project>-<ts>-diff.html`
        // when comparing against a baseline. Data is embedded inline (self-contained).
        let mut name = render_name(html_name, &target);
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

/// Expand `{project-dir}` and `{ts}` placeholders in a filename template.
fn render_name(template: &str, target: &Path) -> String {
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
    template
        .replace("{project-dir}", &slug)
        .replace("{ts}", &ts)
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
        let mut json = serde_json::to_string_pretty(&summary)?;
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
            Some(s) => serde_json::to_string(s).expect("serialize snapshot"),
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
        let out = render_name("{project-dir}-{ts}.json", Path::new("/x/My_Project"));
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
