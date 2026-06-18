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
    if let Some(level) = graphs.get_mut("files") {
        config::apply_cycle_rules(&mut level.cycles, &mut level.nodes, &cfg.rules.cycles);
    }
    let violations = config::check_violations(&graphs, &cfg.rules);

    Ok(Analyzed {
        snapshot,
        violations,
        cycles: cfg.rules.cycles,
        rules: cfg.rules,
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
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    fn mk_snap() -> Snapshot {
        Snapshot::new(
            "cmd".into(),
            "ws".into(),
            "tgt".into(),
            "rust".into(),
            None,
            BTreeMap::new(),
            BTreeMap::new(),
            None,
            Vec::new(),
            BTreeMap::new(),
            Vec::new(),
            Default::default(),
        )
    }

    #[test]
    fn viewer_embeds_snapshot_inline_and_round_trips() {
        let snap = mk_snap();
        // review: current = snapshot, baseline = null
        let html = code_ranker_viewer::render_html_viewer(None, Some(&snap));
        assert!(
            html.contains(r#"<script type="application/json" id="cs-current">"#),
            "embeds current snapshot inline"
        );
        assert!(
            html.contains(r#"id="cs-baseline">null</script>"#),
            "baseline is null in review mode"
        );
        let back = code_ranker_viewer::extract_embedded_snapshot(&html, "cs-current")
            .expect("cs-current present")
            .unwrap();
        assert_eq!(back.plugin, "rust", "round-trips through embed/extract");
        assert!(
            code_ranker_viewer::extract_embedded_snapshot(&html, "cs-baseline").is_none(),
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
        fs::write(
            &hp,
            code_ranker_viewer::render_html_viewer(None, Some(&snap)),
        )
        .unwrap();
        assert_eq!(
            load_snapshot_any(&hp).unwrap().plugin,
            "rust",
            "from embedded .html"
        );
    }

    #[test]
    fn load_snapshot_rejects_schema_version_mismatch() {
        let d = tempfile::tempdir().unwrap();
        let jp = d.path().join("old.json");
        // A snapshot tagged with a different schema version must be rejected
        // with a structured error (not silently mis-parsed).
        let mut v = serde_json::to_value(mk_snap()).unwrap();
        v["schema_version"] = serde_json::Value::String("1".into());
        fs::write(&jp, serde_json::to_string(&v).unwrap()).unwrap();
        let err = format!("{:#}", load_snapshot_any(&jp).unwrap_err());
        assert!(err.contains("schema_version"), "schema error: {err}");
        assert!(err.contains("\"1\""), "names the offending version: {err}");
    }
}
