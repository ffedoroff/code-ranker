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
fn analyze_resolves_aliased_from_import_and_skips_root_init() {
    use crate::test_support::write_file;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    // A root-level `__init__.py` maps to no module path → skipped by `analyze`.
    write_file(root, "__init__.py", "");
    write_file(root, "pkg/__init__.py", "");
    write_file(root, "pkg/b.py", "x = 1\n");
    // `b as c` is an `aliased_import`; resolution keys on its `name` field (`b`).
    write_file(root, "pkg/a.py", "from pkg import b as c\nprint(c)\n");

    let g = analyze(root, false).unwrap();
    let a = root.join("pkg/a.py").to_string_lossy().into_owned();
    let b = root.join("pkg/b.py").to_string_lossy().into_owned();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source == a && e.target == b && e.kind == "uses"),
        "aliased `from pkg import b as c` resolves to pkg/b.py: {:?}",
        g.edges
    );
}

#[test]
fn file_to_module_path_maps_and_rejects() {
    let ws = std::path::Path::new("/proj");
    // `pkg/__init__.py` → the package; `pkg/mod.py` → the dotted module.
    assert_eq!(
        file_to_module_path(ws, std::path::Path::new("/proj/pkg/mod.py")).as_deref(),
        Some("pkg.mod")
    );
    assert_eq!(
        file_to_module_path(ws, std::path::Path::new("/proj/pkg/__init__.py")).as_deref(),
        Some("pkg")
    );
    // A root-level `__init__.py` collapses to an empty path → None.
    assert_eq!(
        file_to_module_path(ws, std::path::Path::new("/proj/__init__.py")),
        None
    );
    // A non-`.py` file is not a module → None.
    assert_eq!(
        file_to_module_path(ws, std::path::Path::new("/proj/pkg/data.txt")),
        None
    );
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
