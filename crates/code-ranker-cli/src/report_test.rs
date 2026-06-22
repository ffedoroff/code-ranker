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
    let out = render_name(
        "{ts}-{git-hash-3}.json",
        t,
        Some("69aa698abcde"),
        fixed_ts(),
    );
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

/// A malformed `{git-hash-…}` token (no closing brace, or a non-numeric width)
/// is left untouched rather than panicking — the `break` arms of the loop.
#[test]
fn render_name_leaves_malformed_git_hash_tokens_intact() {
    let t = Path::new("/x/proj");
    let at = fixed_ts();
    // No closing brace → break, token kept verbatim.
    let unclosed = render_name("a-{git-hash-3", t, Some("abcdef"), at);
    assert_eq!(unclosed, "a-{git-hash-3");
    // Non-numeric width → break, token kept verbatim.
    let nonnum = render_name("a-{git-hash-x}.json", t, Some("abcdef"), at);
    assert_eq!(nonnum, "a-{git-hash-x}.json");
}

#[test]
fn want_format_precedence() {
    use config::OutputArtifact;
    let off = OutputArtifact {
        enabled: None,
        path: None,
    };
    // A CLI flag forces it on regardless of config.
    assert!(want_format(true, None, &off));
    // A CLI path forces it on.
    assert!(want_format(false, Some("x.json"), &off));
    // No CLI selectors, no config → off (path.is_some() == false).
    assert!(!want_format(false, None, &off));
    // No CLI selectors, but a configured path implies on.
    let with_path = OutputArtifact {
        enabled: None,
        path: Some("x.json".into()),
    };
    assert!(want_format(false, None, &with_path));
    // An explicit `enabled = false` wins over a configured path.
    let disabled = OutputArtifact {
        enabled: Some(false),
        path: Some("x.json".into()),
    };
    assert!(!want_format(false, None, &disabled));
}

#[test]
fn is_stream_recognizes_stdout_markers() {
    assert!(is_stream("stdout"));
    assert!(is_stream("-"));
    assert!(!is_stream("out.json"));
}

#[test]
fn write_artifact_to_stream_is_a_noop_on_disk() {
    // `stdout`/`-` print rather than write a file — nothing lands on disk.
    write_artifact("stdout", "hello", "json").unwrap();
    assert!(!Path::new("stdout").exists());
}

#[test]
fn write_artifact_creates_parent_dirs_and_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("nested/sub/report.json");
    write_artifact(dest.to_str().unwrap(), "payload", "json").unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "payload");
}
