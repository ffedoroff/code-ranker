//! Machine-readable check output: SARIF 2.1.0 (GitHub code scanning) and the
//! GitLab Code Quality report. Both serialize the gate's violations into a
//! CI-ingestable document and share the same cross-run finding identity
//! (`(rule, location)`, no line number).
//!
//! This is a dependency-free leaf: it owns `DOCS_URL` and `violation_rel_path`,
//! which the parent `check` module imports back. Dependencies flow one way
//! (`check → sarif`); nothing here reaches into `super::`, so no parent ↔ child
//! cycle forms.

use crate::config;
use std::collections::BTreeMap;

/// Base URL for the published docs. Diagnostics pointers (`ref` lines, SARIF
/// `helpUri`) use absolute URLs so they're clickable from a terminal, a CI log,
/// or a report — not just from a repo checkout.
pub(crate) const DOCS_URL: &str = "https://github.com/ffedoroff/code-ranker/blob/main/docs";

/// The repo-relative path inside a violation `location` (`{target}/rel` →
/// `rel`), or `None` when it has no file path (e.g. a cycle whose breaking edge
/// couldn't be placed). Assumes `check` ran from the repo root, so a
/// target-relative path is also repo-relative — what both GitHub annotations
/// and SARIF `artifactLocation` expect.
pub(crate) fn violation_rel_path(location: &str) -> Option<&str> {
    location
        .strip_prefix("{target}/")
        .filter(|rel| !rel.is_empty())
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
            let doc = config::rule_doc(&v.rule, &v.language, node_attributes, cycle_kinds);
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
                    "codeRankerRuleLocation/v1": format!("{}:{}:{}", v.language, v.rule, v.location),
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
                "fingerprint": format!("{}:{}:{}", v.language, v.rule, v.location),
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
