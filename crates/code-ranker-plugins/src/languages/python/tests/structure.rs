//! Tests for `python/structure.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn py_is_test_path_matches_conventions() {
    for p in [
        "tests/test_x.py",
        "pkg/tests/helper.py",
        "test/helper.py",
        "conftest.py",
        "pkg/conftest.py",
        "test_module.py",
        "pkg/test_module.py",
        "module_test.py",
    ] {
        assert!(py_is_test_path(p), "should be a test: {p}");
    }
    for p in [
        "pkg/module.py",
        "pkg/contest.py",    // not a `tests` dir component
        "pkg/test_data.txt", // `test_` but not `.py`
        "latest.py",
    ] {
        assert!(!py_is_test_path(p), "should not be a test: {p}");
    }
}

#[test]
fn test_convention_lists_load_from_config() {
    // The moved DATA lists resolve from `python/config.toml` verbatim.
    assert_eq!(KINDS.test_dirs, ["tests", "test"]);
    assert_eq!(KINDS.test_files, ["conftest.py"]);
    assert_eq!(KINDS.test_prefixes, ["test_"]);
    assert_eq!(KINDS.test_suffixes, ["_test.py"]);
}

#[test]
fn uses_edge_kind_resolves_against_published_vocab() {
    // The tagged kind is validated against the merged `[edge_kinds]` (inherited
    // `uses` from defaults.toml) — never a bare literal.
    assert_eq!(uses_edge_kind(), "uses");
}

#[test]
fn py_visibility_str_classifies_by_underscore_convention() {
    // The naming-convention LOGIC is a Python syntax rule; the output strings are
    // config DATA (`[visibility]`). Cover all three branches.
    assert_eq!(py_visibility_str("mod"), "public");
    assert_eq!(py_visibility_str("_mod"), "restricted");
    assert_eq!(py_visibility_str("__mod"), "private");
    // A trailing dunder (`__init__`) is NOT name-mangled → restricted, not private.
    assert_eq!(py_visibility_str("__init__"), "restricted");
}

#[test]
fn absolute_base_resolves_relative_imports() {
    // A non-relative base passes through unchanged.
    assert_eq!(absolute_base("pkg.mod", "a.b"), "pkg.mod");
    // `.utils` → a sibling under the current package.
    assert_eq!(absolute_base(".utils", "pkg.mod"), "pkg.utils");
    // Bare `.` (no suffix) → the current package itself (suffix-empty branch).
    assert_eq!(absolute_base(".", "pkg.mod"), "pkg");
    // `..x` walks above the single-segment root → pkg empty, the suffix wins.
    assert_eq!(absolute_base("..x", "a"), "x");
}
