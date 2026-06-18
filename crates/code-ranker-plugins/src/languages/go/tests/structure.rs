//! Tests for `go/structure.rs` (wired via `#[path]` from that source).

use super::*;
use std::fs;

#[test]
fn go_is_test_path_matches_conventions() {
    for p in ["foo_test.go", "pkg/bar_test.go", "testdata/x.go"] {
        assert!(go_is_test_path(p), "should be a test: {p}");
    }
    for p in ["main.go", "pkg/util.go"] {
        assert!(!go_is_test_path(p), "should not be a test: {p}");
    }
}

#[test]
fn builds_import_graph_internal_and_external() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("go.mod"), "module example.com/m\n\ngo 1.22\n").unwrap();
    fs::write(
        d.path().join("main.go"),
        "package main\n\nimport (\n\t\"fmt\"\n\t\"example.com/m/util\"\n)\n\nfunc main() { fmt.Println(util.X()) }\n",
    )
    .unwrap();
    fs::create_dir(d.path().join("util")).unwrap();
    fs::write(
        d.path().join("util/util.go"),
        "package util\n\nfunc X() int { return 1 }\n",
    )
    .unwrap();

    let g = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();

    let files = g
        .nodes
        .iter()
        .filter(|n| n.kind == code_ranker_plugin_api::node::FILE)
        .count();
    assert_eq!(files, 2, "main.go + util/util.go");

    // Internal import → file→file `uses` edge.
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("main.go") && e.target.ends_with("util.go")),
        "main.go uses util.go"
    );
    // Standard-library import → one external node named `fmt`.
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "fmt"),
        "external fmt node"
    );
}

#[test]
fn go_mod_without_module_line_yields_external() {
    let d = tempfile::tempdir().unwrap();
    // go.mod present but with no `module` line → empty module path → external.
    fs::write(d.path().join("go.mod"), "go 1.22\n").unwrap();
    fs::write(
        d.path().join("x.go"),
        "package m\nimport \"strings\"\nfunc F() string { return strings.ToUpper(\"a\") }\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "strings")
    );
}

#[test]
fn no_go_mod_treats_all_imports_as_external() {
    let d = tempfile::tempdir().unwrap();
    // No go.mod (root-level file) → empty module path → every import is external.
    fs::write(
        d.path().join("x.go"),
        "package m\nimport \"fmt\"\nfunc F() { fmt.Println() }\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::FILE && n.name == "x.go"),
        "the root file node"
    );
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "fmt"),
        "fmt is external without a module path"
    );
}

#[test]
fn ignore_tests_drops_test_files() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("go.mod"), "module m\n").unwrap();
    fs::write(d.path().join("a.go"), "package m\nfunc A() {}\n").unwrap();
    fs::write(d.path().join("a_test.go"), "package m\nfunc TestA() {}\n").unwrap();

    let kept = analyze(d.path(), true, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(
        kept.nodes.iter().all(|n| !n.id.ends_with("a_test.go")),
        "a_test.go dropped with ignore_tests"
    );
    let all = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(all.nodes.iter().any(|n| n.id.ends_with("a_test.go")));
}

#[test]
fn internal_import_without_matching_package_yields_no_edge() {
    // An import under the module path whose package isn't present in the project
    // is `internal` but absent from the package index, so it produces no edge and
    // no external node (the `if let Some(targets)` miss).
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("go.mod"), "module example.com/m\n\ngo 1.22\n").unwrap();
    fs::write(
        d.path().join("main.go"),
        "package main\n\nimport \"example.com/m/missing\"\n\nfunc main() { _ = missing.X }\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(g.edges.is_empty(), "unknown internal package -> no edge");
    assert!(
        g.nodes
            .iter()
            .all(|n| n.kind != code_ranker_plugin_api::node::EXTERNAL),
        "an internal (module-path) import is never external"
    );
}

#[test]
fn empty_import_path_is_ignored() {
    // A degenerate `import ""` parses to an empty string literal; the
    // `if !path.is_empty()` guard drops it, so it yields no import.
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("go.mod"), "module m\n").unwrap();
    fs::write(
        d.path().join("x.go"),
        "package m\nimport \"\"\nfunc F() {}\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &crate::test_support::IGNORE_ALL).unwrap();
    assert!(g.edges.is_empty(), "empty import path -> no edge");
    assert!(
        g.nodes
            .iter()
            .all(|n| n.kind != code_ranker_plugin_api::node::EXTERNAL),
        "empty import path -> no external node"
    );
}
