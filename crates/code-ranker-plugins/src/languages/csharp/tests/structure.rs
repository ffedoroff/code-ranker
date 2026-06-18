//! Tests for `csharp/structure.rs` (wired via `#[path]` from that source).

use super::*;
use std::fs;

#[test]
fn is_test_path_matches_conventions() {
    assert!(is_test_path("CalcTests.cs"));
    assert!(is_test_path("test/Helper.cs"));
    assert!(!is_test_path("src/Calc.cs"));
}

#[test]
fn using_graph_resolves_internal_and_external() {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("Main.cs"),
        "using System;\nusing Sample.Mathx;\nnamespace Sample { class P { void M(){} } }\n",
    )
    .unwrap();
    fs::write(
        d.path().join("Mathx.cs"),
        "namespace Sample.Mathx { public static class M { public static int Sq(int n){ return n*n; } } }\n",
    )
    .unwrap();

    let g = analyze(d.path(), false).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("Main.cs") && e.target.ends_with("Mathx.cs")),
        "using Sample.Mathx → uses edge to Mathx.cs"
    );
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "System"),
        "using System → external node"
    );
}

#[test]
fn ignore_tests_drops_test_files() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("A.cs"), "namespace N { class A {} }\n").unwrap();
    fs::write(d.path().join("ATests.cs"), "namespace N { class T {} }\n").unwrap();
    let g = analyze(d.path(), true).unwrap();
    assert!(g.nodes.iter().all(|n| !n.id.ends_with("ATests.cs")));
}
