//! Module-tree walk cluster, extracted from `module_graph.rs` to keep per-file
//! complexity under the project's thresholds. Pure code movement: walks a
//! crate's files and inline modules, building module nodes / `contains` edges
//! and collecting pending `use` / bare-path references for later resolution.

use super::resolve::collect_use_paths;
use super::shared::{PendingUse, crate_label, module_node_id, target_kind_label};
use crate::internal::{Edge, EdgeKind, GraphBuilder, Node, NodeId, NodeKind, Visibility};
use anyhow::{Context, Result};
use cargo_metadata::{Package, Target};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned as _;
use syn::{Item, ItemMod, Visibility as SynVis};

/// Collects every qualified path (≥ 2 segments) in a parsed file.
#[derive(Default)]
pub(super) struct CratePathCollector {
    pub(super) paths: std::collections::BTreeSet<Vec<String>>,
}

impl<'ast> syn::visit::Visit<'ast> for CratePathCollector {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.segments.len() >= 2 {
            self.paths
                .insert(path.segments.iter().map(|s| s.ident.to_string()).collect());
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        // `#[derive(...)]` arguments are an opaque token stream that the default
        // traversal never parses into paths, so a crate used *only* via a
        // qualified derive (e.g. `#[derive(serde::Serialize)]` with no `use
        // serde`) would otherwise produce no edge. Parse the derive list as a
        // comma-separated path list and record each qualified path.
        if attr.path().is_ident("derive")
            && let Ok(paths) = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
        {
            for p in &paths {
                if p.segments.len() >= 2 {
                    self.paths
                        .insert(p.segments.iter().map(|s| s.ident.to_string()).collect());
                }
            }
        }
        // Other attributes (`#[tokio::main]`, `#[serde(...)]`, …) keep the
        // default visit, which already routes the attribute's own path through
        // `visit_path`.
        syn::visit::visit_attribute(self, attr);
    }
}

/// Counts `unsafe` usages in a parsed file: `unsafe { }` expression blocks plus
/// `unsafe fn` / `unsafe impl` / `unsafe trait` declarations. Purely syntactic —
/// it does not (and cannot, without type info) tell an `unsafe` block doing real
/// work from a trivially-justified one, and `unsafe` produced inside a macro body
/// is invisible (macros are never expanded).
#[derive(Default)]
pub(super) struct UnsafeCounter {
    pub(super) count: u32,
}

impl<'ast> syn::visit::Visit<'ast> for UnsafeCounter {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.count += 1;
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if node.sig.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if node.sig.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if node.unsafety.is_some() {
            self.count += 1;
        }
        syn::visit::visit_item_trait(self, node);
    }
}

fn convert_visibility(v: &SynVis) -> Visibility {
    match v {
        SynVis::Public(_) => Visibility::Public,
        SynVis::Restricted(r) => {
            let s = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            match s.as_str() {
                "crate" => Visibility::Crate,
                "super" => Visibility::Super,
                "self" | "" => Visibility::Private,
                _ => Visibility::Restricted { path: s },
            }
        }
        SynVis::Inherited => Visibility::Private,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn walk_file(
    file_path: &Path,
    parent_mod_id: &NodeId,
    parent_mod_path: &[String],
    pkg: &Package,
    target: &Target,
    ignore_tests: bool,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    if !visited_files.insert(file_path.to_path_buf()) {
        return Ok(());
    }
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("reading {}", file_path.display()))?;
    let parsed =
        syn::parse_file(&content).with_context(|| format!("parsing {}", file_path.display()))?;

    let loc = content.lines().count() as u32;
    let item_count = count_items(&parsed.items) as u32;

    // Walk the non-test items once, driving two visitors: the bare-path
    // collector and the `unsafe` counter. When skipping tests, visit only
    // non-test items so neither do references made solely by `#[cfg(test)]` code
    // become edges, nor does test-only `unsafe` inflate the count (consistent
    // with how `sloc`/complexity exclude tests).
    let mut collector = CratePathCollector::default();
    let mut unsafe_counter = UnsafeCounter::default();
    for item in &parsed.items {
        if ignore_tests && is_test_item(item) {
            continue;
        }
        syn::visit::Visit::visit_item(&mut collector, item);
        syn::visit::Visit::visit_item(&mut unsafe_counter, item);
    }

    // Annotate the parent module node with LOC, item_count and unsafe count.
    if let Some(node) = builder
        .nodes_mut()
        .iter_mut()
        .find(|n| n.id == *parent_mod_id)
    {
        node.loc = Some(loc);
        node.item_count = Some(item_count);
        node.unsafe_count = Some(unsafe_counter.count);
        node.path = file_path.display().to_string();
    }

    for path in collector.paths {
        pending_uses.push(PendingUse {
            from_mod_id: parent_mod_id.clone(),
            current_path: parent_mod_path.to_vec(),
            use_path: path,
            visibility: Visibility::Private,
            bare: true,
            glob: false,
            line: None,
        });
    }

    walk_items(
        &parsed.items,
        parent_mod_id,
        parent_mod_path,
        file_path,
        pkg,
        target,
        ignore_tests,
        module_index,
        pending_uses,
        builder,
        visited_files,
    )
}

#[allow(clippy::too_many_arguments)]
fn walk_items(
    items: &[Item],
    current_mod_id: &NodeId,
    current_mod_path: &[String],
    enclosing_file: &Path,
    pkg: &Package,
    target: &Target,
    ignore_tests: bool,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    for item in items {
        // Skip `#[cfg(test)]` / `#[test]` / `#[bench]` items entirely when
        // requested — their modules, `use`s and bare paths are test-only.
        if ignore_tests && is_test_item(item) {
            continue;
        }
        match item {
            Item::Mod(m) => {
                process_mod(
                    m,
                    current_mod_id,
                    current_mod_path,
                    enclosing_file,
                    pkg,
                    target,
                    ignore_tests,
                    module_index,
                    pending_uses,
                    builder,
                    visited_files,
                )?;
            }
            Item::Use(u) => {
                let mut paths = Vec::new();
                collect_use_paths(&u.tree, Vec::new(), &mut paths);
                let vis = convert_visibility(&u.vis);
                let line = Some(u.span().start().line as u32);
                for (use_path, glob) in paths {
                    pending_uses.push(PendingUse {
                        from_mod_id: current_mod_id.clone(),
                        current_path: current_mod_path.to_vec(),
                        use_path,
                        visibility: vis.clone(),
                        bare: false,
                        glob,
                        line,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_mod(
    m: &ItemMod,
    parent_mod_id: &NodeId,
    parent_mod_path: &[String],
    enclosing_file: &Path,
    pkg: &Package,
    target: &Target,
    ignore_tests: bool,
    module_index: &mut HashMap<Vec<String>, NodeId>,
    pending_uses: &mut Vec<PendingUse>,
    builder: &mut GraphBuilder,
    visited_files: &mut HashSet<PathBuf>,
) -> Result<()> {
    let sub_name = m.ident.to_string();
    let mut sub_path = parent_mod_path.to_vec();
    sub_path.push(sub_name.clone());
    let sub_mod_id = module_node_id(
        &pkg.id.repr,
        target_kind_label(target),
        &target.name,
        &sub_path,
    );

    let (loc, line) = if m.content.is_some() {
        let span = m.span();
        let start = span.start().line as u32;
        let end = span.end().line as u32;
        (Some(end - start + 1), Some(start))
    } else {
        (None, None)
    };
    builder.add_node(Node {
        id: sub_mod_id.clone(),
        kind: NodeKind::Module,
        name: sub_name.clone(),
        path: enclosing_file.display().to_string(),
        parent: Some(parent_mod_id.clone()),
        external: None,
        version: None,
        visibility: Some(convert_visibility(&m.vis)),
        loc,
        line,
        item_count: None,
        unsafe_count: None,
        crate_label: Some(crate_label(pkg, target)),
    });
    builder.add_edge(Edge {
        from: parent_mod_id.clone(),
        to: sub_mod_id.clone(),
        kind: EdgeKind::Contains,
        visibility: None,
        line: None,
    });
    module_index.insert(sub_path.clone(), sub_mod_id.clone());

    if let Some((_, items)) = &m.content {
        walk_items(
            items,
            &sub_mod_id,
            &sub_path,
            enclosing_file,
            pkg,
            target,
            ignore_tests,
            module_index,
            pending_uses,
            builder,
            visited_files,
        )?;
    } else if let Some(sub_file) = mod_file_path(m, enclosing_file, &sub_name) {
        walk_file(
            &sub_file,
            &sub_mod_id,
            &sub_path,
            pkg,
            target,
            ignore_tests,
            module_index,
            pending_uses,
            builder,
            visited_files,
        )?;
    }
    Ok(())
}

/// Resolve the file backing `mod <name>;`. Honours an explicit
/// `#[path = "rel/or/abs.rs"]` attribute (relative to the directory of the file
/// containing the declaration) before falling back to the default
/// `name.rs` / `name/mod.rs` lookup. Without this, a `#[path]` module — and
/// every edge inside it — would be silently dropped.
fn mod_file_path(m: &ItemMod, enclosing_file: &Path, sub_name: &str) -> Option<PathBuf> {
    if let Some(rel) = mod_path_attr(m) {
        let base = enclosing_file.parent().unwrap_or_else(|| Path::new(""));
        let candidate = base.join(&rel);
        return candidate.exists().then_some(candidate);
    }
    resolve_submodule_path(enclosing_file, sub_name)
}

/// Read the string value of a `#[path = "..."]` attribute on a module, if present.
fn mod_path_attr(m: &ItemMod) -> Option<String> {
    for attr in &m.attrs {
        if attr.path().is_ident("path")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return Some(s.value());
        }
    }
    None
}

fn resolve_submodule_path(parent_file: &Path, mod_name: &str) -> Option<PathBuf> {
    let parent_dir = parent_file.parent()?;
    let parent_stem = parent_file.file_stem()?.to_str()?;

    let search_dir = if matches!(parent_stem, "lib" | "main" | "mod") {
        parent_dir.to_path_buf()
    } else {
        parent_dir.join(parent_stem)
    };

    let candidate_a = search_dir.join(format!("{mod_name}.rs"));
    if candidate_a.exists() {
        return Some(candidate_a);
    }
    let candidate_b = search_dir.join(mod_name).join("mod.rs");
    if candidate_b.exists() {
        return Some(candidate_b);
    }
    None
}

/// True for a top-level item gated to tests (`#[cfg(test)]` module,
/// `#[test]`/`#[bench]`/`#[cfg(test)]` fn, etc). Mirrors the line-stripping in
/// `code-ranker-complexity` so the graph and the metrics agree on what is test.
pub(super) fn is_test_item(item: &Item) -> bool {
    let attrs: &[syn::Attribute] = match item {
        Item::Mod(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Const(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        _ => return false,
    };
    attrs.iter().any(is_test_attr)
}

/// True if an attribute gates an item to tests: `#[test]`, `#[bench]`, or a
/// `cfg(...)` whose predicate contains a bare `test` identifier
/// (`#[cfg(test)]`, `#[cfg(all(test, …))]`). `cfg(feature = "test")` does not
/// match — only the `test` *identifier* does.
fn is_test_attr(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("test") || attr.path().is_ident("bench") {
        return true;
    }
    if attr.path().is_ident("cfg")
        && let Ok(list) = attr.meta.require_list()
    {
        return tokens_have_test_ident(list.tokens.clone());
    }
    false
}

/// Recursively scan a token stream for a bare `test` identifier (descends into
/// `all(...)` / `any(...)` / `not(...)` groups).
fn tokens_have_test_ident(ts: proc_macro2::TokenStream) -> bool {
    ts.into_iter().any(|tt| match tt {
        proc_macro2::TokenTree::Ident(i) => i == "test",
        proc_macro2::TokenTree::Group(g) => tokens_have_test_ident(g.stream()),
        _ => false,
    })
}

fn count_items(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|i| {
            matches!(
                i,
                Item::Fn(_)
                    | Item::Struct(_)
                    | Item::Enum(_)
                    | Item::Trait(_)
                    | Item::Impl(_)
                    | Item::Type(_)
                    | Item::Const(_)
                    | Item::Static(_)
                    | Item::Mod(_)
                    | Item::Macro(_)
                    | Item::Union(_)
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
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
        let f = syn::parse_file("#[derive(Debug, serde::Serialize, thiserror::Error)] struct S;")
            .unwrap();
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
}
