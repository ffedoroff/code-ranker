use super::*;

/// Count `unsafe` over non-test items, mirroring the loop in `walk_file`
/// (which always skips test items for this count).
fn count_unsafe(src: &str) -> u32 {
    let f = syn::parse_file(src).unwrap();
    let mut counter = UnsafeCounter::default();
    for item in &f.items {
        if is_test_item(item) {
            continue;
        }
        syn::visit::Visit::visit_item(&mut counter, item);
    }
    counter.count
}

#[test]
fn counts_unsafe_blocks_fns_impls_and_traits() {
    let src = r#"
        fn uses_block() {
            unsafe { core::ptr::null::<u8>(); }
        }
        unsafe fn raw() {}
        unsafe trait Marker {}
        unsafe impl Marker for u8 {}
    "#;
    // unsafe block (1) + unsafe fn (1) + unsafe trait (1) + unsafe impl (1).
    assert_eq!(count_unsafe(src), 4);
}

#[test]
fn unsafe_in_production_is_counted_but_tests_are_excluded() {
    let src = r#"
        fn prod() {
            unsafe { core::ptr::null::<u8>(); }
        }

        #[cfg(test)]
        mod tests {
            #[test]
            fn t() {
                unsafe { core::ptr::null::<u8>(); }
                unsafe { core::ptr::null::<u8>(); }
            }
        }
    "#;
    // Only the one production block counts; the two in `#[cfg(test)]` do not.
    assert_eq!(count_unsafe(src), 1);
}

#[test]
fn no_unsafe_is_zero() {
    assert_eq!(count_unsafe("fn safe() { let _ = 1 + 1; }"), 0);
}

#[test]
fn unsafe_ignores_keyword_lookalikes() {
    // Layer-1 metamorphic FP guard (see docs/metric-correctness.md): the word
    // `unsafe` appearing only as an identifier, a comment, a doc-comment, or a
    // string literal — with no real `unsafe` construct — must count 0. This is
    // the AST-Accurate principle: we count syntax nodes, not text.
    let src = r#"
        // unsafe unsafe — just a comment mentioning unsafe
        /// doc comment: this fn is not unsafe
        fn super_unsafe_fn() -> &'static str {
            let unsafe_mode = "unsafe { } unsafe fn impl trait";
            unsafe_mode
        }
        struct UnsafeWrapper;
        enum UnsafeKind { A, B }
    "#;
    assert_eq!(
        count_unsafe(src),
        0,
        "`unsafe` in names/comments/strings must not be counted"
    );
}

#[test]
fn collector_captures_qualified_paths() {
    let f = syn::parse_file(
        "fn run() { let _ = once_cell::sync::Lazy::new(|| 1); commands::go(); plain(); }",
    )
    .unwrap();
    let mut c = CratePathCollector::default();
    syn::visit::Visit::visit_file(&mut c, &f);
    assert!(
        c.paths.contains(&vec![
            "once_cell".into(),
            "sync".into(),
            "Lazy".into(),
            "new".into()
        ]),
        "got {:?}",
        c.paths
    );
    assert!(
        c.paths.contains(&vec!["commands".into(), "go".into()]),
        "got {:?}",
        c.paths
    );
    assert!(
        !c.paths.iter().any(|p| p == &vec!["plain".to_string()]),
        "single-segment call ignored"
    );
}

#[test]
fn collector_ignores_paths_in_strings_and_comments() {
    // Layer-1 metamorphic FP guard (see docs/metric-correctness.md): a
    // qualified path that appears only inside a comment or a string literal is
    // not a path expression, so it yields no dependency edge — we collect AST
    // path nodes, not text.
    let f = syn::parse_file(
        "// commands::go() once_cell::sync::Lazy\n\
         fn run() { let _ = \"once_cell::sync::Lazy and commands::go\"; }",
    )
    .unwrap();
    let mut c = CratePathCollector::default();
    syn::visit::Visit::visit_file(&mut c, &f);
    assert!(
        c.paths.is_empty(),
        "no path should come from comment/string text, got {:?}",
        c.paths
    );
}

#[test]
fn collector_scales_with_real_paths_not_text() {
    // Layer-2 generative (docs/metric-correctness.md): generate a body with
    // `reals` real qualified-path calls and `noise` path-like strings, and
    // assert the collector picks up exactly `reals` — ground truth by
    // construction, swept over a grid. Deterministic, no random dependency.
    for reals in 0..6 {
        for noise in 0..4 {
            let mut body = String::new();
            for i in 0..noise {
                body.push_str(&format!("let _c{i} = \"mod{i}::go() once::Lazy\"; "));
            }
            for i in 0..reals {
                body.push_str(&format!("mod{i}::go(); "));
            }
            let src = format!("fn run() {{ {body} }}");
            let f = syn::parse_file(&src).unwrap();
            let mut c = CratePathCollector::default();
            syn::visit::Visit::visit_file(&mut c, &f);
            let got = c
                .paths
                .iter()
                .filter(|p| p.len() == 2 && p[0].starts_with("mod") && p[1] == "go")
                .count();
            assert_eq!(
                got, reals,
                "expected exactly {reals} real paths (noise={noise}), got {:?}",
                c.paths
            );
        }
    }
}

#[test]
fn collector_captures_qualified_derive_paths() {
    // A crate referenced only through a qualified derive (no `use`) must
    // still produce a path — the derive arguments are otherwise opaque tokens.
    let f =
        syn::parse_file("#[derive(Debug, serde::Serialize, thiserror::Error)] struct S;").unwrap();
    let mut c = CratePathCollector::default();
    syn::visit::Visit::visit_file(&mut c, &f);
    assert!(
        c.paths.contains(&vec!["serde".into(), "Serialize".into()]),
        "got {:?}",
        c.paths
    );
    assert!(
        c.paths.contains(&vec!["thiserror".into(), "Error".into()]),
        "got {:?}",
        c.paths
    );
    // The bare `Debug` derive (single segment, std prelude) is not an edge.
    assert!(
        !c.paths.iter().any(|p| p == &vec!["Debug".to_string()]),
        "single-segment derive ignored"
    );
}

#[test]
fn counts_unsafe_methods_in_impl_and_trait() {
    // `unsafe fn` declared inside an `impl` block and inside a `trait` body —
    // distinct visitor hooks from a free `unsafe fn`.
    let src = r#"
        struct S;
        impl S { unsafe fn m(&self) {} fn safe(&self) {} }
        trait T { unsafe fn req(&self); fn ok(&self) {} }
    "#;
    // one impl method + one trait method = 2 (the safe ones and the
    // non-unsafe impl/trait headers do not count).
    assert_eq!(count_unsafe(src), 2);
}

#[test]
fn convert_visibility_maps_every_form() {
    let conv = |s: &str| convert_visibility(&syn::parse_str::<SynVis>(s).unwrap());
    assert_eq!(conv("pub"), Visibility::Public);
    assert_eq!(conv("pub(crate)"), Visibility::Crate);
    assert_eq!(conv("pub(super)"), Visibility::Super);
    assert_eq!(conv("pub(self)"), Visibility::Private);
    assert_eq!(conv(""), Visibility::Private); // inherited
    assert_eq!(
        conv("pub(in crate::module_graph)"),
        Visibility::Restricted {
            path: "crate::module_graph".into()
        }
    );
}

#[test]
fn is_test_item_covers_every_item_kind_and_attr_form() {
    // `#[cfg(test)]` on each item *kind* (mod/fn/impl/struct/enum/trait/type/
    // const/static/use/macro/union) is recognised — exercising every arm of the
    // `is_test_item` match. `#[test]`/`#[bench]` and `cfg(all(test, ..))` cover
    // the `is_test_attr` forms; a plain item and an `extern` block are NOT tests
    // (the `_ => false` arms).
    let src = "\
#[cfg(test)] mod m {}\n\
#[test] fn tagged() {}\n\
#[bench] fn benched() {}\n\
#[cfg(all(test, feature = \"x\"))] fn nested_cfg() {}\n\
#[cfg(test)] impl Plain {}\n\
#[cfg(test)] struct Sx;\n\
#[cfg(test)] enum E { A }\n\
#[cfg(test)] trait T {}\n\
#[cfg(test)] type Y = u8;\n\
#[cfg(test)] const C: u8 = 0;\n\
#[cfg(test)] static ST: u8 = 0;\n\
#[cfg(test)] use std::fmt;\n\
#[cfg(test)] union U { a: u8 }\n\
#[cfg(test)] mac! {}\n\
struct Plain;\n\
extern \"C\" {}\n\
#[cfg(feature = \"test\")] fn not_a_test() {}\n";
    let f = syn::parse_file(src).unwrap();
    let test_count = f.items.iter().filter(|i| is_test_item(i)).count();
    // The 14 `#[cfg(test)]` / `#[test]` / `#[bench]` / `cfg(all(test,..))` items.
    assert_eq!(test_count, 14, "every test-gated item kind is recognised");
    // `Plain`, `extern "C" {}`, and `cfg(feature = "test")` are not tests.
    assert_eq!(
        f.items.len() - test_count,
        3,
        "non-test items are not flagged"
    );
}
