//! CLI surface: the clap argument model (`Cli` / `Command` / `AnalyzeArgs`
//! / `OutputFormat`). Parsing only — no behaviour.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "code-ranker",
    version,
    about = "Pluggable multi-language structural analysis platform"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Diagnostics format for `check`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Default)]
pub(crate) enum OutputFormat {
    #[default]
    Human,
    Json,
    Github,
    Sarif,
    Codequality,
    /// Markdown AI fix-prompt built from the gate's own violations.
    Prompt,
}

/// Common input + analysis options shared by `check` and `report`.
#[derive(clap::Args, Debug)]
pub(crate) struct AnalyzeArgs {
    /// Input: a directory (source tree → analyze) or a `.json`/`.html` snapshot
    /// (read, no analysis). Default: current directory.
    #[arg(default_value = ".")]
    pub(crate) input: PathBuf,

    /// Plugin: rust | python | javascript | auto. Default: auto (detect by markers).
    /// Only applies when the input is a directory.
    #[arg(long)]
    pub(crate) plugin: Option<String>,

    /// Config file path, or inline `KEY=VALUE` override. Repeatable: files layer
    /// in command-line order (later wins) over the built-in defaults; passing any
    /// file skips auto-discovery of `code-ranker.toml`. Inline `KEY=VALUE`
    /// overrides apply last, after all files.
    #[arg(long, value_name = "PATH | KEY=VALUE")]
    pub(crate) config: Vec<String>,

    /// Ignore paths matching these globs (repeatable). Merged with config file.
    /// Only applies when the input is a directory.
    #[arg(long = "ignore", value_name = "GLOB")]
    pub(crate) ignore_paths: Vec<String>,

    /// Override the snapshot's git branch instead of reading it from `git`.
    /// Useful in CI, where a detached checkout reports the branch as `HEAD`
    /// (map a clean value, e.g. `--git.branch="$CI_COMMIT_REF_NAME"`).
    #[arg(long = "git.branch", value_name = "NAME")]
    pub(crate) git_branch: Option<String>,

    /// Override the snapshot's git commit hash (e.g. `--git.commit="$CI_COMMIT_SHA"`).
    #[arg(long = "git.commit", value_name = "HASH")]
    pub(crate) git_commit: Option<String>,

    /// Override the dirty-file count (e.g. `--git.dirty-files=0` to ignore the
    /// untracked files a CI job creates before the analysis runs).
    #[arg(long = "git.dirty-files", value_name = "N")]
    pub(crate) git_dirty_files: Option<u32>,

    /// Override the remote origin URL used for source links
    /// (e.g. `--git.origin="$CI_PROJECT_URL"`, avoiding a token-bearing clone URL).
    #[arg(long = "git.origin", value_name = "URL")]
    pub(crate) git_origin: Option<String>,
}

// A clap subcommand enum is parsed once at startup and never stored in a hot
// collection, so the size gap between `Check` and `Report` (both large flag
// bundles) is irrelevant — boxing the fields would only obscure the arg model.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
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

        /// Restrict the gate to these files/folders (repeatable). The whole project
        /// is still analyzed (the dependency graph needs it), but only violations
        /// located under one of these paths are reported and counted toward the exit
        /// code. Paths are repo-relative (matching the reported `where`); a folder
        /// matches everything beneath it.
        #[arg(long = "focus-path", value_name = "PATH")]
        focus_path: Vec<String>,

        /// Restrict the gate to these rules / concern groups (repeatable). Matches a
        /// full rule id (`threshold.file.hk`, `check.inline_tests_too_large`), the
        /// bare id (`inline_tests_too_large`), or a group (`TST`, `CPL`). Combine with
        /// `--focus-path` to intersect (a violation must match both).
        #[arg(long = "focus-rule", value_name = "RULE|GROUP")]
        focus_rule: Vec<String>,

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
        /// code-ranker.toml baseline (cycle counts + per-file thresholds).
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

        /// Emit a SARIF 2.1.0 report of rule violations (path from
        /// --output.sarif.path / config / default).
        #[arg(long = "output.sarif")]
        output_sarif: bool,

        /// SARIF destination: a path or name template, or `stdout`/`-`.
        /// Placeholders: {project-dir}, {ts}, {git-hash}, {git-hash-N}. Selects SARIF.
        #[arg(long = "output.sarif.path", value_name = "PATH")]
        output_sarif_path: Option<String>,

        /// Emit a GitLab Code Quality (CodeClimate) report of rule violations
        /// (path from --output.codequality.path / config / default).
        #[arg(long = "output.codequality")]
        output_codequality: bool,

        /// Code Quality destination: a path or name template, or `stdout`/`-`.
        /// Placeholders: {project-dir}, {ts}, {git-hash}, {git-hash-N}. Selects it.
        #[arg(long = "output.codequality.path", value_name = "PATH")]
        output_codequality_path: Option<String>,

        /// Emit the AI fix-prompt, auto-targeted at the single worst module of the
        /// worst-violating principle (requires `--top 1`; default to a `…-{preset}.md`
        /// file, where {preset} is that principle).
        #[arg(long = "output.prompt")]
        output_prompt: bool,

        /// Emit the console triage scorecard (default to stdout).
        #[arg(long = "output.scorecard")]
        output_scorecard: bool,

        /// AI-prompt destination: a path or name template (extra placeholder
        /// {preset}), or `stdout`/`-`. Selects the prompt format.
        #[arg(long = "output.prompt.path", value_name = "PATH")]
        output_prompt_path: Option<String>,

        /// Scorecard destination: a path or name template, or `stdout`/`-`
        /// (the default). Selects the scorecard format.
        #[arg(long = "output.scorecard.path", value_name = "PATH")]
        output_scorecard_path: Option<String>,

        /// Focus the scorecard / prompt on one axis. Accepts a **metric**
        /// (`hk`, `sloc`, … — case-insensitive, matched by value so it works with
        /// or without a configured threshold), the full threshold rule id
        /// (`threshold.file.hk`), or a **principle** id (`LSP`, `ADP`, …). A metric
        /// frames the output by the metric itself (no SOLID wrapper); a principle by
        /// that design principle. Without it, the scorecard spans every principle and
        /// the prompt auto-targets the worst. Mirrors `check`'s `--focus-rule`.
        #[arg(long = "focus-rule", value_name = "METRIC | RULE | PRINCIPLE")]
        focus_rule: Option<String>,

        /// Restrict the scorecard / prompt to modules under these paths (repeatable).
        /// The whole project is still analyzed (the graph needs it), but only modules
        /// located under one of these paths are ranked and listed. Paths are
        /// repo-relative (matching the reported location); a folder matches everything
        /// beneath it. Mirrors `check`'s `--focus-path`.
        #[arg(long = "focus-path", value_name = "PATH")]
        focus_path: Vec<String>,

        /// Threshold tier driving the scorecard: info | warning | auto.
        /// Repeatable to show several tiers. `scorecard` only.
        #[arg(long = "severity", value_name = "TIER")]
        severity: Vec<String>,

        /// Rows the scorecard shows (`--top 1` = the single worst module).
        /// `--output.prompt` requires exactly `--top 1`. Prompt/scorecard only.
        #[arg(long)]
        top: Option<usize>,

        /// Rejected: use `--top N` instead (`--top 1` = the single worst module).
        #[arg(long, value_name = "K")]
        index: Option<usize>,

        /// Instead of analyzing, write the full effective configuration to this
        /// path and exit: `[project]` (built-in defaults ⊕ `--config`) + `[plugin]`
        /// (the merged language config for `--plugin`). A diagnostic view of every
        /// parameter you can override.
        #[arg(long = "export-full-config", value_name = "PATH")]
        export_full_config: Option<PathBuf>,

        /// Print the AI fix-prompt for one principle/metric to stdout and exit
        /// (e.g. `--prompt HK`) — the named counterpart of `--output.prompt`
        /// (which auto-targets the worst). Combine with `--top N` / `--focus-path`
        /// to shape the ranked module list.
        #[arg(long = "prompt", value_name = "PRINCIPLE | METRIC")]
        prompt_id: Option<String>,

        /// Print the principle/metric doc Markdown for one id to stdout and exit
        /// (e.g. `--doc HK`) — the resolved `languages/<lang>/<ID>.md`, with any
        /// `[templates.languages.…]` override applied. No artifacts are written.
        #[arg(long = "doc", value_name = "PRINCIPLE | METRIC")]
        doc_id: Option<String>,
    },

    /// Assemble the embedded doc corpus into a directory for publishing (e.g.
    /// GitHub Pages): `base/<ID>.md` copied as-is, each language manifest emitted as
    /// its assembled Markdown, full language docs copied verbatim. No analysis.
    Docs {
        /// Output directory for the composed corpus.
        #[arg(long = "out", value_name = "DIR", default_value = "site")]
        out: PathBuf,
    },
}
