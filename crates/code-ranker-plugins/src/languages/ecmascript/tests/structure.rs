use super::*;
use crate::test_support::{edge_count_from, write_file};
use std::fs;
use tempfile::TempDir;

#[test]
fn file_to_mod_path_strips_ext_and_collapses_index() {
    let ws = Path::new("/proj");
    assert_eq!(
        file_to_mod_path(ws, Path::new("/proj/src/lib/utils.ts")).as_deref(),
        Some("src/lib/utils")
    );
    assert_eq!(
        file_to_mod_path(ws, Path::new("/proj/src/lib/index.ts")).as_deref(),
        Some("src/lib")
    );
    // A bare `index.<ext>` at the workspace root collapses to an empty path → None.
    assert_eq!(file_to_mod_path(ws, Path::new("/proj/index.ts")), None);
}

#[test]
fn normalize_path_resolves_dot_and_dotdot() {
    // `..` pops the previous component, `.` is skipped — no filesystem touched.
    assert_eq!(
        normalize_path(Path::new("a/b/../c/./d")),
        std::path::PathBuf::from("a/c/d")
    );
}

#[test]
fn external_package_extracts_top_level_and_scope() {
    assert_eq!(external_package("react").as_deref(), Some("react"));
    assert_eq!(external_package("lodash/fp").as_deref(), Some("lodash"));
    assert_eq!(
        external_package("@scope/pkg/sub").as_deref(),
        Some("@scope/pkg")
    );
    // A bare scope with no sub-package keeps the scope verbatim.
    assert_eq!(external_package("@scope").as_deref(), Some("@scope"));
    assert_eq!(external_package("./local"), None);
    assert_eq!(external_package("../up"), None);
    assert_eq!(external_package("@/aliased"), None);
    assert_eq!(external_package(""), None);
}

#[test]
fn resolve_import_external_package_is_skipped() {
    let got = resolve_import(
        "react",
        Path::new("/proj/src/a.ts"),
        Path::new("/proj"),
        Path::new("/proj/src"),
        &HashMap::new(),
        &["ts", "tsx", "js", "jsx"],
    );
    assert_eq!(got, None, "bare package specifiers are not local imports");
}

#[test]
fn find_source_root_prefers_existing_src_dir() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(find_source_root(tmp.path()), tmp.path());
    fs::create_dir(tmp.path().join("src")).unwrap();
    assert_eq!(find_source_root(tmp.path()), tmp.path().join("src"));
}

#[test]
fn analyze_builds_file_graph_with_imports_and_externals() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_file(
        root,
        "src/a.ts",
        "import { greet } from \"./b\";\n\
         import React from \"react\";\n\
         export function helper() { return greet(); }\n",
    );
    write_file(
        root,
        "src/b.ts",
        "export function greet() { return \"hi\"; }\n",
    );

    // Use TS extensions so the tree-sitter-javascript parser (used here
    // via the shared helper) can still parse the TS syntax subset.
    let graph = analyze_ecmascript(
        root,
        &["ts"],
        |ext| match ext {
            "ts" => Some(tree_sitter_javascript::LANGUAGE.into()),
            _ => None,
        },
        &["ts", "tsx", "js", "jsx"],
        false,
        &crate::test_support::IGNORE_ALL,
    )
    .expect("analyze_ecmascript should succeed");

    let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
    let b_id = root.join("src/b.ts").to_string_lossy().into_owned();

    assert!(
        graph.nodes.iter().any(|n| n.id == a_id && n.kind == "file"),
        "a.ts node present"
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.source == a_id && e.target == b_id && e.kind == "uses"),
        "expected import edge a.ts → b.ts"
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.id == "ext:react" && n.kind == "external"),
        "external node for react"
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.source == a_id && e.target == "ext:react"),
        "external edge a.ts → react"
    );
}

#[test]
fn import_path_in_comment_or_string_is_not_an_edge() {
    // Layer-1 metamorphic FP guard (docs/metric-correctness.md): a module path
    // that appears only inside a comment, a string, or a template literal must
    // NOT create a dependency edge — imports are read from AST nodes, not text.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_file(
        root,
        "src/a.ts",
        "// import { greet } from \"./b\";\n\
         const note = \"import { greet } from './b'\";\n\
         const tpl = `import './b'`;\n\
         void note;\n\
         void tpl;\n\
         export function helper() { return 1; }\n",
    );
    write_file(
        root,
        "src/b.ts",
        "export function greet() { return \"hi\"; }\n",
    );

    let graph = analyze_ecmascript(
        root,
        &["ts"],
        |ext| match ext {
            "ts" => Some(tree_sitter_javascript::LANGUAGE.into()),
            _ => None,
        },
        &["ts", "tsx", "js", "jsx"],
        false,
        &crate::test_support::IGNORE_ALL,
    )
    .expect("analyze_ecmascript should succeed");

    let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
    let b_id = root.join("src/b.ts").to_string_lossy().into_owned();
    assert!(
        !graph
            .edges
            .iter()
            .any(|e| e.source == a_id && e.target == b_id),
        "a path in a comment/string/template must not produce an edge"
    );
}

#[test]
fn edges_scale_with_real_imports() {
    // Layer-2 generative (docs/metric-correctness.md): `a` really imports
    // `n` files; the edge count from `a` must equal `n` (ground truth by
    // construction), swept over a grid. Deterministic, no random dependency.
    for n in 0..5 {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let mut a = String::new();
        for i in 0..n {
            a.push_str(&format!("import {{ x{i} }} from \"./b{i}\";\n"));
            write_file(
                root,
                &format!("src/b{i}.ts"),
                &format!("export const x{i} = {i};\n"),
            );
        }
        a.push_str("export const y = 1;\n");
        write_file(root, "src/a.ts", &a);

        let graph = analyze_ecmascript(
            root,
            &["ts"],
            |ext| match ext {
                "ts" => Some(tree_sitter_javascript::LANGUAGE.into()),
                _ => None,
            },
            &["ts", "tsx", "js", "jsx"],
            false,
            &crate::test_support::IGNORE_ALL,
        )
        .expect("analyze_ecmascript should succeed");

        let a_id = root.join("src/a.ts").to_string_lossy().into_owned();
        let got = graph
            .edges
            .iter()
            .filter(|e| e.source == a_id && e.kind == "uses")
            .count();
        assert_eq!(
            got, n,
            "expected exactly {n} import edges from a.ts, got {got}"
        );
    }
}

#[test]
fn ecmascript_is_test_path_matches_conventions() {
    for p in [
        "src/a.test.ts",
        "src/a.spec.tsx",
        "__tests__/a.js",
        "src/__mocks__/fs.js",
        "test/helper.ts",
        "src/foo_test.js",
    ] {
        assert!(ecmascript_is_test_path(p), "should be a test: {p}");
    }
    for p in ["src/a.ts", "src/latest.ts", "src/contest.js"] {
        assert!(!ecmascript_is_test_path(p), "should not be a test: {p}");
    }
}

/// Build a 2-file JS project (`a.js` importing from `./b`) and count the
/// `uses` edges leaving `a`.
fn js_uses_edges(a_src: &str) -> usize {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_file(root, "src/a.js", a_src);
    write_file(root, "src/b.js", "export const g = 1;\n");
    let g = analyze_ecmascript(
        root,
        &["js"],
        |ext| match ext {
            "js" => Some(tree_sitter_javascript::LANGUAGE.into()),
            _ => None,
        },
        &["js", "jsx", "ts"],
        false,
        &crate::test_support::IGNORE_ALL,
    )
    .expect("analyze_ecmascript should succeed");
    let a_id = root.join("src/a.js").to_string_lossy().into_owned();
    edge_count_from(&g, &a_id, "uses")
}

#[test]
fn js_static_import_forms_produce_edges() {
    // Namespace / aliased / default / re-export / require all resolve to one
    // dependency edge — the resolver keys on the module specifier string, so
    // the binding sugar is transparent.
    let forms: &[(&str, &str)] = &[
        (
            "namespace",
            "import * as ns from \"./b\";\nvoid ns;\nexport const x = 1;\n",
        ),
        (
            "aliased named",
            "import { g as h } from \"./b\";\nvoid h;\nexport const x = 1;\n",
        ),
        (
            "default",
            "import b from \"./b\";\nvoid b;\nexport const x = 1;\n",
        ),
        ("re-export", "export { g } from \"./b\";\n"),
        (
            "require",
            "const b = require(\"./b\");\nvoid b;\nexport const x = 1;\n",
        ),
    ];
    let mut fails = Vec::new();
    for (label, src) in forms {
        let n = js_uses_edges(src);
        if n != 1 {
            fails.push(format!("{label}: expected 1 edge, got {n}"));
        }
    }
    assert!(
        fails.is_empty(),
        "import forms not resolved:\n{}",
        fails.join("\n")
    );
}

#[test]
fn js_dynamic_import_is_a_non_goal() {
    // A *dynamic* `import("./b")` is not resolved into an edge (the specifier
    // sits inside a call the walker does not descend into) — documented
    // limitation, pinned so a future change is deliberate.
    assert_eq!(
        js_uses_edges("export async function f() { return import(\"./b\"); }\n"),
        0,
        "dynamic import() is a documented non-goal"
    );
}

// Helper: run the JS-grammar walker over a project rooted at `root`.
fn analyze_js(root: &std::path::Path, ignore_tests: bool) -> Graph {
    analyze_ecmascript(
        root,
        &["js"],
        |ext| match ext {
            "js" => Some(tree_sitter_javascript::LANGUAGE.into()),
            _ => None,
        },
        &["js", "jsx"],
        ignore_tests,
        &crate::test_support::IGNORE_ALL,
    )
    .expect("analyze_ecmascript should succeed")
}

#[test]
fn resolve_index_and_parent_dir_imports() {
    // `./pkg` resolves to `pkg/index.js` (index resolution); `../lib` from a
    // nested dir resolves to `src/lib.js` (parent-dir `..` normalization).
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_file(root, "src/lib.js", "export const v = 1;\n");
    write_file(root, "src/pkg/index.js", "export const p = 2;\n");
    write_file(
        root,
        "src/a.js",
        "import { p } from \"./pkg\";\nvoid p;\nexport const x = 1;\n",
    );
    write_file(
        root,
        "src/sub/c.js",
        "import { v } from \"../lib\";\nvoid v;\nexport const y = 1;\n",
    );

    let g = analyze_js(root, false);
    let a = root.join("src/a.js").to_string_lossy().into_owned();
    let c = root.join("src/sub/c.js").to_string_lossy().into_owned();
    let idx = root.join("src/pkg/index.js").to_string_lossy().into_owned();
    let lib = root.join("src/lib.js").to_string_lossy().into_owned();
    assert!(
        g.edges.iter().any(|e| e.source == a && e.target == idx),
        "`./pkg` resolves to pkg/index.js"
    );
    assert!(
        g.edges.iter().any(|e| e.source == c && e.target == lib),
        "`../lib` resolves to src/lib.js"
    );
}

#[test]
fn skips_node_modules_and_ignored_test_files() {
    // Files under a `node_modules` dir are skipped entirely; with
    // `ignore_tests` a `*.test.js` file is dropped from the walk.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_file(root, "src/a.js", "export const x = 1;\n");
    write_file(root, "src/node_modules/dep.js", "export const d = 1;\n");
    write_file(root, "src/a.test.js", "export const t = 1;\n");

    let g = analyze_js(root, true);
    let has = |rel: &str| {
        let id = root.join(rel).to_string_lossy().into_owned();
        g.nodes.iter().any(|n| n.id == id)
    };
    assert!(has("src/a.js"), "the real source file is kept");
    assert!(!has("src/node_modules/dep.js"), "node_modules is skipped");
    assert!(!has("src/a.test.js"), "ignore_tests drops the test file");
}

#[test]
fn test_convention_lists_load_from_config() {
    // The moved DATA lists resolve from `ecmascript/config.toml` verbatim.
    assert_eq!(TEST.dirs, ["__tests__", "__mocks__", "tests", "test"]);
    assert_eq!(TEST.infixes, [".test.", ".spec."]);
    assert_eq!(TEST.stem_suffixes, ["_test", "_spec"]);
}

#[test]
fn source_root_and_module_lists_load_from_config() {
    assert_eq!(MODULE.source_dirs, ["src"]);
    assert_eq!(
        MODULE.strip_exts,
        [".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".mts", ".cts"]
    );
    assert_eq!(MODULE.index_file, "index");
}

#[test]
fn uses_edge_kind_resolves_against_published_vocab() {
    // The tagged kind is validated against the merged `[edge_kinds]` (inherited
    // `uses` from defaults.toml) — never a bare literal.
    assert_eq!(uses_edge_kind(), "uses");
}
