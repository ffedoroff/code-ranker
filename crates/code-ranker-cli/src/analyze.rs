//! Analysis entry point: dispatch `[input]` to the directory pipeline or read
//! a `.json`/`.html` snapshot, plus snapshot loading and the project label.
//! `check` and `report` consume the [`Analyzed`](crate::pipeline::Analyzed) result.

use crate::cli::AnalyzeArgs;
use crate::config;
use crate::pipeline::{Analyzed, analyze_directory};
use anyhow::{Context, Result};
use code_ranker_graph::snapshot::{SCHEMA_VERSION, Snapshot};
use std::path::Path;

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
pub(crate) fn analyze_input(
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
/// (`--plugins` / `--ignore`) are rejected because there is nothing to analyze.
fn analyze_from_snapshot(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<Analyzed> {
    if !args.plugins.is_empty() {
        anyhow::bail!(
            "--plugins does not apply to a snapshot input ({}): there is nothing to analyze",
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

    // Resolve each snapshot language's effective rules (`[plugins.base]` ⊕
    // `[plugins.<lang>]`), then apply cycle rules and gate per language.
    let mut rules_by_lang: std::collections::BTreeMap<String, config::RulesConfig> =
        std::collections::BTreeMap::new();
    for lang in snapshot.languages.keys() {
        rules_by_lang.insert(lang.clone(), cfg.language_config(lang)?.rules);
    }
    let mut languages = snapshot.languages.clone();
    for (lang, ls) in languages.iter_mut() {
        if let Some(level) = ls.graphs.get_mut("files") {
            let cycles = rules_by_lang
                .get(lang)
                .map(|r| r.cycles)
                .unwrap_or_default();
            config::apply_cycle_rules(&mut level.cycles, &mut level.nodes, &cycles);
        }
    }
    let violations = config::check_violations_all(&languages, &rules_by_lang);

    Ok(Analyzed {
        snapshot,
        violations,
        rules_by_lang,
        output: cfg.output,
    })
}

/// Project label for diagnostics — the basename of the analyzed target.
pub(crate) fn project_name(target: &str) -> String {
    Path::new(target)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// Load a snapshot from a `.json` file, or extract the one embedded in a `.html` report.
/// For an HTML report the `cs-current` snapshot is preferred (the state it represents),
/// falling back to `cs-baseline` (single-snapshot review reports).
pub(crate) fn load_snapshot_any(path: &Path) -> Result<Snapshot> {
    let is_html = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    if !is_html {
        return load_snapshot(path);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let snap = code_ranker_viewer::extract_embedded_snapshot(&text, "cs-current")
        .or_else(|| code_ranker_viewer::extract_embedded_snapshot(&text, "cs-baseline"))
        .with_context(|| format!("no embedded snapshot found in {}", path.display()))??;
    ensure_schema(&snap.schema_version, path)?;
    Ok(snap)
}

fn load_snapshot(path: &Path) -> Result<Snapshot> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading snapshot {}", path.display()))?;
    // Check the schema version on the raw value first, so an incompatible
    // snapshot fails with a clear version error rather than an opaque
    // deserialization error about a moved/renamed field.
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing snapshot {}", path.display()))?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    ensure_schema(version, path)?;
    serde_json::from_value(value).with_context(|| format!("parsing snapshot {}", path.display()))
}

/// Reject a snapshot whose `schema_version` this build cannot read (e.g. a
/// `--baseline` produced by an older/newer code-ranker). A structured error, so
/// `check`'s exit code distinguishes it from a passing gate.
fn ensure_schema(version: &str, path: &Path) -> Result<()> {
    if version != SCHEMA_VERSION {
        anyhow::bail!(
            "snapshot {} has schema_version {version:?}, but this build reads version {SCHEMA_VERSION:?}; \
             regenerate it with `code-ranker report`",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "analyze_test.rs"]
mod tests;
