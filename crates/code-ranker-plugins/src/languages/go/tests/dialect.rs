//! Tests for `go/dialect.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn computes_go_file_metrics() {
    let src = b"package main\n\
\n\
import \"fmt\"\n\
\n\
func add(a, b int) int {\n\
\tif a > b && a > 0 {\n\
\t\treturn a\n\
\t}\n\
\treturn b\n\
}\n\
\n\
func main() {\n\
\tfmt.Println(add(1, 2))\n\
}\n";
    let m = compute(src).expect("go metrics");
    // 1 file space + 2 funcs.
    assert_eq!(m.spaces, 3.0, "file + two funcs");
    // `if` plus the `&&` operator are decision points.
    assert!(m.branches >= 2.0, "if + && branches: {}", m.branches);
    // two `return`s are exits.
    assert_eq!(m.exits, 2.0, "two returns");
    assert!(m.sloc > 0.0 && m.eta1 > 0.0 && m.n2 > 0.0);
}

#[test]
fn cognitive_counts_else_and_closure() {
    // `if` (+1), `else if` (its nested `if` increments; the else is not double
    // counted), plain `else` (+1), and a closure with its own `if`.
    let src = b"package p\n\
func f(x int) int {\n\
\tif x > 0 {\n\
\t\treturn 1\n\
\t} else if x < 0 {\n\
\t\treturn -1\n\
\t} else {\n\
\t\treturn 0\n\
\t}\n\
}\n\
\n\
var g = func(y int) int {\n\
\tif y > 0 {\n\
\t\treturn y\n\
\t}\n\
\treturn 0\n\
}\n";
    let m = compute(src).expect("metrics");
    assert!(m.cognitive > 0.0, "cognitive: {}", m.cognitive);
    assert!(m.closures >= 1.0, "one closure: {}", m.closures);
}

#[test]
fn function_units_are_collected() {
    let src = b"package p\nfunc Foo() {}\ntype T struct{}\nfunc (t T) Bar() {}\n";
    let units = compute_functions(src);
    let kinds: Vec<&str> = units.iter().map(|u| u.kind.as_str()).collect();
    assert!(kinds.contains(&"function"), "Foo is a function: {kinds:?}");
    assert!(kinds.contains(&"method"), "Bar is a method: {kinds:?}");
}
