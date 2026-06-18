//! Tests for `cfamily/mod.rs` (wired via `#[path]` from that source).

use super::*;

fn cfg() -> Cfg {
    Cfg {
        extensions: vec!["c".into(), "h".into()],
        skip_dirs: vec!["build".into()],
        test_dirs: vec!["test".into()],
        test_suffixes: vec!["_test.c".into()],
        ext_prefix: "ext:".into(),
        uses_kind: "uses".into(),
        loc_attr: "loc".into(),
        external_attr: "external".into(),
    }
}

#[test]
fn is_test_path_matches_conventions() {
    let c = cfg();
    assert!(is_test_path("foo_test.c", &c));
    assert!(is_test_path("test/util.c", &c));
    assert!(!is_test_path("src/main.c", &c));
}

#[test]
fn scan_includes_splits_local_and_system() {
    // Mixes a system include, a spaced local include, a non-include `#` directive
    // (ignored), plain code, and another system include.
    let src =
        "#include <stdio.h>\n  #  include \"util.h\"\n#define FOO 1\nint x;\n#include <math.h>\n";
    let incs = scan_includes(src);
    assert_eq!(incs.len(), 3, "the #define line is not an include");
    assert_eq!(incs[0], ("stdio.h".to_string(), true, 1));
    assert_eq!(incs[1], ("util.h".to_string(), false, 2));
    assert!(incs[2].1, "math.h is a system include");
}

#[test]
fn include_graph_resolves_local_and_external() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("main.c"),
        "#include <stdio.h>\n#include \"util.h\"\nint main() { return util(); }\n",
    )
    .unwrap();
    std::fs::write(d.path().join("util.h"), "int util(void);\n").unwrap();

    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("main.c") && e.target.ends_with("util.h")),
        "local include → uses edge"
    );
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "stdio.h"),
        "system include → external node"
    );
}

#[test]
fn unresolved_local_include_becomes_external() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.c"), "#include \"missing.h\"\nint a;\n").unwrap();
    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.nodes
            .iter()
            .any(|n| n.kind == code_ranker_plugin_api::node::EXTERNAL && n.name == "missing.h"),
        "an unresolved local include is surfaced as external"
    );
}

#[test]
fn resolves_local_include_by_unique_basename() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("inc")).unwrap();
    std::fs::write(d.path().join("inc/util.h"), "int u;\n").unwrap();
    // a.c's "util.h" isn't in a.c's dir → resolved by unique basename to inc/util.h.
    std::fs::write(d.path().join("a.c"), "#include \"util.h\"\nint a;\n").unwrap();
    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("a.c") && e.target.ends_with("inc/util.h")),
        "basename fallback resolution"
    );
}

#[test]
fn ignore_tests_drops_test_files() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.c"), "int a;\n").unwrap();
    std::fs::write(d.path().join("a_test.c"), "int t;\n").unwrap();
    let g = analyze(d.path(), true, &cfg()).unwrap();
    assert!(g.nodes.iter().all(|n| !n.id.ends_with("a_test.c")));
    assert!(g.nodes.iter().any(|n| n.id.ends_with("a.c")));
}

#[test]
fn non_utf8_source_file_is_skipped() {
    // A `.c` file with invalid UTF-8 bytes fails `read_to_string` and is skipped
    // (the `else continue` in `analyze`), without aborting the whole walk.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("ok.c"), "int a;\n").unwrap();
    std::fs::write(d.path().join("bad.c"), [0xFFu8, 0xFE, 0x00]).unwrap();
    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.nodes.iter().any(|n| n.id.ends_with("ok.c")),
        "good file kept"
    );
    assert!(
        g.nodes.iter().all(|n| !n.id.ends_with("bad.c")),
        "non-UTF8 file skipped"
    );
}

#[test]
fn resolves_include_by_full_relative_path_from_subdir() {
    // src/main.c includes "util.h"; the file lives at the project root. The
    // parent-relative join (src/util.h) misses, so resolution falls back to the
    // repo-relative `by_rel` lookup on the raw include path.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("src")).unwrap();
    std::fs::write(d.path().join("util.h"), "int u;\n").unwrap();
    std::fs::write(
        d.path().join("src/main.c"),
        "#include \"util.h\"\nint main(){return 0;}\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("src/main.c") && e.target.ends_with("util.h")),
        "resolved by repo-relative path"
    );
}

#[test]
fn resolves_adjacent_file_outside_the_collected_set() {
    // main.c includes "vendor.hpp"; `.hpp` is not in this cfg's extensions, so the
    // file is never collected (absent from `by_rel`) but exists on disk next to
    // main.c — the `cand.is_file()` fallback resolves it.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("vendor.hpp"), "// not collected\n").unwrap();
    std::fs::write(
        d.path().join("main.c"),
        "#include \"vendor.hpp\"\nint main(){return 0;}\n",
    )
    .unwrap();
    let g = analyze(d.path(), false, &cfg()).unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.source.ends_with("main.c") && e.target.ends_with("vendor.hpp")),
        "on-disk neighbour resolved even though not collected"
    );
}

#[test]
fn scan_includes_ignores_macro_and_unterminated_forms() {
    // `#include FOO` (a macro, neither `"` nor `<`) hits the `_ => continue` arm;
    // `#include "x` (no closing quote) finds no terminator and adds nothing.
    let incs = scan_includes("#include FOO\n#include \"x\n#include <ok.h>\n");
    assert_eq!(incs.len(), 1, "only the well-formed system include counts");
    assert_eq!(incs[0].0, "ok.h");
    assert!(incs[0].1, "system include");
}
