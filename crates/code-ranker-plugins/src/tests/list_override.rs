//! Tests for `list_override.rs` (wired via `#[path]` from that source).

use super::*;
use crate::config::load;

/// `report_override` parses the `[report]` section's per-list patches; each is a
/// plain array (replace) or an op-table (mutate), applied over a base list.
#[test]
fn report_override_parses_and_applies_patches() {
    let cfg: Table = "[report]\n\
         columns = { remove = [\"volume\", \"effort\"], add = [\"unsafe\"] }\n\
         stats = { add = [\"unsafe\"] }\n\
         card = [\"hk\"]\n"
        .parse()
        .unwrap();
    let ro = report_override(&cfg);

    let base: Vec<String> = ["kind", "volume", "effort", "sloc"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        ro.columns.apply(&base),
        ["kind", "sloc", "unsafe"],
        "columns: two dropped, `unsafe` appended"
    );
    assert_eq!(ro.stats.apply(&[]), ["unsafe"], "stats: `unsafe` added");
    assert_eq!(
        ro.card.apply(&base),
        ["hk"],
        "card: a plain array replaces wholesale"
    );
}

/// The `replace` op swaps an element in place (position preserved).
#[test]
fn report_override_replace_in_place() {
    let cfg: Table = "[report]\ncard = { replace = { \"sloc\" = \"unsafe\" } }\n"
        .parse()
        .unwrap();
    let base: Vec<String> = ["hk", "sloc", "mi"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        report_override(&cfg).card.apply(&base),
        ["hk", "unsafe", "mi"]
    );
}

/// No `[report]` section → every patch is a no-op (the catalog list is kept).
#[test]
fn report_override_absent_is_noop() {
    let ro = report_override(&Table::new());
    assert!(ro.columns.is_noop() && ro.card.is_noop() && ro.stats.is_noop());
}

/// `report_override_section` reads a bare `[report]` table (the project
/// `code-ranker.toml` form); the `after` op inserts after an anchor column.
#[test]
fn report_override_section_after_anchor() {
    let report: Table = "columns = { after = { hk = [\"tsr\", \"tsr_big\"] } }\n"
        .parse()
        .unwrap();
    let ro = report_override_section(&report);
    let base: Vec<String> = ["kind", "sloc", "hk", "blank"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        ro.columns.apply(&base),
        ["kind", "sloc", "hk", "tsr", "tsr_big", "blank"],
        "columns inserted right after `hk`"
    );
}

/// The real Rust config carries the demo override: five Halstead-derivative
/// columns dropped, `unsafe` added to both the columns and the JSON `stats`.
#[test]
fn rust_config_report_override_is_the_demo() {
    let rust = report_override(&load(include_str!("../languages/rust/config.toml")));
    for dropped in ["volume", "effort", "time", "length", "vocabulary"] {
        assert!(
            rust.columns.remove.contains(&dropped.to_string()),
            "rust drops the `{dropped}` column"
        );
    }
    assert!(rust.columns.add.contains(&"unsafe".to_string()));
    assert!(rust.stats.add.contains(&"unsafe".to_string()));
}

/// `patch_value_list` only patches a list of strings; a non-string base (e.g. a
/// list of integers) can't be patched by value, so it is returned unchanged.
#[test]
fn patch_value_list_passes_through_non_string_base() {
    let base = vec![Value::Integer(1), Value::Integer(2)];
    let op: Value = "add = [\"x\"]".parse::<Table>().unwrap().into();
    assert_eq!(patch_value_list(base.clone(), &op), base);
}

/// A scalar TOML value (neither an array nor an op-table) yields a no-op patch.
#[test]
fn list_patch_on_scalar_is_noop() {
    assert!(list_patch(&Value::Integer(7)).is_noop());
}
