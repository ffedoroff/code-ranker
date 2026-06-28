//! `check` — the linter: evaluate rules (and, with `--baseline`, regressions),
//! render diagnostics (human / json / github / sarif / codequality), and the
//! `--suggest-config` current-values dump.

use crate::analyze::{analyze_input, load_snapshot_any, project_name};
use crate::cli::{AnalyzeArgs, OutputFormat};
use crate::config;
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

mod values;
use values::print_current_values;

// Machine-readable output (SARIF / Code Quality) lives in the `sarif` leaf
// submodule, which also owns the shared `DOCS_URL` const and `violation_rel_path`
// helper. Re-importing them here keeps every call site and the test module
// (`super::*`) compiling unchanged, with dependencies flowing one way
// (`check → sarif`) so no parent ↔ child cycle forms.
mod sarif;
use sarif::{DOCS_URL, violation_rel_path};
// Re-exported at crate-internal visibility so `report.rs` keeps calling these as
// `crate::check::sarif_document` / `codequality_document` unchanged.
pub(crate) use sarif::{codequality_document, sarif_document};

/// `check` — the linter. Evaluate rules (and, with `--baseline`, regressions);
/// exit non-zero on any violation that fails the gate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_check(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
    focus_path: &[String],
    focus: &[String],
    baseline: Option<&Path>,
    output_format: OutputFormat,
    top: Option<usize>,
    exit_zero: bool,
    suggest_config: bool,
) -> Result<()> {
    let a = analyze_input(args, cycle_rules, thresholds)?;
    let project = project_name(&a.snapshot.target);
    let plugins = a.snapshot.plugins.join(", ");

    // Without --baseline the gate is absolute: every violation counts. With
    // --baseline it is relative: only violations not already present in the
    // baseline (under the same rules) count — pre-existing ones are tolerated.
    let (mut findings, verdict) = match baseline {
        None => (a.violations, None),
        Some(bpath) => {
            let base = load_snapshot_any(bpath)?;
            let mut blanguages = base.languages.clone();
            for (lang, ls) in blanguages.iter_mut() {
                if let Some(level) = ls.graphs.get_mut("files") {
                    let cycles = a
                        .rules_by_lang
                        .get(lang)
                        .map(|r| r.cycles)
                        .unwrap_or_default();
                    config::apply_cycle_rules(&mut level.cycles, &mut level.nodes, &cycles);
                }
            }
            let base_v = config::check_violations_all(&blanguages, &a.rules_by_lang);
            // Dedup key includes language so violations from different languages
            // with identical rule+location don't collide.
            let sig =
                |v: &config::Violation| (v.language.clone(), v.rule.clone(), v.location.clone());
            let base_sigs: HashSet<(String, String, String)> = base_v.iter().map(sig).collect();
            let cur_sigs: HashSet<(String, String, String)> =
                a.violations.iter().map(sig).collect();
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

    // Scope the gate. `--focus-path` keeps violations under the given files/folders;
    // `--focus` keeps violations of the given rule ids or concern groups. The
    // whole project is still analyzed, but a violation outside an active focus is
    // dropped — neither reported nor counted toward the exit code. With both set, a
    // violation must satisfy both (path AND rule). A locationless violation can't be
    // attributed to a path, so `--focus-path` drops it too.
    if !focus_path.is_empty() {
        findings.retain(|v| {
            violation_rel_path(&v.location).is_some_and(|rel| path_matches(rel, focus_path))
        });
    }
    if !focus.is_empty() {
        findings.retain(|v| rule_matches(v, focus));
    }
    let scope_note = focus_scope_note(focus_path, focus);

    let total = findings.len();
    // Rank worst-first by breach magnitude; `--top` limits only what is
    // reported, never the exit code.
    findings.sort_by(|x, y| y.weight.total_cmp(&x.weight));
    let shown = match top {
        Some(n) => &findings[..n.min(findings.len())],
        None => &findings[..],
    };

    // Diagnostic copy (why / fix / title) is resolved from the snapshot's
    // `files`-level specs — the metric `description`/`remediation` and cycle-kind
    // vocab — so no rule prose lives in the CLI. Merge specs across all languages
    // (last-wins) so diagnostics work regardless of which language a violation
    // comes from.
    let (node_attributes, cycle_kinds) = merged_specs_pub(&a.snapshot.languages);

    emit_diagnostics(
        shown,
        total,
        &plugins,
        &project,
        output_format,
        verdict,
        &scope_note,
        &node_attributes,
        &cycle_kinds,
    );

    // Surface the current measured values as ready-to-paste config blocks only on
    // request (`--suggest-config`), human output only — machine formats stay pure.
    if suggest_config && matches!(output_format, OutputFormat::Human) {
        // Union the `files` graphs across all languages for the values dump.
        let all_graphs: BTreeMap<String, code_ranker_graph::level_graph::LevelGraph> = a
            .snapshot
            .languages
            .values()
            .flat_map(|ls| ls.graphs.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect();
        // The suggested `[rules.cycles]` block uses the first language's cycle
        // policy (the values dump is an aggregate convenience across languages).
        let cycles = a
            .rules_by_lang
            .values()
            .next()
            .map(|r| r.cycles)
            .unwrap_or_default();
        print_current_values(&all_graphs, &cycles);
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
#[allow(clippy::too_many_arguments)]
fn emit_diagnostics(
    violations: &[config::Violation],
    total: usize,
    plugin: &str,
    project: &str,
    format: OutputFormat,
    verdict: Option<&str>,
    scope_note: &str,
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    cycle_kinds: &BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) {
    match format {
        OutputFormat::Human => {
            print_human_diagnostics(
                violations,
                total,
                plugin,
                project,
                scope_note,
                node_attributes,
                cycle_kinds,
            );
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
                // GitHub Actions workflow-command annotation (rule id in the
                // title). `file=`/`line=` pin it to a spot when the violation
                // carries a path; otherwise it stays a general annotation.
                let loc = annotation_location(&v.location, v.line);
                println!(
                    "::error {loc}title=code-ranker {} ({})::{}",
                    v.rule,
                    v.graph,
                    v.summary()
                );
            }
        }
        OutputFormat::Sarif => println!(
            "{}",
            sarif_document(violations, node_attributes, cycle_kinds)
        ),
        OutputFormat::Codequality => println!("{}", codequality_document(violations)),
        OutputFormat::Prompt => {
            print!(
                "{}",
                render_prompt(violations, total, project, node_attributes, cycle_kinds)
            )
        }
    }
}

/// A self-contained Markdown AI fix-prompt built from the **gate's own violations**
/// (`--output-format prompt`). Because it is derived from the same violations that
/// failed the gate (the configured `rules.thresholds` / `rules.cycles`), it always
/// describes exactly what failed — no principle selection, no tier mismatch. Empty
/// when the gate passes, so an agent reads it as "nothing to do".
fn render_prompt(
    violations: &[config::Violation],
    total: usize,
    project: &str,
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    cycle_kinds: &BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) -> String {
    if total == 0 {
        return String::new();
    }
    let mut s = String::new();
    let _ = writeln!(s, "# Fix {total} code-ranker violation(s) in {project}\n");
    let _ = writeln!(
        s,
        "The modules below violate the rules configured in `code-ranker.toml`. Fix each \
         one, keeping existing behavior and public APIs intact.\n"
    );
    for v in violations {
        let doc = config::rule_doc(&v.rule, &v.language, node_attributes, cycle_kinds);
        let title = doc
            .as_ref()
            .and_then(|d| d.title.clone())
            .unwrap_or_else(|| v.rule.clone());
        let _ = writeln!(s, "## {title} ({})", v.group);
        // Prefer the repo-relative path; fall back to any non-`{target}` id.
        let module = violation_rel_path(&v.location)
            .map(str::to_string)
            .or_else(|| (!v.location.is_empty()).then(|| v.location.clone()));
        if let Some(m) = module {
            match v.line {
                Some(l) => {
                    let _ = writeln!(s, "- **Module:** `{m}` (line {l})");
                }
                None => {
                    let _ = writeln!(s, "- **Module:** `{m}`");
                }
            }
        }
        let _ = writeln!(s, "- **Issue:** {}", v.message);
        let why = v
            .why
            .clone()
            .or_else(|| doc.as_ref().and_then(|d| d.why.clone()));
        let fix = v
            .fix
            .clone()
            .or_else(|| doc.as_ref().and_then(|d| d.fix.clone()));
        if let Some(why) = why {
            let _ = writeln!(s, "- **Why:** {why}");
        }
        if let Some(fix) = fix {
            let _ = writeln!(s, "- **Fix:** {fix}");
        }
        let _ = writeln!(
            s,
            "- **Reference:** {DOCS_URL}/code-ranker-cli/ERRORS.md#group-{}",
            v.group.to_lowercase()
        );
        let _ = writeln!(s);
    }
    let _ = writeln!(s, "## Task\n");
    let _ = writeln!(
        s,
        "Address each finding above. After the fix, re-run `code-ranker check .` until the \
         gate passes."
    );
    s
}

/// Merge `node_attributes` and `cycle_kinds` specs across all languages for use
/// in diagnostic copy resolution. Last-wins per key is fine: the same metric
/// name carries the same description across languages by convention, and a
/// language-specific refinement is preferable to nothing.
pub(crate) fn merged_specs_pub(
    languages: &std::collections::BTreeMap<String, code_ranker_graph::snapshot::LanguageSnapshot>,
) -> (
    BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) {
    let mut na = BTreeMap::new();
    let mut ck = BTreeMap::new();
    for ls in languages.values() {
        if let Some(files) = ls.graphs.get("files") {
            na.extend(files.node_attributes.clone());
            ck.extend(files.cycle_kinds.clone());
        }
    }
    (na, ck)
}

/// Whether a violation's repo-relative path falls under one of the `--focus-path`
/// entries. An entry matches a file exactly or, treated as a folder, anything
/// beneath it (`crates/a/src` matches `crates/a/src/x.rs`). Leading `./` and a
/// trailing `/` on an entry are ignored so `./crates/a/` and `crates/a` are
/// equivalent.
fn path_matches(rel: &str, focus: &[String]) -> bool {
    focus.iter().any(|f| {
        let f = f.trim_start_matches("./").trim_end_matches('/');
        !f.is_empty() && (rel == f || rel.starts_with(&format!("{f}/")))
    })
}

/// Whether a violation matches one of the `--focus` entries. An entry matches
/// the full rule id (`threshold.file.hk`, `check.inline_tests_too_large`), the bare
/// id after the last dot (`inline_tests_too_large`), or the concern group (`TST`,
/// `CPL`) — so `--focus TST` and `--focus inline_tests_too_large` both work.
fn rule_matches(v: &config::Violation, focus: &[String]) -> bool {
    focus
        .iter()
        .any(|r| v.rule == *r || v.group == *r || v.rule.rsplit('.').next() == Some(r.as_str()))
}

/// The trailing "(focused on …)" note for the human header, covering whichever of
/// `--focus-path` / `--focus` are active (empty when neither is).
fn focus_scope_note(focus_path: &[String], focus: &[String]) -> String {
    let mut parts = Vec::new();
    if !focus_path.is_empty() {
        parts.push(format!("path {}", focus_path.join(", ")));
    }
    if !focus.is_empty() {
        parts.push(format!("rule {}", focus.join(", ")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" (focused on {})", parts.join("; "))
    }
}

/// GitHub workflow-command location params (`file=rel,line=N,`) for a violation,
/// or an empty string when it has no file path. Whole-file metrics have no line
/// (`None`) → default to line 1; cycles carry the breaking edge's line.
fn annotation_location(location: &str, line: Option<u32>) -> String {
    match violation_rel_path(location) {
        Some(rel) => format!("file={rel},line={},", line.unwrap_or(1)),
        None => String::new(),
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
    scope_note: &str,
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    cycle_kinds: &BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) {
    if total == 0 {
        println!("✓ code-ranker check: no violations in {project} ({plugin} plugin){scope_note}.");
        return;
    }

    println!("code-ranker check — {total} violation(s) in {project} ({plugin} plugin){scope_note}");
    if violations.len() < total {
        println!(
            "  showing the {} worst by severity; run without --top to see all",
            violations.len()
        );
    }
    println!(
        "Each finding below is self-contained — copy a block into an AI assistant to act on it."
    );
    println!("Full rule reference: {DOCS_URL}/code-ranker-cli/ERRORS.md\n");

    for v in violations {
        let doc = config::rule_doc(&v.rule, &v.language, node_attributes, cycle_kinds);
        println!(
            "{}  ·  {}  ·  {}  ·  {} graph",
            v.rule, v.language, v.group, v.graph
        );
        if !v.location.is_empty() {
            println!("  where  {}", v.location);
        }
        println!("  issue  {}", v.message);
        // A custom check carries its own copy on the violation; metric/cycle rules
        // resolve it from the snapshot specs. Prefer the violation's own copy.
        let why = v
            .why
            .clone()
            .or_else(|| doc.as_ref().and_then(|d| d.why.clone()));
        let fix = v
            .fix
            .clone()
            .or_else(|| doc.as_ref().and_then(|d| d.fix.clone()));
        if let Some(why) = why {
            println!("  why    {why}");
        }
        if let Some(fix) = fix {
            println!("  fix    {fix}");
        }
        let tune = config::rule_tuning(&v.rule, &v.language);
        if !tune.is_empty() {
            println!("  tune   {tune}");
        }
        println!(
            "  ref    {DOCS_URL}/code-ranker-cli/ERRORS.md#group-{}",
            v.group.to_lowercase()
        );
        println!();
    }

    // Tail breakdown by concern group so the end of the output summarizes at a glance.
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for v in violations {
        match counts.iter_mut().find(|(g, _)| *g == v.group.as_str()) {
            Some((_, n)) => *n += 1,
            None => counts.push((v.group.as_str(), 1)),
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

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
