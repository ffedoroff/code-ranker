//! `report` — analyze (or read) the input and write artifacts: JSON snapshot,
//! HTML viewer (diff with `--baseline`), and the advisory prompt / scorecard.

use crate::analyze::{analyze_input, load_snapshot_any};
use crate::cli::AnalyzeArgs;
use crate::{config, logger, recommend};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use code_split_graph::snapshot::Snapshot;
use std::path::{Path, PathBuf};

/// Built-in artifact path templates, used when neither a `--output.<fmt>` flag,
/// a `--output.<fmt>.path`, nor the `[output.<fmt>]` config section sets one.
const DEFAULT_JSON_PATH: &str = ".code-split/{ts}-{git-hash-3}.json";
const DEFAULT_HTML_PATH: &str = ".code-split/{ts}-{git-hash-3}.html";
/// The prompt defaults to a per-principle Markdown file; the scorecard is a
/// console overview and defaults to the stdout stream.
const DEFAULT_PROMPT_PATH: &str = ".code-split/{ts}-{git-hash-3}-{preset}.md";
const DEFAULT_SCORECARD_PATH: &str = "stdout";

/// Which `report` artifact formats were requested (flags + `.path` selectors).
pub(crate) struct ReportOutputs {
    pub(crate) json: bool,
    pub(crate) html: bool,
    pub(crate) prompt: bool,
    pub(crate) scorecard: bool,
    pub(crate) json_path: Option<String>,
    pub(crate) html_path: Option<String>,
    pub(crate) prompt_path: Option<String>,
    pub(crate) scorecard_path: Option<String>,
}

/// Recommendation knobs for the `prompt` / `scorecard` formats.
pub(crate) struct ReportReco {
    pub(crate) preset: Option<String>,
    pub(crate) severity: Vec<String>,
    pub(crate) top: Option<usize>,
    pub(crate) index: Option<usize>,
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
    let json_path = out.json_path.as_deref();
    let html_path = out.html_path.as_deref();
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
        && (reco.preset.is_some() || !reco.severity.is_empty() || reco.top.is_some())
    {
        anyhow::bail!(
            "--preset/--severity/--top apply only with --output.prompt or --output.scorecard"
        );
    }

    let a = analyze_input(args, &[], &[])?;

    // A json/html format is selected by a CLI flag (`--output.<fmt>` /
    // `--output.<fmt>.path`) or by config (`enabled`, else a configured `path`).
    // If NOTHING is selected across all formats, write json + html by default.
    let mut want_json = want_format(out.json, json_path, &a.output.json);
    let mut want_html = want_format(out.html, html_path, &a.output.html);
    if !want_json && !want_html && !want_prompt && !want_scorecard {
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
            .unwrap_or(DEFAULT_JSON_PATH);
        let dest = render_name(tpl, &target, commit, generated_at);
        let mut json = code_split_graph::serialize::to_canonical_string_pretty(snap)?;
        json.push('\n');
        write_artifact(&dest, &json, "json")?;
    }

    if want_html {
        let tpl = html_path
            .or(a.output.html.path.as_deref())
            .unwrap_or(DEFAULT_HTML_PATH);
        let mut dest = render_name(tpl, &target, commit, generated_at);
        // A baseline turns the HTML into a diff; mark the filename `…-diff.html`
        // (unless it goes to the stdout stream).
        if baseline_snap.is_some() && !is_stream(&dest) {
            dest = match dest.strip_suffix(".html") {
                Some(stem) => format!("{stem}-diff.html"),
                None => format!("{dest}-diff"),
            };
        }
        let html = code_split_viewer::render_html_viewer(baseline_snap.as_ref(), Some(snap));
        write_artifact(&dest, &html, "html")?;
    }

    if want_prompt || want_scorecard {
        write_recommendations(
            snap,
            &reco,
            want_prompt,
            want_scorecard,
            prompt_path,
            scorecard_path,
            &target,
            commit,
            generated_at,
        )?;
    }

    Ok(())
}

/// Write the recommendation artifacts (`prompt` / `scorecard`) for the analyzed
/// snapshot. Both read the `files` level; the prompt resolves a single principle
/// (explicit `--preset`, else the worst-violating one), the scorecard spans all.
#[allow(clippy::too_many_arguments)]
fn write_recommendations(
    snap: &Snapshot,
    reco: &ReportReco,
    want_prompt: bool,
    want_scorecard: bool,
    prompt_path: Option<&str>,
    scorecard_path: Option<&str>,
    target: &Path,
    commit: Option<&str>,
    generated_at: DateTime<Utc>,
) -> Result<()> {
    let level = snap
        .graphs
        .get("files")
        .context("snapshot has no `files` level to build recommendations from")?;

    if want_prompt {
        let preset_id = match &reco.preset {
            Some(p) => p.clone(),
            None => recommend::worst_preset(level, &snap.presets)
                .context("no presets in the snapshot to recommend from")?,
        };
        // The prompt takes a single tier; default `auto`.
        let sev = match reco.severity.as_slice() {
            [] => recommend::Severity::Auto,
            [one] => recommend::parse_severity(one)?,
            _ => anyhow::bail!(
                "--output.prompt takes a single --severity (info | warning | auto); the scorecard accepts several"
            ),
        };
        let md = recommend::compose_prompt(level, &snap.presets, &preset_id, sev, reco.top)?;
        let tpl = prompt_path.unwrap_or(DEFAULT_PROMPT_PATH);
        let dest = render_name(tpl, target, commit, generated_at).replace("{preset}", &preset_id);
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
            reco.preset.as_deref(),
        )?;
        let tpl = scorecard_path.unwrap_or(DEFAULT_SCORECARD_PATH);
        let dest = render_name(tpl, target, commit, generated_at);
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
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// A fixed instant so the `{ts}` expansion is deterministic in tests.
    fn fixed_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 4, 13, 59, 48).unwrap()
    }

    #[test]
    fn render_name_expands_placeholders_and_slugifies() {
        let out = render_name(
            "{project-dir}-{ts}.json",
            Path::new("/x/My_Project"),
            None,
            fixed_ts(),
        );
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
        let out = render_name("{ts}-{git-hash-3}.json", t, Some("69aa698abcde"), fixed_ts());
        assert!(out.ends_with("-69a.json"), "first 3 hash chars: {out}");
        // Full short hash.
        let full = render_name("{git-hash}.json", t, Some("69aa698abcde"), fixed_ts());
        assert_eq!(full, "69aa698abcde.json");
        // No git → zero fallback, still no leftover placeholder.
        let none = render_name("{git-hash-3}.json", t, None, fixed_ts());
        assert_eq!(none, "000.json");
    }

    /// Two artifacts of the same run share one `{ts}` — the snapshot's
    /// `generated_at` — rather than each re-reading the clock. This is the bug the
    /// `generated_at` anchoring fixes: json and html names must not drift apart.
    #[test]
    fn render_name_ts_is_stable_across_artifacts_of_one_run() {
        let t = Path::new("/x/proj");
        let at = fixed_ts();
        let json = render_name("{ts}-{git-hash-3}.json", t, Some("abc123def456"), at);
        let html = render_name("{ts}-{git-hash-3}.html", t, Some("abc123def456"), at);
        let json_ts = json.trim_end_matches("-abc.json");
        let html_ts = html.trim_end_matches("-abc.html");
        assert_eq!(json_ts, html_ts, "json and html share one stamp");
    }
}
