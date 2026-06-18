//! Tests for `c/dialect.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn computes_c_file_metrics() {
    let src = b"#include <stdio.h>\n\
\n\
// add returns the larger via a branch.\n\
int classify(int a, int b) {\n\
\tif (a > b && a > 0) {\n\
\t\treturn 1;\n\
\t} else {\n\
\t\treturn 0;\n\
\t}\n\
}\n";
    let m = compute(src).expect("c metrics");
    assert_eq!(m.spaces, 2.0, "file + one function");
    assert!(m.branches >= 2.0, "if + && branches: {}", m.branches);
    assert_eq!(m.exits, 2.0, "two returns");
    assert_eq!(m.args, 2.0, "two params (a, b)");
    assert!(m.cognitive > 0.0 && m.eta1 > 0.0);
}

#[test]
fn switch_cases_and_ternary_count() {
    let src = b"int f(int x) {\n\
\tint y = x > 0 ? 1 : 2;\n\
\tswitch (x) {\n\
\tcase 1: return y;\n\
\tcase 2: return y + 1;\n\
\tdefault: return 0;\n\
\t}\n\
}\n";
    let m = compute(src).expect("metrics");
    // ternary + two case arms are decision points.
    assert!(m.branches >= 3.0, "branches: {}", m.branches);
    let units = compute_functions(src);
    assert!(units.iter().any(|u| u.name == "f" && u.kind == "function"));
}

#[test]
fn pointer_returning_function_name_and_args() {
    // `char *dup(...)` wraps the function_declarator in a pointer_declarator, so
    // both the name and the params are found via the declarator recursion.
    let src = b"char *dup(const char *s, int n) {\n\treturn 0;\n}\n";
    let units = compute_functions(src);
    let u = units
        .iter()
        .find(|u| u.name == "dup")
        .expect("name via pointer declarator");
    assert_eq!(u.inputs.args, 2.0, "two params");
}

#[test]
fn knr_style_function_params_are_not_counted_as_args() {
    // Old-style (K&R) C parses `add(a, b)` as a `parameter_list` of bare
    // `identifier` nodes (the types arrive as separate `declaration`s), not
    // `parameter_declaration`s — so the `args` filter counts 0.
    let units = compute_functions(b"int add(a, b) int a; int b; { return a + b; }\n");
    let u = units.iter().find(|u| u.name == "add").expect("add unit");
    assert_eq!(
        u.inputs.args, 0.0,
        "K&R identifiers are not parameter_declarations"
    );
}
