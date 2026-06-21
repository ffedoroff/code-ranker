//! End-to-end fixture tests.
//!
//! For every language's fixture project (colocated with its language module in
//! the merged plugins crate at `crates/code-ranker-plugins/src/languages/<lang>/tests/sample/`),
//! run the built `code-ranker` binary and compare its JSON report against the
//! committed golden
//! `crates/code-ranker-plugins/src/languages/<lang>/tests/sample/code-ranker-report.json`.
//!
//! The committed golden keeps its RAW header (timestamp, command, git, versions,
//! absolute paths, timings). The comparison therefore:
//!   1. asserts the volatile fields that MUST differ between two runs actually
//!      differ (proof we compared a fresh run, not a stale copy);
//!   2. normalizes the volatile header **structure-preservingly** on BOTH sides
//!      — only scalar leaves are blanked (with a type tag); object keys, array
//!      lengths and leaf types are kept, so the comparison still enforces the
//!      *presence* and *shape* of every field, not just its value (e.g. a golden
//!      missing `git.origin`, or a field that changed type, still fails);
//!   3. compares the entire normalized structure character-for-character and
//!      requires a 100% match.
//!
//! Char-length contracts that structure preservation cannot express (the
//! `git.commit` `--short=12` width) are asserted explicitly in `assert_git_shape`.
//!
//! The graph itself (nodes/edges/cycles/stats) is already machine-independent —
//! the tool relativizes paths to the `{target}` placeholder — so it is compared
//! verbatim, which is where the real assertions about detected dependencies and
//! blind spots live.
//!
//! To refresh the goldens after an intentional change, see `docs/e2e.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Fields that MUST differ between the golden (captured earlier) and a fresh
/// run — otherwise we are not actually exercising the binary.
const MUST_CHANGE: &[&str] = &["generated_at"];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/crates/code-ranker-cli
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root is two levels above the crate manifest")
        .to_path_buf()
}

/// The fixture project for a language, colocated with its language module in the
/// merged plugins crate at `crates/code-ranker-plugins/src/languages/<lang>/tests/sample`.
fn sample_dir(lang: &str) -> PathBuf {
    repo_root()
        .join("crates")
        .join("code-ranker-plugins")
        .join("src")
        .join("languages")
        .join(lang)
        .join("tests")
        .join("sample")
}

/// Run the binary on the language's `sample/` project with its own config and
/// return the parsed JSON report.
fn run_report(lang: &str) -> Value {
    let root = repo_root();
    let sample = sample_dir(lang);
    let out_dir = tempfile::tempdir().expect("create temp output dir");

    let out_json = out_dir.path().join("fresh.json");
    let status = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true") // Rust sample resolves crates from cache
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-ranker.toml"))
        .arg(format!("--output.json.path={}", out_json.display()))
        .status()
        .expect("spawn code-ranker");
    assert!(status.success(), "code-ranker failed for sample `{lang}`");

    let text =
        std::fs::read_to_string(out_dir.path().join("fresh.json")).expect("read fresh report json");
    serde_json::from_str(&text).expect("parse fresh report json")
}

fn read_golden(lang: &str) -> Value {
    let path = sample_dir(lang).join("code-ranker-report.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse golden report json")
}

/// All header fields whose VALUES are volatile (env-/time-dependent) but whose
/// SHAPE is a contract: presence of every (nested) key, array lengths, and leaf
/// types must still match between a fresh run and the golden.
const NORMALIZED_HEADER: &[&str] = &[
    "generated_at",
    "command",
    "workspace",
    "target",
    "config_file",
    "versions",
    "roots",
    "git",
    "timings",
];

/// Structure-preserving normalization: recurse through a value and replace every
/// scalar *leaf* with a type-tagged sentinel, while keeping object keys and array
/// element counts intact. This filters out the volatile values yet still lets the
/// byte comparison enforce **presence** (a missing/extra key differs), **length**
/// (a different array/object size differs), and **leaf type** (string vs number).
fn normalize_leaves(v: &mut Value) {
    match v {
        Value::Object(map) => map.values_mut().for_each(normalize_leaves),
        Value::Array(arr) => arr.iter_mut().for_each(normalize_leaves),
        Value::String(_) => *v = Value::String("<str>".into()),
        Value::Number(_) => *v = Value::String("<num>".into()),
        Value::Bool(_) => *v = Value::String("<bool>".into()),
        Value::Null => *v = Value::String("<null>".into()),
    }
}

/// Normalize every volatile header field in place (structure-preserving), so the
/// later comparison checks shape, not values. Top-level presence is asserted
/// separately for a clearer error than a whole-document diff.
fn canonicalize(v: &mut Value, lang: &str) {
    let obj = v.as_object_mut().expect("report root is a JSON object");
    for key in NORMALIZED_HEADER {
        let field = obj
            .get_mut(*key)
            .unwrap_or_else(|| panic!("[{lang}] header field `{key}` missing from report"));
        normalize_leaves(field);
    }
}

/// Assert the shape of the dynamic `git` block on a fresh run: every field must
/// be present with the right type, and the commit must be a (≥12-char) hex
/// abbreviation. The *values* vary per checkout, so this is where we pin the
/// contract — the blanket `canonicalize` cannot (it would erase the shape too).
fn assert_git_shape(report: &Value, lang: &str) {
    let git = report
        .get("git")
        .unwrap_or_else(|| panic!("[{lang}] report has no `git` block"));
    let obj = git
        .as_object()
        .unwrap_or_else(|| panic!("[{lang}] `git` is not an object: {git:?}"));

    for field in ["branch", "commit", "dirty_files", "origin"] {
        assert!(
            obj.contains_key(field),
            "[{lang}] git.{field} missing — every git field must be present: {git:?}"
        );
    }

    let branch = obj["branch"]
        .as_str()
        .unwrap_or_else(|| panic!("[{lang}] git.branch is not a string: {:?}", obj["branch"]));
    assert!(!branch.is_empty(), "[{lang}] git.branch is empty");

    let commit = obj["commit"]
        .as_str()
        .unwrap_or_else(|| panic!("[{lang}] git.commit is not a string: {:?}", obj["commit"]));
    // We request `--short=12`; git may extend it to stay unambiguous but never
    // shortens it. A 7-char value (the old `--short` default) must fail here.
    assert!(
        commit.len() >= 12,
        "[{lang}] git.commit must be at least 12 chars (got {} in {commit:?})",
        commit.len()
    );
    assert!(
        commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "[{lang}] git.commit is not a hex hash: {commit:?}"
    );

    assert!(
        obj["dirty_files"].is_u64(),
        "[{lang}] git.dirty_files must be a non-negative integer: {:?}",
        obj["dirty_files"]
    );

    let origin = obj["origin"]
        .as_str()
        .unwrap_or_else(|| panic!("[{lang}] git.origin is not a string: {:?}", obj["origin"]));
    assert!(!origin.is_empty(), "[{lang}] git.origin is empty");
}

fn assert_sample_matches(lang: &str) {
    let mut fresh = run_report(lang);
    let mut golden = read_golden(lang);

    // 1. The fields that must change really changed.
    for key in MUST_CHANGE {
        let f = fresh.get(*key);
        let g = golden.get(*key);
        assert!(
            f.is_some() && g.is_some(),
            "[{lang}] volatile field `{key}` missing (fresh={f:?}, golden={g:?})"
        );
        assert_ne!(
            f, g,
            "[{lang}] field `{key}` did not change between golden and a fresh run — \
             stale comparison?"
        );
    }

    // 1b. The commit hash has a char-length contract (`--short=12`) that a
    // structure-preserving normalization cannot express, so check it explicitly
    // on the fresh, real-git output (alongside presence/type of every git field).
    assert_git_shape(&fresh, lang);

    // 2. Structure-preserving normalization of the volatile header on both sides:
    // values are blanked, but keys, array lengths and leaf types are kept — so
    // the comparison below still enforces presence and shape of every field.
    canonicalize(&mut fresh, lang);
    canonicalize(&mut golden, lang);

    // 3. Character-for-character comparison of the whole normalized structure.
    // serde_json's default map sorts keys, so both sides serialize identically.
    let fresh_s = serde_json::to_string_pretty(&fresh).unwrap();
    let golden_s = serde_json::to_string_pretty(&golden).unwrap();
    assert_eq!(
        fresh_s, golden_s,
        "[{lang}] normalized report differs from golden. \
         If this change is intentional, regenerate the goldens (see docs/e2e.md)."
    );
}

/// Run `report` on a language's `sample/` with extra args, capturing stdout and
/// stderr (instead of comparing a golden file). Used for the recommendation
/// formats (`scorecard` / `prompt`), which stream to stdout.
fn run_report_capture(lang: &str, extra: &[&str]) -> (bool, String, String) {
    let root = repo_root();
    let sample = sample_dir(lang);
    let out = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-ranker.toml"))
        .args(extra)
        .output()
        .expect("spawn code-ranker");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Run `check` on a language sample with its own config, capturing the outcome.
fn run_check_capture(lang: &str, extra: &[&str]) -> (bool, String, String) {
    let root = repo_root();
    let sample = sample_dir(lang);
    let out = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("check")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-ranker.toml"))
        .args(extra)
        .output()
        .expect("spawn code-ranker");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `check` is the gate. The Rust sample has an a ⇄ b mutual cycle, so the default
/// run fails (exit non-zero) and prints a self-contained human diagnostic.
#[test]
fn rust_sample_check_human_diagnostic() {
    let (ok, stdout, stderr) = run_check_capture("rust", &[]);
    assert!(!ok, "gate fails on the mutual cycle: {stderr}");
    let out = format!("{stdout}{stderr}");
    assert!(
        out.contains("cycle.mutual") && out.contains("a.rs") && out.contains("b.rs"),
        "human diagnostic names the cycle members: {out}"
    );
}

/// `--output-format json` emits the machine-readable violation list.
#[test]
fn rust_sample_check_json_violations() {
    let (_ok, stdout, stderr) = run_check_capture("rust", &["--output-format", "json"]);
    let v: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json: {e}: {stderr}"));
    let first = &v.as_array().expect("array")[0];
    // The sample has two cycles: the 2-node `a ⇄ b` mutual and the 3-node
    // `chain::one→two→three` chain. The chain is the more severe SCC, so it is
    // reported first.
    assert_eq!(first["rule"], "cycle.chain");
    assert_eq!(first["graph"], "files");
}

/// `--output-format sarif` emits a SARIF 2.1.0 document. Every result carries a
/// stable `partialFingerprints` entry keyed on `(rule, location)` (no line number)
/// so a consumer (GitHub code scanning, SARIF viewers) matches the same finding
/// across runs even when code shifts.
#[test]
fn rust_sample_check_sarif() {
    let (_ok, stdout, _e) = run_check_capture("rust", &["--output-format", "sarif"]);
    let v: Value = serde_json::from_str(&stdout).expect("sarif json");
    assert!(
        v["$schema"].as_str().unwrap_or_default().contains("sarif"),
        "sarif schema present: {stdout}"
    );
    assert!(v["runs"].is_array(), "sarif runs array");

    let results = v["runs"][0]["results"].as_array().expect("results array");
    assert!(
        !results.is_empty(),
        "sample fires at least one violation: {stdout}"
    );
    for r in results {
        let fp = r["partialFingerprints"]["codeRankerRuleLocation/v1"]
            .as_str()
            .unwrap_or_else(|| panic!("result has a versioned partial fingerprint: {r}"));
        // The fingerprint is `rule:location` — it encodes the rule id and the
        // file uri but never the line, so a shift does not reopen the finding.
        let rule = r["ruleId"].as_str().expect("ruleId");
        assert!(
            fp.starts_with(&format!("{rule}:")),
            "fingerprint encodes the rule id: {fp}"
        );
        if let Some(uri) = r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].as_str()
        {
            assert!(
                fp.ends_with(uri),
                "fingerprint encodes the file location: {fp} / {uri}"
            );
        }
    }
}

/// `report --output.sarif` emits the same SARIF 2.1.0 document as
/// `check --output-format sarif`: the linter's violations, written as an artifact.
/// `--output.sarif.path=stdout` streams it so json/html are not also produced.
#[test]
fn rust_sample_report_sarif() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--output.sarif.path=stdout"]);
    assert!(ok, "report does not gate, so it succeeds: {stderr}");
    let v: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("sarif: {e}: {stdout}"));
    assert!(
        v["$schema"].as_str().unwrap_or_default().contains("sarif"),
        "sarif schema present: {stdout}"
    );
    let results = v["runs"][0]["results"].as_array().expect("results array");
    assert!(
        !results.is_empty(),
        "the sample's violations are reported: {stdout}"
    );
    // Same finding identity as `check`: each result carries the versioned
    // `(rule, location)` partial fingerprint.
    for r in results {
        assert!(
            r["partialFingerprints"]["codeRankerRuleLocation/v1"].is_string(),
            "result has a versioned partial fingerprint: {r}"
        );
    }
}

/// `--output-format codequality` emits a GitLab Code Quality (CodeClimate) array:
/// each issue has a stable `fingerprint`, a repo-relative `location.path`, a
/// `lines.begin`, and a `severity` — the shape GitLab ingests as
/// `artifacts:reports:codequality`.
#[test]
fn rust_sample_check_codequality() {
    let (_ok, stdout, _e) = run_check_capture("rust", &["--output-format", "codequality"]);
    let v: Value = serde_json::from_str(&stdout).expect("codequality json");
    let issues = v.as_array().expect("array of issues");
    assert!(!issues.is_empty(), "sample fires issues: {stdout}");
    for i in issues {
        assert!(i["fingerprint"].is_string(), "stable fingerprint: {i}");
        assert!(i["location"]["path"].is_string(), "repo-relative path: {i}");
        assert!(
            i["location"]["lines"]["begin"].is_number(),
            "begin line: {i}"
        );
        assert_eq!(i["severity"], "major", "severity present: {i}");
    }
}

/// `report --output.codequality` writes the same Code Quality document as `check`,
/// as an artifact. `--output.codequality.path=stdout` streams it so json/html are
/// not also produced.
#[test]
fn rust_sample_report_codequality() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--output.codequality.path=stdout"]);
    assert!(ok, "report does not gate, so it succeeds: {stderr}");
    let v: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("codequality: {e}: {stdout}"));
    assert!(
        !v.as_array().expect("array").is_empty(),
        "the sample's violations are reported: {stdout}"
    );
}

/// `--output-format github` emits `::error` workflow annotations with file/line.
#[test]
fn rust_sample_check_github_annotations() {
    let (_ok, stdout, stderr) = run_check_capture("rust", &["--output-format", "github"]);
    let out = format!("{stdout}{stderr}");
    assert!(
        out.contains("::error") && out.contains("cycle.mutual"),
        "github annotation: {out}"
    );
}

/// `--suggest-config` prints today's measured values as paste-ready TOML blocks.
#[test]
fn rust_sample_check_suggest_config() {
    let (_ok, stdout, _e) = run_check_capture("rust", &["--suggest-config"]);
    assert!(
        stdout.contains("[rules.cycles]") && stdout.contains("[rules.thresholds.file]"),
        "suggested config blocks: {stdout}"
    );
    assert!(
        stdout.contains("mutual") && stdout.contains("chain"),
        "cycle rules listed: {stdout}"
    );
}

/// A `--baseline` run computes a relative verdict; against itself it is `neutral`
/// (no new violations).
#[test]
fn rust_sample_check_baseline_verdict_neutral() {
    let root = repo_root();
    let sample = sample_dir("rust");
    let tmp = std::env::temp_dir().join("cs-e2e-baseline-rust.json");
    // Capture a baseline snapshot.
    let report = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-ranker.toml"))
        .arg(format!("--output.json.path={}", tmp.display()))
        .output()
        .expect("spawn report");
    assert!(report.status.success(), "baseline report");
    let (_ok, stdout, stderr) = run_check_capture(
        "rust",
        &[
            "--baseline",
            tmp.to_str().unwrap(),
            "--output-format",
            "json",
        ],
    );
    let v: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json: {e}: {stderr}"));
    assert_eq!(
        v["verdict"], "neutral",
        "self-baseline is neutral: {stdout}"
    );
}

/// The `scorecard` format streams a per-principle table + worst-module list to
/// stdout. The Rust sample has two cycles (the `a ⇄ b` mutual and the 3-node
/// `chain`) and no metric breaches, so ADP is the only principle with violations
/// and tops the table.
#[test]
fn rust_sample_scorecard_triage() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--output.scorecard"]);
    assert!(ok, "scorecard run failed: {stderr}");
    assert!(
        stdout.contains("scorecard  (rust, 25 files)"),
        "header with file count: {stdout}"
    );
    assert!(
        stdout.contains("ADP") && stdout.contains("Acyclic Dependencies"),
        "ADP principle row present: {stdout}"
    );
    assert!(stdout.contains("WORST MODULES"), "worst-modules section");
    assert!(
        stdout.contains("src/a.rs") && stdout.contains("src/b.rs") && stdout.contains("cycle"),
        "the two cycle members are listed as cycle breaches: {stdout}"
    );
    assert!(
        stdout.contains("--output.prompt.path=… --top 1"),
        "next-step hint points at the auto-prompt: {stdout}"
    );
}

/// With no `--preset`, the prompt auto-picks the worst-violating principle (ADP
/// here) and lists the worst cycle's members + their connections — the same
/// Markdown the HTML viewer's Prompt Generator emits. The 3-node `chain` SCC
/// outranks the 2-node `a ⇄ b` mutual, so it is the cycle shown.
#[test]
fn rust_sample_prompt_auto_picks_worst_principle() {
    let (ok, stdout, stderr) =
        run_report_capture("rust", &["--output.prompt.path=stdout", "--top", "1"]);
    assert!(ok, "prompt run failed: {stderr}");
    assert!(
        stdout.starts_with("# ADP — Acyclic Dependencies Principle"),
        "auto-picked ADP as the title heading: {stdout}"
    );
    assert!(
        stdout.contains("## Modules in a dependency cycle"),
        "cycle-modules section"
    );
    assert!(
        stdout.contains("- `src/chain/one.rs`")
            && stdout.contains("- `src/chain/two.rs`")
            && stdout.contains("- `src/chain/three.rs`"),
        "the worst cycle (3-node chain) members listed with cleaned paths: {stdout}"
    );
    assert!(
        stdout.contains("## Connections — common"),
        "ADP pre-selects the `common` connection set"
    );
    assert!(
        stdout.contains(".code-ranker/<YYYYMMDD-HHMMSS>-ADP.md"),
        "save-report instruction carries the preset id: {stdout}"
    );
}

/// `report --prompt <ID>` prints the named principle's prompt to stdout directly
/// (the explicit counterpart of `--output.prompt`, which auto-targets the worst),
/// honouring `--top`. Unlike `--output.prompt` it does NOT require `--top 1`.
#[test]
fn rust_sample_prompt_flag_targets_named_principle() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--prompt", "SRP", "--top", "3"]);
    assert!(ok, "--prompt run failed: {stderr}");
    assert!(
        stdout.starts_with("# SRP — Single Responsibility Principle"),
        "named principle prompt, not the auto-worst: {stdout}"
    );
    assert!(
        stdout.contains("## Summary") && stdout.contains("## Task"),
        "the prompt scaffolding is composed: {stdout}"
    );
}

/// `report --doc <ID>` prints the embedded corpus Markdown for a principle/metric
/// directly. `HK` is a metric (its doc lives in `base/`, reached via the metric's
/// remediation URL), exercising the metric-doc resolution path.
#[test]
fn rust_sample_doc_flag_prints_embedded_markdown() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--doc", "HK"]);
    assert!(ok, "--doc run failed: {stderr}");
    assert!(
        stdout.starts_with("# HK — Henry-Kafura Coupling"),
        "embedded HK doc printed: {stdout}"
    );
    // A principle id resolves too (SRP → its own rust/ corpus doc).
    let (ok2, stdout2, _) = run_report_capture("rust", &["--doc", "SRP"]);
    assert!(
        ok2 && stdout2.contains("Single Responsibility"),
        "SRP doc: {stdout2}"
    );
}

/// `--focus-rule <metric>` frames the scorecard by that metric. `--focus-rule cycle`
/// shows the dependency-cycle members (the ADP view) without the principle table.
#[test]
fn rust_sample_scorecard_focus_metric() {
    let (ok, stdout, stderr) =
        run_report_capture("rust", &["--output.scorecard", "--focus-rule", "cycle"]);
    assert!(ok, "focused scorecard run failed: {stderr}");
    assert!(
        stdout.contains("scorecard  (rust, 25 files)"),
        "header present: {stdout}"
    );
    assert!(
        stdout.contains("WORST MODULES") && stdout.contains("src/chain/one.rs"),
        "cycle members ranked under the focused metric: {stdout}"
    );
}

/// `--focus-rule HK` (a metric, by value) frames the output by the metric itself —
/// no SOLID principle (the Liskov row the hk-ranking preset would otherwise show).
/// Also accepts the full threshold rule id `threshold.file.hk`.
#[test]
fn rust_sample_scorecard_focus_metric_hides_principle() {
    for rule in ["HK", "threshold.file.hk"] {
        let (ok, stdout, stderr) =
            run_report_capture("rust", &["--output.scorecard", "--focus-rule", rule]);
        assert!(ok, "metric-lens scorecard failed for {rule}: {stderr}");
        assert!(
            stdout.contains("focus: HK"),
            "names the focused metric for {rule}: {stdout}"
        );
        assert!(
            !stdout.contains("Liskov"),
            "metric lens must not surface the SOLID principle for {rule}: {stdout}"
        );
    }
}

/// `--focus-path` restricts the ranked modules to a subtree (whole project still
/// analyzed). Modules outside the path are absent from the worst-modules list.
#[test]
fn rust_sample_scorecard_focus_path_scopes_modules() {
    let (ok, stdout, stderr) = run_report_capture(
        "rust",
        &[
            "--output.scorecard",
            "--focus-rule",
            "hk",
            "--focus-path",
            "src/chain",
        ],
    );
    assert!(ok, "focus-path scorecard failed: {stderr}");
    assert!(
        stdout.contains("src/chain/"),
        "lists modules under the focus path: {stdout}"
    );
    assert!(
        !stdout.contains("src/a.rs"),
        "modules outside the focus path are excluded: {stdout}"
    );
}

/// An unknown `--focus-rule` name is a hard error naming both namespaces.
#[test]
fn rust_sample_scorecard_unknown_focus() {
    let (ok, _stdout, stderr) =
        run_report_capture("rust", &["--output.scorecard", "--focus-rule", "nope"]);
    assert!(!ok, "unknown focus must fail");
    assert!(
        stderr.contains("unknown --focus-rule 'nope'"),
        "actionable error: {stderr}"
    );
}

/// `check --output-format prompt` gates AND, on failure, prints a Markdown
/// fix-prompt built from the gate's own violations (the cycle here). One command,
/// no `||`, exit non-zero.
#[test]
fn rust_sample_check_prompt_format() {
    let (ok, stdout, stderr) = run_check_capture("rust", &["--output-format", "prompt"]);
    assert!(!ok, "gate still fails on the cycle: {stderr}");
    assert!(
        stdout.starts_with("# Fix") && stdout.contains("code-ranker violation"),
        "markdown prompt heading: {stdout}"
    );
    assert!(
        stdout.contains("cycle") && stdout.contains("a.rs"),
        "describes the failing cycle: {stdout}"
    );
    assert!(
        stdout.contains("**Fix:**") && stdout.contains("## Task"),
        "carries fix guidance and a task section: {stdout}"
    );
}

/// `--output.prompt` is auto-targeted at the single worst module, so it requires
/// exactly `--top 1`; without it the run errors with a pointer to the scorecard.
#[test]
fn rust_sample_prompt_requires_top1() {
    let (ok, _stdout, stderr) = run_report_capture("rust", &["--output.prompt.path=stdout"]);
    assert!(!ok, "prompt without --top 1 must fail");
    assert!(
        stderr.contains("--output.prompt requires --top 1"),
        "actionable error: {stderr}"
    );
}

/// `--index` is rejected with a hint to use `--top`.
#[test]
fn rust_sample_report_rejects_index() {
    let (ok, _stdout, stderr) =
        run_report_capture("rust", &["--output.prompt.path=stdout", "--index", "0"]);
    assert!(!ok, "--index must fail");
    assert!(
        stderr.contains("--index is not supported") && stderr.contains("--top"),
        "actionable error: {stderr}"
    );
}

/// The recommendation knobs only apply with a `prompt` / `scorecard` format.
#[test]
fn rust_sample_report_rejects_stray_reco_flags() {
    let (ok, _stdout, stderr) = run_report_capture("rust", &["--focus-rule", "hk"]);
    assert!(
        !ok,
        "--focus-rule without a prompt/scorecard format must fail"
    );
    assert!(
        stderr.contains("apply only with --output.prompt or --output.scorecard"),
        "actionable error: {stderr}"
    );
}

#[test]
fn rust_sample_matches_golden() {
    assert_sample_matches("rust");
}

#[test]
fn python_sample_matches_golden() {
    assert_sample_matches("python");
}

#[test]
fn javascript_sample_matches_golden() {
    assert_sample_matches("javascript");
}

#[test]
fn typescript_sample_matches_golden() {
    assert_sample_matches("typescript");
}

#[test]
fn go_sample_matches_golden() {
    assert_sample_matches("go");
}

#[test]
fn c_sample_matches_golden() {
    assert_sample_matches("c");
}

#[test]
fn cpp_sample_matches_golden() {
    assert_sample_matches("cpp");
}

#[test]
fn csharp_sample_matches_golden() {
    assert_sample_matches("csharp");
}

// Markdown is documentation, not code — it produces only `loc` + the doc link
// graph (coupling/cycles), none of the central code metrics — so it is verified
// by its golden but is NOT in `LANGS` (the all-central-metrics invariant).
#[test]
fn markdown_sample_matches_golden() {
    assert_sample_matches("markdown");
}

/// Read a committed golden SARIF document for a language's `check` output.
fn read_check_sarif_golden(lang: &str) -> Value {
    let path = sample_dir(lang).join("code-ranker-check.sarif");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse golden sarif")
}

/// `check --output-format sarif` must match the committed golden for the language,
/// char-for-char, after blanking the one volatile field — `tool.driver.version`
/// (the crate version, which bumps every release). Everything else — the results,
/// their `partialFingerprints`, and the fired-rules catalog — is deterministic and
/// `{target}`-relative, so it is compared verbatim.
fn assert_check_sarif_matches_golden(lang: &str) {
    let (_ok, stdout, stderr) = run_check_capture(lang, &["--output-format", "sarif"]);
    let mut fresh: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("[{lang}] fresh sarif: {e}: {stderr}"));
    let mut golden = read_check_sarif_golden(lang);

    // The one volatile field must be present and carry the live crate version;
    // it is then blanked on both sides so a release bump does not churn the golden.
    assert_eq!(
        fresh["runs"][0]["tool"]["driver"]["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "[{lang}] sarif tool.driver.version must be the live crate version"
    );
    for doc in [&mut fresh, &mut golden] {
        doc["runs"][0]["tool"]["driver"]["version"] = Value::String("<ver>".into());
    }

    // serde_json sorts object keys, so both sides serialize identically.
    let fresh_s = serde_json::to_string_pretty(&fresh).unwrap();
    let golden_s = serde_json::to_string_pretty(&golden).unwrap();
    assert_eq!(
        fresh_s, golden_s,
        "[{lang}] check SARIF differs from golden. If intentional, regenerate it (see docs/e2e.md)."
    );
}

#[test]
fn rust_sample_check_sarif_matches_golden() {
    assert_check_sarif_matches_golden("rust");
}

#[test]
fn python_sample_check_sarif_matches_golden() {
    assert_check_sarif_matches_golden("python");
}

#[test]
fn javascript_sample_check_sarif_matches_golden() {
    assert_check_sarif_matches_golden("javascript");
}

#[test]
fn typescript_sample_check_sarif_matches_golden() {
    assert_check_sarif_matches_golden("typescript");
}

/// `check --output-format codequality` must match the committed golden for the
/// language, char-for-char. The Code Quality array has no volatile fields (no tool
/// version), is `{target}`-relative, and serde sorts object keys, so it is compared
/// verbatim.
fn assert_check_codequality_matches_golden(lang: &str) {
    let (_ok, stdout, stderr) = run_check_capture(lang, &["--output-format", "codequality"]);
    let fresh: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("[{lang}] fresh codequality: {e}: {stderr}"));
    let path = sample_dir(lang).join("code-ranker-check.codequality.json");
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display())),
    )
    .expect("parse golden codequality");
    assert_eq!(
        serde_json::to_string_pretty(&fresh).unwrap(),
        serde_json::to_string_pretty(&golden).unwrap(),
        "[{lang}] check Code Quality differs from golden. If intentional, regenerate it (see docs/e2e.md)."
    );
}

#[test]
fn rust_sample_check_codequality_matches_golden() {
    assert_check_codequality_matches_golden("rust");
}

#[test]
fn python_sample_check_codequality_matches_golden() {
    assert_check_codequality_matches_golden("python");
}

#[test]
fn javascript_sample_check_codequality_matches_golden() {
    assert_check_codequality_matches_golden("javascript");
}

#[test]
fn typescript_sample_check_codequality_matches_golden() {
    assert_check_codequality_matches_golden("typescript");
}

/// Every language whose golden is committed.
const LANGS: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "c",
    "cpp",
    "csharp",
];

/// Central metrics (`metric_specs` + `coupling_specs`) the analyzer does NOT
/// produce for a given language, so they are legitimately absent from that
/// language's golden. Each is a deliberate, documented gap — keep in lock-step
/// with the "Per-language metric scope" table in `docs/e2e.md`. A stale entry
/// (the analyzer started emitting the metric) is itself a test failure, so this
/// list cannot silently drift.
const COVERAGE_EXCEPTIONS: &[(&str, &[&str])] = &[
    // `tloc` is genuinely 0 for non-Rust: only the Rust pass strips `#[cfg(test)]`
    // items, so there are no test lines to count elsewhere.
    (
        "tloc",
        &[
            "python",
            "javascript",
            "typescript",
            "go",
            "c",
            "cpp",
            "csharp",
        ],
    ),
    // C has no closures/lambdas, so the `closures` counter is always 0.
    ("closures", &["c"]),
];

fn is_excepted(metric: &str, lang: &str) -> bool {
    COVERAGE_EXCEPTIONS
        .iter()
        .any(|(m, langs)| *m == metric && langs.contains(&lang))
}

/// True if `metric` is non-zero on at least one internal (non-external) file node
/// of this golden.
fn metric_present(golden: &Value, metric: &str) -> bool {
    golden["graphs"]["files"]["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .any(|n| {
            !n["external"].as_bool().unwrap_or(false)
                && n.get(metric)
                    .and_then(Value::as_f64)
                    .is_some_and(|v| v != 0.0)
        })
}

/// Coverage invariant: every centrally-computed metric must be exercised with a
/// non-zero value in EVERY language's golden, except the documented per-language
/// gaps in `COVERAGE_EXCEPTIONS`.
///
/// This is the guard the root-vs-sum bug slipped past: a metric that silently
/// reads its no-signal value gets pruned out of the golden, and without this test
/// nothing notices it is unexercised. The catalog is sourced from the spec
/// functions themselves, so a newly-added metric is automatically required to
/// appear in a golden — add a fixture that produces it, or (if the analyzer
/// cannot) a documented `COVERAGE_EXCEPTIONS` entry.
#[test]
fn every_central_metric_is_exercised_per_language() {
    let (complexity, _) = code_ranker_graph::metric_specs();
    let (coupling, _) = code_ranker_graph::coupling_specs();
    // `cycle` is a string classification ("mutual"/"chain"), not a numeric metric;
    // its per-kind coverage is guarded by the verbatim golden match, so exclude it
    // from this numeric-presence catalog.
    let catalog: Vec<String> = complexity
        .keys()
        .chain(coupling.keys())
        .filter(|k| *k != "cycle")
        .cloned()
        .collect();

    let goldens: Vec<(&str, Value)> = LANGS.iter().map(|l| (*l, read_golden(l))).collect();

    let mut unexercised = Vec::new();
    let mut stale_exceptions = Vec::new();
    for (lang, golden) in &goldens {
        for metric in &catalog {
            let present = metric_present(golden, metric);
            match (is_excepted(metric, lang), present) {
                (false, false) => unexercised.push(format!("{lang}:{metric}")),
                // An exception that is no longer absent — the analyzer now emits
                // it, so the gap closed and the entry must be removed.
                (true, true) => stale_exceptions.push(format!("{lang}:{metric}")),
                _ => {}
            }
        }
    }

    assert!(
        unexercised.is_empty(),
        "central metrics never exercised (non-zero) in a language's golden — \
         unguarded against the root-vs-sum class of bug. Add a fixture that \
         produces them, or a documented COVERAGE_EXCEPTIONS entry + docs/e2e.md \
         row. Missing: {unexercised:?}"
    );
    assert!(
        stale_exceptions.is_empty(),
        "stale COVERAGE_EXCEPTIONS: these metrics are now emitted for the listed \
         language, so the exception (and its docs/e2e.md row) must be removed: \
         {stale_exceptions:?}"
    );
}

/// A user-defined `[metrics.<key>]` CEL formula is computed per node and emitted
/// as a first-class metric — value plus its `node_attributes` spec. This is the
/// declarative-metric path: a metric the engine never hardcodes, added purely in
/// config (here `comment_ratio = cloc / sloc * 100`).
#[test]
fn user_defined_metric_is_computed_and_emitted() {
    let dir = tempfile::tempdir().expect("temp dir");
    let p = dir.path();
    // cloc = 2 comment lines, sloc = 4 code lines → comment_ratio = 50.
    std::fs::write(
        p.join("m.py"),
        "# a comment line\n# another comment\ndef f(x):\n    return x + 1\n\n\ndef g(y):\n    return y * 2\n",
    )
    .unwrap();
    std::fs::write(
        p.join("code-ranker.toml"),
        "[metrics.comment_ratio]\n\
         formula_cel = \"sloc > 0.0 ? cloc / sloc * 100.0 : 0.0\"\n\
         label = \"Comments %\"\n\
         direction = \"higher_better\"\n\
         group = \"loc\"\n",
    )
    .unwrap();
    let out = p.join("out.json");
    let status = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(p)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(".")
        .arg("--plugin")
        .arg("python")
        .arg("--config")
        .arg(p.join("code-ranker.toml"))
        .arg(format!("--output.json.path={}", out.display()))
        .status()
        .expect("spawn code-ranker");
    assert!(status.success(), "report should succeed");

    let v: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let files = &v["graphs"]["files"];
    assert!(
        files["node_attributes"]["comment_ratio"].is_object(),
        "the user metric must appear in node_attributes (renders as a column)"
    );
    let node = files["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"].as_str().unwrap_or("").ends_with("m.py"))
        .expect("file node present");
    assert_eq!(
        node["comment_ratio"],
        serde_json::json!(50),
        "comment_ratio = cloc(2) / sloc(4) * 100"
    );
}

/// A graph-scope (aggregate) `[metrics]` entry reduces a metric across all nodes
/// via `agg(key, reducer, population)` and lands in the level `stats` block —
/// the declarative analytics path (percentiles/means as user config).
#[test]
fn user_defined_aggregate_lands_in_stats() {
    let dir = tempfile::tempdir().expect("temp dir");
    let p = dir.path();
    // Two files with different branch counts → different cyclomatic.
    std::fs::write(
        p.join("a.py"),
        "def f(x):\n    if x:\n        return 1\n    return 0\n",
    )
    .unwrap();
    std::fs::write(p.join("b.py"), "def g(y):\n    return y\n").unwrap();
    std::fs::write(
        p.join("code-ranker.toml"),
        "[metrics.cyc_mean]\n\
         scope = \"graph\"\n\
         formula_cel = \"agg('cyclomatic', 'avg', 'not_empty')\"\n",
    )
    .unwrap();
    let out = p.join("out.json");
    let status = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(p)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(".")
        .arg("--plugin")
        .arg("python")
        .arg("--config")
        .arg(p.join("code-ranker.toml"))
        .arg(format!("--output.json.path={}", out.display()))
        .status()
        .expect("spawn code-ranker");
    assert!(status.success(), "report should succeed");

    let v: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let stats = &v["graphs"]["files"]["stats"];
    assert!(
        stats.get("cyc_mean").is_some(),
        "graph-scope aggregate must appear in stats: {stats}"
    );
}

/// The opt-in `functions` level (`[levels] functions = true`) emits per-function
/// metric nodes (kind + parent + metrics), and is ABSENT by default — so the
/// `files` level and its goldens are unaffected.
#[test]
fn functions_level_is_opt_in() {
    let dir = tempfile::tempdir().expect("temp dir");
    let p = dir.path();
    std::fs::write(
        p.join("a.py"),
        "def f(x):\n    if x:\n        return 1\n    return 0\n\nclass C:\n    def m(self, y):\n        return y\n",
    )
    .unwrap();

    let run = |cfg: &str| -> Value {
        std::fs::write(p.join("code-ranker.toml"), cfg).unwrap();
        let out = p.join("out.json");
        let status = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
            .current_dir(p)
            .env("CARGO_NET_OFFLINE", "true")
            .arg("report")
            .arg(".")
            .arg("--plugin")
            .arg("python")
            .arg("--config")
            .arg(p.join("code-ranker.toml"))
            .arg(format!("--output.json.path={}", out.display()))
            .status()
            .expect("spawn code-ranker");
        assert!(status.success());
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap()
    };

    // Off by default → only the files level.
    let off = run("");
    assert!(
        off["graphs"]["functions"].is_null(),
        "functions level must be opt-in"
    );

    // On → a functions level with per-function nodes.
    let on = run("[levels]\nfunctions = true\n");
    let fns = &on["graphs"]["functions"];
    assert!(fns.is_object(), "functions level present when enabled");
    let nodes = fns["nodes"].as_array().expect("function nodes");
    let f = nodes.iter().find(|n| n["name"] == "f").expect("function f");
    assert_eq!(f["kind"], "function");
    assert_eq!(f["cyclomatic"], serde_json::json!(2)); // 1 base + 1 `if`
    assert!(f["parent"].as_str().unwrap().ends_with("a.py"));
    let m = nodes.iter().find(|n| n["name"] == "m").expect("method m");
    assert_eq!(m["kind"], "method");
}

/// A declared metric whose formula references a misspelled input produces no
/// value anywhere — the run still succeeds (graceful per-node omit) but prints a
/// project-wide-empty warning to stderr so the typo isn't silent.
#[test]
fn empty_metric_warns_on_stderr() {
    let dir = tempfile::tempdir().expect("temp dir");
    let p = dir.path();
    std::fs::write(p.join("m.py"), "def f(x):\n    return x\n").unwrap();
    std::fs::write(
        p.join("code-ranker.toml"),
        "[metrics.bad]\nformula_cel = \"slocc / 100.0\"\n", // `slocc` is a typo for `sloc`
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_code-ranker"))
        .current_dir(p)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(".")
        .arg("--plugin")
        .arg("python")
        .arg("--config")
        .arg(p.join("code-ranker.toml"))
        .arg(format!("--output.json.path={}", p.join("o.json").display()))
        .output()
        .expect("spawn code-ranker");
    assert!(
        out.status.success(),
        "run still succeeds despite the bad metric"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("metric `bad`") && stderr.contains("no value on any node"),
        "project-wide-empty warning expected on stderr, got: {stderr}"
    );
}
