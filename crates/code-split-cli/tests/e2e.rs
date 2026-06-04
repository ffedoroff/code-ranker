//! End-to-end fixture tests.
//!
//! For every language's fixture project (colocated with its plugin crate at
//! `crates/code-split-plugin-<lang>/sample/`), run the built `code-split` binary
//! and compare its JSON report against the committed golden
//! `crates/code-split-plugin-<lang>/sample/code-split-report.json`.
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
    // CARGO_MANIFEST_DIR = <repo>/crates/code-split-cli
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root is two levels above the crate manifest")
        .to_path_buf()
}

/// The fixture project for a language, now colocated with its plugin crate at
/// `crates/code-split-plugin-<lang>/sample`.
fn sample_dir(lang: &str) -> PathBuf {
    repo_root()
        .join("crates")
        .join(format!("code-split-plugin-{lang}"))
        .join("sample")
}

/// Run the binary on the language's `sample/` project with its own config and
/// return the parsed JSON report.
fn run_report(lang: &str) -> Value {
    let root = repo_root();
    let sample = sample_dir(lang);
    let out_dir = tempfile::tempdir().expect("create temp output dir");

    let out_json = out_dir.path().join("fresh.json");
    let status = Command::new(env!("CARGO_BIN_EXE_code-split"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true") // Rust sample resolves crates from cache
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-split.toml"))
        .arg(format!("--output.json.path={}", out_json.display()))
        .status()
        .expect("spawn code-split");
    assert!(status.success(), "code-split failed for sample `{lang}`");

    let text =
        std::fs::read_to_string(out_dir.path().join("fresh.json")).expect("read fresh report json");
    serde_json::from_str(&text).expect("parse fresh report json")
}

fn read_golden(lang: &str) -> Value {
    let path = sample_dir(lang).join("code-split-report.json");
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
    let out = Command::new(env!("CARGO_BIN_EXE_code-split"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true")
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-split.toml"))
        .args(extra)
        .output()
        .expect("spawn code-split");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The `scorecard` format streams a per-principle table + worst-module list to
/// stdout. The Rust sample has a mutual cycle (a.rs ↔ b.rs) and no metric
/// breaches, so ADP is the only principle with violations and tops the table.
#[test]
fn rust_sample_scorecard_triage() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--output.scorecard"]);
    assert!(ok, "scorecard run failed: {stderr}");
    assert!(
        stdout.contains("scorecard  (rust, 13 files)"),
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
        stdout.contains("--preset ADP --output.prompt.path"),
        "next-step hint points at the worst principle: {stdout}"
    );
}

/// With no `--preset`, the prompt auto-picks the worst-violating principle (ADP
/// here) and lists the cycle members + their connections — the same Markdown the
/// HTML viewer's Prompt Generator emits.
#[test]
fn rust_sample_prompt_auto_picks_worst_principle() {
    let (ok, stdout, stderr) = run_report_capture("rust", &["--output.prompt.path=stdout"]);
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
        stdout.contains("- `src/a.rs`") && stdout.contains("- `src/b.rs`"),
        "both cycle members listed with cleaned paths: {stdout}"
    );
    assert!(
        stdout.contains("## Connections — common"),
        "ADP pre-selects the `common` connection set"
    );
    assert!(
        stdout.contains(".code-split/<YYYYMMDD-HHMMSS>-ADP.md"),
        "save-report instruction carries the preset id: {stdout}"
    );
}

/// An explicit metric principle (`SRP`, ranked by SLOC) with `--top 1` yields the
/// single worst module in an "ordered by" section.
#[test]
fn rust_sample_prompt_explicit_preset_top1() {
    let (ok, stdout, stderr) = run_report_capture(
        "rust",
        &[
            "--preset",
            "SRP",
            "--top",
            "1",
            "--output.prompt.path=stdout",
        ],
    );
    assert!(ok, "prompt run failed: {stderr}");
    assert!(
        stdout.starts_with("# SRP — Single Responsibility Principle"),
        "explicit preset honoured: {stdout}"
    );
    assert!(
        stdout.contains("## Modules ordered by"),
        "metric ordering section: {stdout}"
    );
    // lib.rs is the largest file in the sample (physical SLOC 25).
    assert!(
        stdout.contains("- `src/lib.rs` (SLOC: 25)"),
        "the single worst SLOC module: {stdout}"
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
    let (ok, _stdout, stderr) = run_report_capture("rust", &["--preset", "ADP"]);
    assert!(!ok, "--preset without a prompt/scorecard format must fail");
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
