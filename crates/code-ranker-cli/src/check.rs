//! `check` — the linter: evaluate rules (and, with `--baseline`, regressions),
//! render diagnostics (human / json / github / sarif / codequality), and the
//! `--suggest-config` current-values dump.

use crate::analyze::{analyze_input, load_snapshot_any, project_name};
use crate::cli::{AnalyzeArgs, OutputFormat};
use crate::config;
use anyhow::Result;
use code_ranker_graph::level_graph::LevelGraph;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

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
    focus: &[String],
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

    // `--focus`: scope the gate to the given files/folders. The whole project is
    // analyzed (the dependency graph needs it), but a violation outside the
    // focused paths is dropped entirely — so it is neither reported nor counted
    // toward the exit code. A locationless violation (e.g. a cycle whose breaking
    // edge couldn't be placed) can't be attributed to a path, so it is dropped too.
    if !focus.is_empty() {
        findings.retain(|v| {
            violation_rel_path(&v.location).is_some_and(|rel| focus_matches(rel, focus))
        });
    }

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
        focus,
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
    focus: &[String],
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
                focus,
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

/// Whether a violation's repo-relative path falls under one of the `--focus`
/// paths. A focus entry matches a file exactly or, treated as a folder, anything
/// beneath it (`crates/a/src` matches `crates/a/src/x.rs`). Leading `./` and a
/// trailing `/` on a focus entry are ignored so `./crates/a/` and `crates/a` are
/// equivalent.
fn focus_matches(rel: &str, focus: &[String]) -> bool {
    focus.iter().any(|f| {
        let f = f.trim_start_matches("./").trim_end_matches('/');
        !f.is_empty() && (rel == f || rel.starts_with(&format!("{f}/")))
    })
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
    focus: &[String],
    node_attributes: &BTreeMap<String, code_ranker_plugin_api::level::AttributeSpec>,
    cycle_kinds: &BTreeMap<String, code_ranker_plugin_api::level::CycleKindSpec>,
) {
    // A trailing scope note so the count is never mistaken for the whole project.
    let scope_note = if focus.is_empty() {
        String::new()
    } else {
        format!(" (focused on {})", focus.join(", "))
    };
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

/// The six threshold metrics, in display order.
const METRICS: [&str; 6] = ["cyclomatic", "cognitive", "hk", "fan_in", "fan_out", "loc"];

/// Print the current measured values per scope as ready-to-paste `code-ranker.toml`
/// threshold blocks: the per-unit worst value (`single`) and the graph-wide
/// average (`avg`). Lets a user pin today's numbers as a baseline that passes.
fn print_current_values(graphs: &BTreeMap<String, LevelGraph>, cycles: &config::CycleRules) {
    let Some(level) = graphs.get("files") else {
        return;
    };
    println!();
    println!("Current config — copy the blocks below into code-ranker.toml:");

    // Cycle budgets: today's count per kind (paste to forbid adding more).
    println!();
    println!(
        "# cycles: max allowed count per kind (today's count — raise only to allow more; false = off)"
    );
    println!("[rules.cycles]");
    for (key, kind, rule) in [
        ("mutual", "mutual", cycles.mutual),
        ("chain", "chain", cycles.chain),
    ] {
        if rule.is_off() {
            println!("{key:<12}= false");
        } else {
            let n = level.cycles.iter().filter(|c| c.kind == kind).count();
            println!("{key:<12}= {n}");
        }
    }

    // Thresholds: measured per-file maxima to pin as a baseline.
    println!();
    println!("# thresholds: the worst single file (max) per metric");
    print_scope_values("file", level);
}

/// Emit a `[rules.thresholds.<scope>]` block with the per-file metric maxima,
/// read from the flat node `attrs`.
fn print_scope_values(scope: &str, level: &LevelGraph) {
    let attr = |n: &code_ranker_plugin_api::node::Node, key: &str| -> f64 {
        match n.attrs.get(key) {
            Some(code_ranker_plugin_api::attrs::AttrValue::Int(i)) => *i as f64,
            Some(code_ranker_plugin_api::attrs::AttrValue::Float(f)) => *f,
            _ => 0.0,
        }
    };
    let mut max = [0f64; 6];
    let mut any = false;
    for n in &level.nodes {
        if n.kind == "external" {
            continue;
        }
        any = true;
        max[0] = max[0].max(attr(n, "cyclomatic"));
        max[1] = max[1].max(attr(n, "cognitive"));
        max[2] = max[2].max(attr(n, "hk"));
        max[3] = max[3].max(attr(n, "fan_in"));
        max[4] = max[4].max(attr(n, "fan_out"));
        max[5] = max[5].max(attr(n, "loc"));
    }
    if !any {
        return;
    }
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
mod tests {
    use super::*;

    fn viol(location: &str, line: Option<u32>) -> config::Violation {
        config::Violation {
            rule: "threshold.file.loc".into(),
            group: "SIZ".into(),
            graph: "files",
            location: location.into(),
            line,
            message: "source loc 1318 exceeds limit 150".into(),
            weight: 8.78,
            why: None,
            fix: None,
        }
    }

    #[test]
    fn focus_matches_file_exactly_and_folder_prefix() {
        let focus = vec![
            "crates/a/src/plugin.rs".to_string(),
            "crates/b/src".to_string(),
        ];
        // Exact file match.
        assert!(focus_matches("crates/a/src/plugin.rs", &focus));
        // Folder matches everything beneath it.
        assert!(focus_matches("crates/b/src/registry.rs", &focus));
        // Outside the focused paths.
        assert!(!focus_matches("crates/c/src/lib.rs", &focus));
        // A folder must match on a path boundary, not a bare prefix.
        assert!(!focus_matches("crates/b/src_extra.rs", &focus));
    }

    #[test]
    fn focus_matches_ignores_leading_dot_slash_and_trailing_slash() {
        let focus = vec!["./crates/a/".to_string()];
        assert!(focus_matches("crates/a/src/x.rs", &focus));
        assert!(focus_matches("crates/a", &focus));
    }

    #[test]
    fn annotation_location_maps_target_path_with_line() {
        assert_eq!(
            annotation_location("{target}/crates/a/src/x.rs", Some(42)),
            "file=crates/a/src/x.rs,line=42,"
        );
    }

    #[test]
    fn annotation_location_defaults_missing_line_to_one() {
        // Whole-file metrics carry no line → annotation pins to line 1.
        assert_eq!(
            annotation_location("{target}/src/x.rs", None),
            "file=src/x.rs,line=1,"
        );
    }

    #[test]
    fn annotation_location_empty_without_a_file_path() {
        // Locationless (cycle fallback) and non-`{target}` ids stay general.
        assert_eq!(annotation_location("", Some(5)), "");
        assert_eq!(annotation_location("ext:serde", None), "");
        assert_eq!(annotation_location("{target}/", Some(1)), "");
    }

    #[test]
    fn sarif_attaches_physical_location_from_violation() {
        let doc = sarif_document(
            &[viol("{target}/src/x.rs", Some(7))],
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        let loc = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "src/x.rs");
        assert_eq!(loc["region"]["startLine"], 7);
    }

    #[test]
    fn sarif_omits_location_when_no_path() {
        let doc = sarif_document(&[viol("", None)], &BTreeMap::new(), &BTreeMap::new());
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert!(v["runs"][0]["results"][0].get("locations").is_none());
    }

    #[test]
    fn codequality_issue_has_fingerprint_path_and_line() {
        let doc = codequality_document(&[viol("{target}/src/x.rs", Some(7))]);
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        let issue = &v[0];
        assert_eq!(issue["check_name"], "threshold.file.loc");
        assert_eq!(issue["severity"], "major");
        assert_eq!(issue["location"]["path"], "src/x.rs");
        assert_eq!(issue["location"]["lines"]["begin"], 7);
        // Stable identity = rule:location, no line (so a shift does not reopen it).
        assert_eq!(issue["fingerprint"], "threshold.file.loc:{target}/src/x.rs");
    }

    #[test]
    fn codequality_whole_file_metric_defaults_line_to_one() {
        // A whole-file metric has no line → CodeClimate needs one, default 1.
        let doc = codequality_document(&[viol("{target}/src/x.rs", None)]);
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(v[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn sarif_partial_fingerprint_is_rule_and_location() {
        let doc = sarif_document(
            &[viol("{target}/src/x.rs", Some(7))],
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
        let fp = &v["runs"][0]["results"][0]["partialFingerprints"];
        assert_eq!(
            fp["codeRankerRuleLocation/v1"],
            "threshold.file.loc:{target}/src/x.rs"
        );
    }

    #[test]
    fn sarif_partial_fingerprint_is_stable_across_line_shifts() {
        // The same finding at a different line keeps the same fingerprint, so a
        // code shift does not reopen it for the consumer.
        let at_7 = sarif_document(
            &[viol("{target}/src/x.rs", Some(7))],
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let at_42 = sarif_document(
            &[viol("{target}/src/x.rs", Some(42))],
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let fp = |doc: &str| -> String {
            let v: serde_json::Value = serde_json::from_str(doc).unwrap();
            v["runs"][0]["results"][0]["partialFingerprints"]["codeRankerRuleLocation/v1"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(fp(&at_7), fp(&at_42));
    }
}
