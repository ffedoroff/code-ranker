//! End-to-end fixture tests.
//!
//! For every project under `samples/`, run the built `code-split` binary and
//! compare its JSON report against the committed golden
//! `samples/<lang>/code-split-report.json`.
//!
//! The committed golden keeps its RAW header (timestamp, command, git, versions,
//! absolute paths, timings). The comparison therefore:
//!   1. asserts the volatile fields that MUST differ between two runs actually
//!      differ (proof we compared a fresh run, not a stale copy);
//!   2. rewrites every volatile field to a fixed canonical value on BOTH the
//!      fresh output and the golden;
//!   3. compares the entire normalized structure character-for-character and
//!      requires a 100% match.
//!
//! The graph itself (nodes/edges/cycles/stats) is already machine-independent —
//! the tool relativizes paths to the `{target}` placeholder — so it is compared
//! verbatim, which is where the real assertions about detected dependencies and
//! blind spots live.
//!
//! To refresh the goldens after an intentional change, run `samples/regen.sh`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Top-level header fields that are environment- or time-dependent and so are
/// canonicalized away before the structural comparison.
const VOLATILE: &[&str] = &[
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

/// Run the binary on `samples/<lang>` with the sample's own config and return
/// the parsed JSON report.
fn run_report(lang: &str) -> Value {
    let root = repo_root();
    let sample = root.join("samples").join(lang);
    let out_dir = tempfile::tempdir().expect("create temp output dir");

    let status = Command::new(env!("CARGO_BIN_EXE_code-split"))
        .current_dir(&root)
        .env("CARGO_NET_OFFLINE", "true") // Rust sample resolves crates from cache
        .arg("report")
        .arg(&sample)
        .arg("--config")
        .arg(sample.join("code-split.toml"))
        .arg("--format")
        .arg("json")
        .arg("--report-path")
        .arg(out_dir.path())
        .arg("--json-name")
        .arg("fresh.json")
        .status()
        .expect("spawn code-split");
    assert!(status.success(), "code-split failed for sample `{lang}`");

    let text = std::fs::read_to_string(out_dir.path().join("fresh.json"))
        .expect("read fresh report json");
    serde_json::from_str(&text).expect("parse fresh report json")
}

fn read_golden(lang: &str) -> Value {
    let path = repo_root()
        .join("samples")
        .join(lang)
        .join("code-split-report.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse golden report json")
}

/// Replace every volatile field with a fixed sentinel so two reports captured
/// at different times / on different machines normalize to the same bytes.
fn canonicalize(v: &mut Value) {
    let obj = v.as_object_mut().expect("report root is a JSON object");
    for key in VOLATILE {
        if obj.contains_key(*key) {
            obj.insert((*key).to_string(), Value::String("<normalized>".into()));
        }
    }
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

    // 2. Normalize the volatile header on both sides.
    canonicalize(&mut fresh);
    canonicalize(&mut golden);

    // 3. Character-for-character comparison of the whole normalized structure.
    // serde_json's default map sorts keys, so both sides serialize identically.
    let fresh_s = serde_json::to_string_pretty(&fresh).unwrap();
    let golden_s = serde_json::to_string_pretty(&golden).unwrap();
    assert_eq!(
        fresh_s, golden_s,
        "[{lang}] normalized report differs from golden. \
         If this change is intentional, run `samples/regen.sh`."
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
