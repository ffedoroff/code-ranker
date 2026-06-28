//! `report` — analyze (or read) the input and write artifacts: JSON snapshot,
//! HTML viewer (diff with `--baseline`), and the advisory scorecard. The named
//! AI fix-prompt is available via `--prompt <ID>` (printed to stdout).

use crate::analyze::{analyze_input, load_snapshot_any};
use crate::cli::AnalyzeArgs;
use crate::{config, logger, recommend};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use code_ranker_graph::snapshot::Snapshot;
use std::path::{Path, PathBuf};

/// Which `report` artifact formats were requested (flags + `.path` selectors).
pub(crate) struct ReportOutputs {
    pub(crate) json: bool,
    pub(crate) html: bool,
    pub(crate) sarif: bool,
    pub(crate) codequality: bool,
    pub(crate) scorecard: bool,
    pub(crate) json_path: Option<String>,
    pub(crate) html_path: Option<String>,
    pub(crate) sarif_path: Option<String>,
    pub(crate) codequality_path: Option<String>,
    pub(crate) scorecard_path: Option<String>,
}

/// Recommendation knobs for the `scorecard` format and the `--prompt <ID>` output.
pub(crate) struct ReportReco {
    /// Focus the `scorecard` / `prompt` on one axis — a metric / rule id (`hk`,
    /// `threshold.file.hk`) or a principle id (`LSP`). Resolved against both.
    pub(crate) focus: Option<String>,
    /// Restrict the ranked modules to these repo-relative paths (folder = subtree).
    pub(crate) focus_path: Vec<String>,
    pub(crate) severity: Vec<String>,
    pub(crate) top: Option<usize>,
    pub(crate) index: Option<usize>,
    /// `--prompt <ID>`: print the named principle/metric prompt to stdout and exit.
    pub(crate) prompt_id: Option<String>,
    /// `--language <name>`: which language's graphs/principles/prompt to use for
    /// recommendations and scorecard. Required when a `--focus`/`--prompt` id is
    /// present in more than one language and the choice would be ambiguous.
    pub(crate) language: Option<String>,
}

/// `report` — analyze (or read) the input and write artifacts. Which formats are
/// written, and where, follows the `--output.<fmt>[.path]` flags and the
/// `[output.<fmt>]` config (see [`want_format`]).
pub(crate) fn run_report(
    args: &AnalyzeArgs,
    baseline: Option<&Path>,
    out: ReportOutputs,
    reco: ReportReco,
) -> Result<()> {
    // `--prompt <ID>`: analyze, print the prompt to stdout, and exit — a direct
    // dump that bypasses the file-artifact flags entirely.
    if reco.prompt_id.is_some() {
        return run_direct(args, &reco);
    }

    let json_path = out.json_path.as_deref();
    let html_path = out.html_path.as_deref();
    let sarif_path = out.sarif_path.as_deref();
    let codequality_path = out.codequality_path.as_deref();
    let scorecard_path = out.scorecard_path.as_deref();

    // The scorecard format is flag-only (no `[output.<fmt>]` config) and is never
    // part of the default set.
    let want_scorecard = out.scorecard || scorecard_path.is_some();

    // Validate the recommendation knobs before any analysis runs. `--index` is
    // intentionally unsupported — complain with a hint rather than a bare clap
    // "unknown flag" — and the other knobs only make sense for the scorecard (the
    // `--prompt <ID>` path is handled by `run_direct` above and never reaches here).
    if reco.index.is_some() {
        anyhow::bail!(
            "--index is not supported; use --top N instead (--top 1 = the single worst module)"
        );
    }
    if !want_scorecard
        && (reco.focus.is_some()
            || !reco.focus_path.is_empty()
            || !reco.severity.is_empty()
            || reco.top.is_some())
    {
        anyhow::bail!(
            "--focus/--focus-path/--severity/--top apply only with --output.scorecard (for a fix-prompt use --prompt <ID>)"
        );
    }
    // `--severity` steers the scorecard only (tiers are a triage concern).
    if !reco.severity.is_empty() && !want_scorecard {
        anyhow::bail!("--severity applies only to --output.scorecard");
    }

    let a = analyze_input(args, &[], &[])?;

    // A json/html/sarif format is selected by a CLI flag (`--output.<fmt>` /
    // `--output.<fmt>.path`) or by config (`enabled`, else a configured `path`).
    // If NOTHING is selected across all formats, write json + html by default.
    let mut want_json = want_format(out.json, json_path, &a.output.json);
    let mut want_html = want_format(out.html, html_path, &a.output.html);
    let want_sarif = want_format(out.sarif, sarif_path, &a.output.sarif);
    let want_codequality = want_format(out.codequality, codequality_path, &a.output.codequality);
    if !want_json && !want_html && !want_sarif && !want_codequality && !want_scorecard {
        want_json = true;
        want_html = true;
    }

    let snap = &a.snapshot;
    let target = PathBuf::from(&snap.target);
    let commit = snap.git.as_ref().map(|g| g.commit.as_str());
    // Single source of truth for `{ts}`: the snapshot's `generated_at`. Every
    // artifact this run writes (json, html, prompt, …) derives the same stamp,
    // and it matches the value embedded in the snapshot. For a snapshot input it
    // is the original analysis time, not the current clock.
    let generated_at = snap.generated_at;

    let baseline_snap = match baseline {
        Some(p) => Some(load_snapshot_any(p)?),
        None => None,
    };

    if want_json {
        let tpl = json_path
            .or(a.output.json.path.as_deref())
            .expect("output.json.path from built-in defaults");
        let dest = render_name(tpl, &target, commit, generated_at);
        let mut json = code_ranker_graph::serialize::to_canonical_string_pretty(snap)?;
        json.push('\n');
        write_artifact(&dest, &json, "json")?;
    }

    if want_html {
        let tpl = html_path
            .or(a.output.html.path.as_deref())
            .expect("output.html.path from built-in defaults");
        let mut dest = render_name(tpl, &target, commit, generated_at);
        // A baseline turns the HTML into a diff; mark the filename `…-diff.html`
        // (unless it goes to the stdout stream).
        if baseline_snap.is_some() && !is_stream(&dest) {
            dest = match dest.strip_suffix(".html") {
                Some(stem) => format!("{stem}-diff.html"),
                None => format!("{dest}-diff"),
            };
        }
        let html = code_ranker_viewer::render_html_viewer(baseline_snap.as_ref(), Some(snap));
        write_artifact(&dest, &html, "html")?;
    }

    if want_sarif {
        // Same SARIF 2.1.0 document `check --output-format sarif` emits, but
        // written as an artifact: the current rule violations (absolute — a
        // `--baseline` here only diffs the HTML, it does not filter SARIF).
        let tpl = sarif_path
            .or(a.output.sarif.path.as_deref())
            .expect("output.sarif.path from built-in defaults");
        let dest = render_name(tpl, &target, commit, generated_at);
        // Diagnostic copy is merged across all languages (last-wins); same
        // strategy as `check`'s human/GitHub/prompt diagnostics.
        let (na, ck) = crate::check::merged_specs_pub(&a.snapshot.languages);
        let mut sarif = crate::check::sarif_document(&a.violations, &na, &ck);
        sarif.push('\n');
        write_artifact(&dest, &sarif, "sarif")?;
    }

    if want_codequality {
        // GitLab Code Quality (CodeClimate) report of the current rule violations
        // — the same document `check --output-format codequality` emits, written
        // as an artifact for `artifacts:reports:codequality`. Absolute (a
        // `--baseline` here only diffs the HTML); GitLab dedups by fingerprint.
        let tpl = codequality_path
            .or(a.output.codequality.path.as_deref())
            .expect("output.codequality.path from built-in defaults");
        let dest = render_name(tpl, &target, commit, generated_at);
        let mut cq = crate::check::codequality_document(&a.violations);
        cq.push('\n');
        write_artifact(&dest, &cq, "codequality")?;
    }

    if want_scorecard {
        // A `--output.scorecard.path` flag wins; otherwise the default template
        // comes from the merged config (always present from the built-in defaults).
        let scorecard_tpl = scorecard_path
            .or(a.output.scorecard.path.as_deref())
            .expect("output.scorecard.path from built-in defaults");
        write_scorecard(snap, &reco, scorecard_tpl, &target, commit, generated_at)?;
    }

    Ok(())
}

/// `--prompt <ID>`: analyze the input and print one principle's AI fix-prompt to
/// stdout, then exit. Standalone — no file artifacts and none of the `--output.*`
/// validation (so `--prompt HK --top 5` is fine). Reference docs are a separate
/// concern, served analysis-free by the `docs` command.
fn run_direct(args: &AnalyzeArgs, reco: &ReportReco) -> Result<()> {
    let a = analyze_input(args, &[], &[])?;
    let snap = &a.snapshot;

    // `--prompt <ID>`: compose the named principle/metric prompt to stdout.
    let id = reco.prompt_id.as_deref().expect("prompt_id is set");
    let lang_snap = recommend::resolve_language_snap(snap, reco.language.as_deref(), Some(id))?;
    let level = lang_snap
        .graphs
        .get("files")
        .context("snapshot has no `files` level to build a prompt from")?;
    let focus = recommend::resolve_focus(level, &lang_snap.principles, id)?;
    let synth; // holds the metric-lens principle for the borrow below
    let (principles_for_prompt, principle_id): (&[recommend::Principle], String) = match &focus {
        recommend::Focus::Metric(m) => {
            synth = [recommend::synth_metric_principle(
                level,
                &lang_snap.principles,
                m,
            )];
            (&synth, m.clone())
        }
        recommend::Focus::Principle(pid) => (&lang_snap.principles, pid.clone()),
    };
    let md = recommend::compose_prompt(
        level,
        principles_for_prompt,
        &lang_snap.prompt,
        &principle_id,
        recommend::Severity::Auto,
        reco.top,
        &reco.focus_path,
    )?;
    print!("{md}");
    Ok(())
}

/// Write the console-triage `scorecard` artifact for the analyzed snapshot. Reads
/// the `files` level of the selected language. `--focus` picks the lens (a metric
/// frames it by the metric itself, a principle by that design principle); without
/// it the scorecard spans every principle.
fn write_scorecard(
    snap: &Snapshot,
    reco: &ReportReco,
    scorecard_tpl: &str,
    target: &Path,
    commit: Option<&str>,
    generated_at: DateTime<Utc>,
) -> Result<()> {
    let lang_snap =
        recommend::resolve_language_snap(snap, reco.language.as_deref(), reco.focus.as_deref())?;
    let level = lang_snap
        .graphs
        .get("files")
        .context("snapshot has no `files` level to build recommendations from")?;

    // Resolve `--focus` once against both namespaces (metric / rule id /
    // principle id). `--focus-path` then narrows the ranked modules to a subtree.
    let focus = reco
        .focus
        .as_deref()
        .map(|n| recommend::resolve_focus(level, &lang_snap.principles, n))
        .transpose()?;

    {
        let severities = if reco.severity.is_empty() {
            vec![recommend::Severity::Warning, recommend::Severity::Info]
        } else {
            reco.severity
                .iter()
                .map(|s| recommend::parse_severity(s))
                .collect::<Result<Vec<_>>>()?
        };
        // Show the plugin name(s) in the scorecard header — join all active
        // plugins, or just the selected language when one is picked.
        let plugin_label = reco.language.as_deref().unwrap_or_else(|| {
            snap.plugins
                .first()
                .map(String::as_str)
                .unwrap_or("unknown")
        });
        let txt = recommend::render_scorecard(
            plugin_label,
            level,
            &lang_snap.principles,
            &severities,
            reco.top,
            focus.as_ref(),
            &reco.focus_path,
        )?;
        let dest = render_name(scorecard_tpl, target, commit, generated_at);
        write_artifact(&dest, &txt, "scorecard")?;
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
    logger::summary(&format!("{kind}-report={}", path.display()));
    Ok(())
}

/// Expand filename-template placeholders:
/// `{project-dir}` (slugified target dir name), `{ts}` (the run's `generated_at`,
/// formatted as a local timestamp), `{git-hash}` (full short commit) and
/// `{git-hash-N}` (first N chars of it). `{ts}` comes from `generated_at` — not a
/// fresh clock read — so every artifact a run writes shares one stamp and it
/// matches the value embedded in the snapshot. When there is no git commit, the
/// hash falls back to zeros.
fn render_name(
    template: &str,
    target: &Path,
    commit: Option<&str>,
    generated_at: DateTime<Utc>,
) -> String {
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
    let ts = generated_at
        .with_timezone(&Local)
        .format("%Y%m%d-%H%M%S")
        .to_string();
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

#[cfg(test)]
#[path = "report_test.rs"]
mod tests;
