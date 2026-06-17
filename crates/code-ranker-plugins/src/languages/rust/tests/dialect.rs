use super::*;

/// `compute_functions` finds top-level fns, impl methods, and counts a nested
/// closure on its owning fn (covers collect_functions / unit_for / fn_kind).
#[test]
fn compute_functions_covers_fn_method_closure() {
    let src = b"fn f(x: i32) -> i32 { if x > 0 { return 1; } 0 }\n\
                struct S;\n\
                impl S { fn m(&self, y: i32) -> i32 { y } }\n\
                fn g() { let c = |z: i32| z + 1; let _ = c(1); }\n";
    let units = compute_functions(src);
    let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
    assert!(names.contains(&"f"), "fn f: {names:?}");
    assert!(names.contains(&"m"), "method m: {names:?}");
    assert!(names.contains(&"g"), "fn g: {names:?}");

    let f = units.iter().find(|u| u.name == "f").unwrap();
    assert_eq!(f.kind, "fn");
    assert!(f.inputs.branches >= 1.0, "f has an `if` branch");
    assert!(
        f.inputs.exits >= 1.0,
        "f has a `return` / value-returning exit"
    );

    let m = units.iter().find(|u| u.name == "m").unwrap();
    assert_eq!(m.kind, "method");

    let g = units.iter().find(|u| u.name == "g").unwrap();
    assert!(g.inputs.closures >= 1.0, "g defines a closure");
}

#[test]
fn compute_functions_empty_on_no_functions() {
    assert!(compute_functions(b"const X: i32 = 1;\n").is_empty());
}

/// A labeled `break` adds a cognitive structural point; a nested `fn` is its own
/// unit, still classified `fn` (not a method) even though it sits inside another
/// function. Covers the labeled break/continue and function-nesting cog branches.
#[test]
fn compute_functions_covers_labeled_break_and_nested_fn() {
    let src = b"fn outer() {\n\
                    'a: loop { break 'a; }\n\
                    fn inner() -> i32 { 1 }\n\
                }\n";
    let units = compute_functions(src);
    let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
    assert!(names.contains(&"outer"), "outer: {names:?}");
    assert!(names.contains(&"inner"), "nested inner: {names:?}");

    let inner = units.iter().find(|u| u.name == "inner").unwrap();
    assert_eq!(inner.kind, "fn", "a nested fn is still `fn`, not a method");

    let outer = units.iter().find(|u| u.name == "outer").unwrap();
    assert!(
        outer.inputs.cognitive >= 1.0,
        "the labeled `break 'a` adds a cognitive point"
    );
}
