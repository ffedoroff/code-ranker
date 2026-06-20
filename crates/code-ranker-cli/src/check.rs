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

/// Base URL for the published docs. Diagnostics pointers (`ref` lines, SARIF
/// `helpUri`) use absolute URLs so they're clickable from a terminal, a CI log,
/// or a report — not just from a repo checkout.
const DOCS_URL: &str = "https://github.com/ffedoroff/code-ranker/blob/main/docs";

/// `check` — the linter. Evaluate rules (and, with `--baseline`, regressions);
/// exit non-zero on any violation that fails the gate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_check(
    args: &AnalyzeArgs,
    cycle_rules: &[String],
    thresholds: &[String],
    focus_path: &[String],
    focus_rule: &[String],
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
            if let Some(level) = bgraphs.get_mut("files") {
                config::apply_cycle_rules(&mut level.cycles, &mut level.nodes, &a.rules.cycles);
            }
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

    // Scope the gate. `--focus-path` keeps violations under the given files/folders;
    // `--focus-rule` keeps violations of the given rule ids or concern groups. The
    // whole project is still analyzed, but a violation outside an active focus is
    // dropped — neither reported nor counted toward the exit code. With both set, a
    // violation must satisfy both (path AND rule). A locationless violation can't be
    // attributed to a path, so `--focus-path` drops it too.
    if !focus_path.is_empty() {
        findings.retain(|v| {
            violation_rel_path(&v.location).is_some_and(|rel| path_matches(rel, focus_path))
        });
    }
    if !focus_rule.is_empty() {
        findings.retain(|v| rule_matches(v, focus_rule));
    }
    let scope_note = focus_scope_note(focus_path, focus_rule);

    let total = findings.len();
    // Rank worst-first by breach magnitude; `--top` limits only what is
    // reported, never the exit code.
    findings.sort_by(|x, y| y.weight.total_cmp(&x.weight));
    let shown = match top {
        Some(n) => &findings[..n.min(findings.len())],
        None => &findings[..],
    };

    // Diagnostic copy (why / fix / title) is resolved from the active snapshot's
    // `files`-level specs — the metric `description`/`remediation` and cycle-kind
    // vocab — so no rule prose lives in the CLI.
    let files = a.snapshot.graphs.get("files");
    let empty_na: BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec> = BTreeMap::new();
    let empty_ck: BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec> = BTreeMap::new();
    let node_attributes = files.map(|g| &g.node_attributes).unwrap_or(&empty_na);
    let cycle_kinds = files.map(|g| &g.cycle_kinds).unwrap_or(&empty_ck);

    emit_diagnostics(
        shown,
        total,
        &plugin,
        &project,
        output_format,
        verdict,
        &scope_note,
        node_attributes,
        cycle_kinds,
    );

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
        let doc = config::rule_doc(&v.rule, node_attributes, cycle_kinds);
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

/// The repo-relative path inside a violation `location` (`{target}/rel` →
/// `rel`), or `None` when it has no file path (e.g. a cycle whose breaking edge
/// couldn't be placed). Assumes `check` ran from the repo root, so a
/// target-relative path is also repo-relative — what both GitHub annotations
/// and SARIF `artifactLocation` expect.
fn violation_rel_path(location: &str) -> Option<&str> {
    location
        .strip_prefix("{target}/")
        .filter(|rel| !rel.is_empty())
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

/// Whether a violation matches one of the `--focus-rule` entries. An entry matches
/// the full rule id (`threshold.file.hk`, `check.inline_tests_too_large`), the bare
/// id after the last dot (`inline_tests_too_large`), or the concern group (`TST`,
/// `CPL`) — so `--focus-rule TST` and `--focus-rule inline_tests_too_large` both work.
fn rule_matches(v: &config::Violation, focus: &[String]) -> bool {
    focus
        .iter()
        .any(|r| v.rule == *r || v.group == *r || v.rule.rsplit('.').next() == Some(r.as_str()))
}

/// The trailing "(focused on …)" note for the human header, covering whichever of
/// `--focus-path` / `--focus-rule` are active (empty when neither is).
fn focus_scope_note(focus_path: &[String], focus_rule: &[String]) -> String {
    let mut parts = Vec::new();
    if !focus_path.is_empty() {
        parts.push(format!("path {}", focus_path.join(", ")));
    }
    if !focus_rule.is_empty() {
        parts.push(format!("rule {}", focus_rule.join(", ")));
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
        let doc = config::rule_doc(&v.rule, node_attributes, cycle_kinds);
        println!("{}  ·  {}  ·  {} graph", v.rule, v.group, v.graph);
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
        let tune = config::rule_tuning(&v.rule);
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

/// Minimal SARIF 2.1.0 document. `ruleId` is the dotted rule id (e.g.
/// `threshold.file.loc`); the rules that actually fired are described under
/// `tool.driver.rules` (id, group, rationale, helpUri) so the report is self-documenting.
/// Each result carries a `partialFingerprints` entry keyed on `(rule, location)` (no
/// line number) so a consumer matches the same finding across runs even when code
/// shifts — the same identity `check --baseline` uses internally.
pub(crate) fn sarif_document(
    violations: &[config::Violation],
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    cycle_kinds: &BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) -> String {
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
            let doc = config::rule_doc(&v.rule, node_attributes, cycle_kinds);
            let title = doc
                .as_ref()
                .and_then(|d| d.title.clone())
                .unwrap_or_else(|| v.rule.clone());
            let why = doc
                .as_ref()
                .and_then(|d| d.why.clone())
                .or_else(|| v.why.clone())
                .unwrap_or_default();
            serde_json::json!({
                "id": v.rule,
                "shortDescription": { "text": title },
                "fullDescription": { "text": why },
                "helpUri": format!(
                    "{DOCS_URL}/code-ranker-cli/ERRORS.md#group-{}",
                    v.group.to_lowercase()
                ),
                "properties": { "group": v.group },
            })
        })
        .collect();
    let results: Vec<serde_json::Value> = violations
        .iter()
        .map(|v| {
            let mut result = serde_json::json!({
                "ruleId": v.rule,
                "level": "error",
                "message": { "text": v.summary() },
                // Stable cross-run identity for the consumer (GitHub code scanning,
                // SARIF viewers): the same `(rule, location)` signature `check
                // --baseline` matches on internally. The line number is deliberately
                // excluded, so shifting a finding up/down the file does not reopen it
                // as "new". The value is the readable composite key (no hashing) — a
                // metric finding has at most one `(rule, location)`, so it is unique.
                "partialFingerprints": {
                    "codeRankerRuleLocation/v1": format!("{}:{}", v.rule, v.location),
                },
                "properties": { "group": v.group, "graph": v.graph, "weight": v.weight },
            });
            // A physical location lets GitHub code scanning render the result
            // inline on the file/line. Whole-file metrics have no line → line 1.
            if let Some(rel) = violation_rel_path(&v.location) {
                result["locations"] = serde_json::json!([{
                    "physicalLocation": {
                        "artifactLocation": { "uri": rel },
                        "region": { "startLine": v.line.unwrap_or(1) },
                    }
                }]);
            }
            result
        })
        .collect();
    let doc = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "code-ranker",
                "informationUri": "https://github.com/ffedoroff/code-ranker",
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules,
            }},
            "results": results,
        }],
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

/// GitLab **Code Quality** report (the CodeClimate-derived JSON GitLab ingests as
/// `artifacts:reports:codequality`). A flat array of issues; GitLab renders them
/// in the MR widget / diff. Each issue carries the dotted rule id as `check_name`,
/// the human message, a `major` severity, the repo-relative `location.path` +
/// `lines.begin`, and a stable `fingerprint` keyed on `(rule, location)` — no line
/// number, so GitLab tracks the same finding across pipelines even when code
/// shifts (the same identity SARIF and `check --baseline` use). Unlike GitHub
/// SARIF this needs no feature flag and works on current GitLab.
pub(crate) fn codequality_document(violations: &[config::Violation]) -> String {
    let issues: Vec<serde_json::Value> = violations
        .iter()
        .map(|v| {
            serde_json::json!({
                "description": v.summary(),
                "check_name": v.rule,
                // Readable composite identity (no hashing) — a finding has at most
                // one (rule, location), so it is unique; line excluded so a shift
                // does not reopen it.
                "fingerprint": format!("{}:{}", v.rule, v.location),
                "severity": "major",
                "location": {
                    "path": violation_rel_path(&v.location).unwrap_or(v.location.as_str()),
                    "lines": { "begin": v.line.unwrap_or(1) },
                },
            })
        })
        .collect();
    serde_json::to_string_pretty(&issues).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
