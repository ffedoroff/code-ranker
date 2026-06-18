//! Tests for `cpp/dialect.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn computes_cpp_metrics_with_method_and_lambda() {
    let src = br#"#include <vector>

// Box holds a value.
class Box {
public:
	int get(int fallback) const {
		if (value_ > 0 && fallback >= 0) {
			return value_;
		}
		return fallback;
	}
private:
	int value_ = 0;
};

int run(int n) {
	auto dbl = [](int x) { return x * 2; };
	int total = 0;
	for (int i = 0; i < n; i++) {
		total += dbl(i);
	}
	return total;
}
"#;
    let m = compute(src).expect("cpp metrics");
    assert!(m.branches >= 2.0, "if + && + for: {}", m.branches);
    assert!(m.closures >= 1.0, "one lambda: {}", m.closures);
    assert!(m.cognitive > 0.0 && m.args >= 1.0 && m.eta1 > 0.0);

    let units = compute_functions(src);
    assert!(
        units.iter().any(|u| u.name == "get" && u.kind == "method"),
        "method get"
    );
    assert!(
        units
            .iter()
            .any(|u| u.name == "run" && u.kind == "function"),
        "free fn run"
    );
}

#[test]
fn cognitive_counts_else_and_else_if() {
    // `if`, `else if` (its nested if increments; the else isn't double-counted),
    // and a plain `else` (+1) — exercises the else_clause handling.
    let src = b"int sign(int x) {\n\
\tif (x > 0) {\n\
\t\treturn 1;\n\
\t} else if (x < 0) {\n\
\t\treturn -1;\n\
\t} else {\n\
\t\treturn 0;\n\
\t}\n\
}\n";
    let m = compute(src).expect("cpp metrics");
    assert!(m.cognitive > 0.0, "cognitive: {}", m.cognitive);
    assert_eq!(m.exits, 3.0, "three returns");
}

#[test]
fn short_circuit_boolean_adds_cognitive() {
    // See the C twin and `contrib/unit-tests.md`: tree-sitter-cpp also declares two
    // `binary_expression` symbols, so cognitive boolean detection matches the SET.
    let plain = compute(b"int f(int a){ if (a > 0) { return 1; } return 0; }\n").unwrap();
    let boolean =
        compute(b"int f(int a){ if (a > 0 && a < 9) { return 1; } return 0; }\n").unwrap();
    assert!(
        boolean.cognitive > plain.cognitive,
        "&& adds a cognitive boolean run: bool={} plain={}",
        boolean.cognitive,
        plain.cognitive
    );
}
