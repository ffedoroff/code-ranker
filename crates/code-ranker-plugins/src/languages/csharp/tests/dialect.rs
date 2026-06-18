//! Tests for `csharp/dialect.rs` (wired via `#[path]` from that source).

use super::*;

#[test]
fn computes_csharp_metrics_with_method_and_lambda() {
    let src = br#"using System;

namespace Sample {
	// Calc does arithmetic.
	class Calc {
		public int Classify(int a, int b) {
			if (a > b && a > 0) {
				return 1;
			} else {
				return 0;
			}
		}

		public Func<int, int> Doubler() {
			return x => x * 2;
		}
	}
}
"#;
    let m = compute(src).expect("c# metrics");
    assert!(m.branches >= 2.0, "if + && branches: {}", m.branches);
    assert!(m.closures >= 1.0, "one lambda: {}", m.closures);
    assert_eq!(m.exits, 3.0, "three returns");
    assert!(m.args >= 2.0 && m.cognitive > 0.0 && m.eta1 > 0.0);

    let units = compute_functions(src);
    assert!(
        units
            .iter()
            .any(|u| u.name == "Classify" && u.kind == "method"),
        "method Classify: {:?}",
        units.iter().map(|u| (&u.name, &u.kind)).collect::<Vec<_>>()
    );
}

#[test]
fn local_function_is_a_function_unit() {
    // A C# local function is a `function` unit (not a `method`).
    let src = b"class C {\n\
\tvoid M() {\n\
\t\tint Helper(int x) { return x + 1; }\n\
\t\tHelper(1);\n\
\t}\n\
}\n";
    let units = compute_functions(src);
    assert!(
        units
            .iter()
            .any(|u| u.name == "Helper" && u.kind == "function"),
        "local function Helper is a `function`: {:?}",
        units.iter().map(|u| (&u.name, &u.kind)).collect::<Vec<_>>()
    );
}
