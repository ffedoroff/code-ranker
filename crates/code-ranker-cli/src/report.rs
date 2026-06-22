//! `report` — analyze (or read) the input and write artifacts: JSON snapshot,
//! HTML viewer (diff with `--baseline`), and the advisory prompt / scorecard.

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
    pub(crate) prompt: bool,
    pub(crate) scorecard: bool,
    pub(crate) json_path: Option<String>,
    pub(crate) html_path: Option<String>,
    pub(crate) sarif_path: Option<String>,
    pub(crate) codequality_path: Option<String>,
    pub(crate) prompt_path: Option<String>,
    pub(crate) scorecard_path: Option<String>,
}

/// Recommendation knobs for the `prompt` / `scorecard` formats.
pub(crate) struct ReportReco {
    /// Focus the `scorecard` / `prompt` on one axis — a metric / rule id (`hk`,
    /// `threshold.file.hk`) or a principle id (`LSP`). Resolved against both.
    pub(crate) focus_rule: Option<String>,
    /// Restrict the ranked modules to these repo-relative paths (folder = subtree).
    pub(crate) focus_path: Vec<String>,
    pub(crate) severity: Vec<String>,
    pub(crate) top: Option<usize>,
    pub(crate) index: Option<usize>,
    /// `--prompt <ID>`: print the named principle/metric prompt to stdout and exit.
    pub(crate) prompt_id: Option<String>,
    /// `--doc <ID>`: print the named principle/metric doc Markdown to stdout and exit.
    pub(crate) doc_id: Option<String>,
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
    // `--prompt <ID>` / `--doc <ID>`: analyze, print the one artifact to stdout,
    // and exit — a direct dump that bypasses the file-artifact flags entirely.
    if reco.prompt_id.is_some() || reco.doc_id.is_some() {
        return run_direct(args, &reco);
    }

    let json_path = out.json_path.as_deref();
    let html_path = out.html_path.as_deref();
    let sarif_path = out.sarif_path.as_deref();
    let codequality_path = out.codequality_path.as_deref();
    let prompt_path = out.prompt_path.as_deref();
    let scorecard_path = out.scorecard_path.as_deref();

    // The recommendation formats are flag-only (no `[output.<fmt>]` config) and
    // are never part of the default set.
    let want_prompt = out.prompt || prompt_path.is_some();
    let want_scorecard = out.scorecard || scorecard_path.is_some();

    // Validate the recommendation knobs before any analysis runs. `--index` is
    // intentionally unsupported — complain with a hint rather than a bare clap
    // "unknown flag" — and the other knobs only make sense for prompt/scorecard.
    if reco.index.is_some() {
        anyhow::bail!(
            "--index is not supported; use --top N instead (--top 1 = the single worst module)"
        );
    }
    if !want_prompt
        && !want_scorecard
        && (reco.focus_rule.is_some()
            || !reco.focus_path.is_empty()
            || !reco.severity.is_empty()
            || reco.top.is_some())
    {
        anyhow::bail!(
            "--focus-rule/--focus-path/--severity/--top apply only with --output.prompt or --output.scorecard"
        );
    }
    // `--severity` steers the scorecard only (tiers are a triage concern).
    if !reco.severity.is_empty() && !want_scorecard {
        anyhow::bail!("--severity applies only to --output.scorecard");
    }
    // The prompt is auto-targeted at the single worst module: it requires exactly
    // `--top 1` (prompts are long; for a broader view use --output.scorecard).
    if want_prompt && reco.top != Some(1) {
        anyhow::bail!(
            "--output.prompt requires --top 1 (it is auto-targeted at the single worst module)"
        );
    }

    let a = analyze_input(args, &[], &[])?;

    // A json/html/sarif format is selected by a CLI flag (`--output.<fmt>` /
    // `--output.<fmt>.path`) or by config (`enabled`, else a configured `path`).
    // If NOTHING is selected across all formats, write json + html by default.
    let mut want_json = want_format(out.json, json_path, &a.output.json);
    let mut want_html = want_format(out.html, html_path, &a.output.html);
    let want_sarif = want_format(out.sarif, sarif_path, &a.output.sarif);
    let want_codequality = want_format(out.codequality, codequality_path, &a.output.codequality);
    if !want_json
        && !want_html
        && !want_sarif
        && !want_codequality
        && !want_prompt
        && !want_scorecard
    {
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
        // Diagnostic copy (rule titles / descriptions) is resolved from the
        // reported snapshot's `files`-level specs — no rule prose in the CLI.
        let files = a.snapshot.graphs.get("files");
        let empty_na: std::collections::BTreeMap<
            String,
            code_ranker_plugin_api::level::AttributeSpec,
        > = Default::default();
        let empty_ck: std::collections::BTreeMap<
            String,
            code_ranker_plugin_api::level::CycleKindSpec,
        > = Default::default();
        let na = files.map(|g| &g.node_attributes).unwrap_or(&empty_na);
        let ck = files.map(|g| &g.cycle_kinds).unwrap_or(&empty_ck);
        let mut sarif = crate::check::sarif_document(&a.violations, na, ck);
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

    if want_prompt || want_scorecard {
        // A `--output.<fmt>.path` flag wins; otherwise the default template comes
        // from the merged config (always present from the built-in defaults).
        let prompt_tpl = prompt_path
            .or(a.output.prompt.path.as_deref())
            .expect("output.prompt.path from built-in defaults");
        let scorecard_tpl = scorecard_path
            .or(a.output.scorecard.path.as_deref())
            .expect("output.scorecard.path from built-in defaults");
        write_recommendations(
            snap,
            &reco,
            want_prompt,
            want_scorecard,
            prompt_tpl,
            scorecard_tpl,
            &target,
            commit,
            generated_at,
        )?;
    }

    Ok(())
}

/// `--prompt <ID>` / `--doc <ID>`: analyze the input and print one principle's AI
/// prompt or its doc Markdown to stdout, then exit. Standalone — no file artifacts
/// and none of the `--output.*` validation (so `--prompt HK --top 5` is fine).
fn run_direct(args: &AnalyzeArgs, reco: &ReportReco) -> Result<()> {
    if reco.prompt_id.is_some() && reco.doc_id.is_some() {
        anyhow::bail!("--prompt and --doc are mutually exclusive");
    }
    let a = analyze_input(args, &[], &[])?;
    let snap = &a.snapshot;

    // `--doc <ID>`: the resolved corpus Markdown (with any `[templates.…]` override).
    if let Some(id) = &reco.doc_id {
        let md = crate::templates::resolve_doc(snap, &a.templates, id)?;
        print!("{md}");
        if !md.ends_with('\n') {
            println!();
        }
        return Ok(());
    }

    // `--prompt <ID>`: compose the named principle/metric prompt (same builder as
    // `--output.prompt`, but for the id you name, to stdout).
    let id = reco.prompt_id.as_deref().expect("prompt_id is set");
    let level = snap
        .graphs
        .get("files")
        .context("snapshot has no `files` level to build a prompt from")?;
    let focus = recommend::resolve_focus(level, &snap.presets, id)?;
    let synth; // holds the metric-lens preset for the borrow below
    let (presets_for_prompt, preset_id): (&[recommend::Preset], String) = match &focus {
        recommend::Focus::Metric(m) => {
            synth = [recommend::synth_metric_preset(level, m)];
            (&synth, m.clone())
        }
        recommend::Focus::Principle(pid) => (&snap.presets, pid.clone()),
    };
    let md = recommend::compose_prompt(
        level,
        presets_for_prompt,
        &snap.prompt,
        &preset_id,
        recommend::Severity::Auto,
        reco.top,
        &reco.focus_path,
    )?;
    print!("{md}");
    Ok(())
}

/// Write the recommendation artifacts (`prompt` / `scorecard`) for the analyzed
/// snapshot. Both read the `files` level. `--focus` picks the lens: a metric frames
/// the output by the metric itself, a principle by that design principle; without
/// it the prompt auto-targets the worst-violating principle and the scorecard spans
/// all.
#[allow(clippy::too_many_arguments)]
fn write_recommendations(
    snap: &Snapshot,
    reco: &ReportReco,
    want_prompt: bool,
    want_scorecard: bool,
    prompt_tpl: &str,
    scorecard_tpl: &str,
    target: &Path,
    commit: Option<&str>,
    generated_at: DateTime<Utc>,
) -> Result<()> {
    let level = snap
        .graphs
        .get("files")
        .context("snapshot has no `files` level to build recommendations from")?;

    // Resolve `--focus-rule` once against both namespaces (metric / rule id /
    // principle id). `--focus-path` then narrows the ranked modules to a subtree.
    let focus = reco
        .focus_rule
        .as_deref()
        .map(|n| recommend::resolve_focus(level, &snap.presets, n))
        .transpose()?;

    if want_prompt {
        // Metric focus frames the prompt by a synthesized metric "preset" (no SOLID
        // principle); a principle focus targets that preset; no focus auto-targets
        // the worst-violating principle. `--top 1` is validated up front, so `Auto`
        // tier is irrelevant.
        let synth; // holds the metric-lens preset, if any, for the borrow below
        let (presets_for_prompt, preset_id): (&[recommend::Preset], String) = match &focus {
            Some(recommend::Focus::Metric(m)) => {
                synth = [recommend::synth_metric_preset(level, m)];
                (&synth, m.clone())
            }
            Some(recommend::Focus::Principle(id)) => (&snap.presets, id.clone()),
            None => (
                &snap.presets,
                recommend::worst_preset(level, &snap.presets)
                    .context("no presets in the snapshot to recommend from")?,
            ),
        };
        let md = recommend::compose_prompt(
            level,
            presets_for_prompt,
            &snap.prompt,
            &preset_id,
            recommend::Severity::Auto,
            reco.top,
            &reco.focus_path,
        )?;
        let dest =
            render_name(prompt_tpl, target, commit, generated_at).replace("{preset}", &preset_id);
        write_artifact(&dest, &md, "prompt")?;
    }

    if want_scorecard {
        let severities = if reco.severity.is_empty() {
            vec![recommend::Severity::Warning, recommend::Severity::Info]
        } else {
            reco.severity
                .iter()
                .map(|s| recommend::parse_severity(s))
                .collect::<Result<Vec<_>>>()?
        };
        let txt = recommend::render_scorecard(
            &snap.plugin,
            level,
            &snap.presets,
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
    logger::info(&format!("{kind}-report={}", path.display()));
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
