use super::super::shared::{ForeignLib, PendingUse, ReexportMap};
use super::*;
use crate::languages::rust::internal::{NodeId, Visibility};
use std::collections::HashMap;

#[test]
fn super_glob_only_marks_ancestor_namespace_pulls() {
    let pu = |use_path: &[&str], current: &[&str], glob: bool| PendingUse {
        from_mod_id: "x".into(),
        current_path: current.iter().map(|s| s.to_string()).collect(),
        use_path: use_path.iter().map(|s| s.to_string()).collect(),
        visibility: Visibility::Private,
        bare: false,
        glob,
        line: None,
    };
    // `use super::*` and `use crate::<ancestor>::*` from a child -> super.
    assert!(is_super_glob(&pu(&["super"], &["assets", "lazy"], true)));
    assert!(is_super_glob(&pu(
        &["crate", "assets"],
        &["assets", "lazy"],
        true
    )));
    // Globbing a *child* module (descendant) is not a super pull.
    assert!(!is_super_glob(&pu(&["serialized"], &["assets"], true)));
    // `use self::*` resolves to a descendant of the current module (not an
    // ancestor), so it is not a super pull either (exercises the `self` arm).
    assert!(!is_super_glob(&pu(&["self", "sub"], &["assets"], true)));
    // A specific (non-glob) import of a parent item is a real dependency.
    assert!(!is_super_glob(&pu(
        &["crate", "syntax_mapping"],
        &["syntax_mapping", "builtin"],
        false
    )));
    // A glob of an unrelated/extern module is not an ancestor pull.
    assert!(!is_super_glob(&pu(
        &["rayon", "prelude"],
        &["assets"],
        true
    )));
}

#[test]
fn resolve_use_path_simple_cases() {
    // Single-shot resolutions over a bare module index + externs, with no
    // deps / foreign libs / re-exports in play. Those richer mechanisms keep
    // their own dedicated tests below (follows_reexport_to_definer,
    // resolves_cross_crate_*), since each needs a distinct fixture and asserts
    // more than one outcome — collapsing them here would hurt clarity.
    let s = |x: &str| x.to_string();
    // (label, use_path, current_module, index_entries, extern_entries, want)
    struct Case {
        label: &'static str,
        path: Vec<String>,
        current: Vec<String>,
        index: Vec<(Vec<String>, &'static str)>,
        externs: Vec<(&'static str, &'static str)>,
        want: Option<&'static str>,
    }
    let cases = vec![
        Case {
            label: "crate::a::b → AB",
            path: vec![s("crate"), s("a"), s("b")],
            current: vec![],
            index: vec![
                (vec![], "ROOT"),
                (vec![s("a")], "A"),
                (vec![s("a"), s("b")], "AB"),
            ],
            externs: vec![],
            want: Some("AB"),
        },
        Case {
            label: "super::super::x → root sibling X",
            path: vec![s("super"), s("super"), s("x")],
            current: vec![s("a"), s("b")],
            index: vec![
                (vec![], "ROOT"),
                (vec![s("a")], "A"),
                (vec![s("a"), s("b")], "AB"),
                (vec![s("x")], "X"),
            ],
            externs: vec![],
            want: Some("X"),
        },
        Case {
            label: "extern crate serde::Deserialize",
            path: vec![s("serde"), s("Deserialize")],
            current: vec![],
            index: vec![],
            externs: vec![("serde", "crate:serde")],
            want: Some("crate:serde"),
        },
        Case {
            label: "std is suppressed",
            path: vec![s("std"), s("collections")],
            current: vec![],
            index: vec![],
            externs: vec![],
            want: None,
        },
    ];
    let mut fails = Vec::new();
    for c in &cases {
        let idx: HashMap<Vec<String>, NodeId> = c
            .index
            .iter()
            .cloned()
            .map(|(k, v)| (k, v.into()))
            .collect();
        let externs: HashMap<String, NodeId> = c
            .externs
            .iter()
            .map(|(k, v)| (k.to_string(), (*v).into()))
            .collect();
        let got = resolve_use_path(
            &c.path,
            &c.current,
            &idx,
            &externs,
            &HashMap::new(),
            &HashMap::new(),
            &ReexportMap::new(),
            0,
        );
        if got.as_deref() != c.want {
            fails.push(format!(
                "{}: want {:?}, got {:?}",
                c.label,
                c.want,
                got.as_deref()
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "resolve_use_path cases failed:\n{}",
        fails.join("\n")
    );
}

#[test]
fn follows_reexport_to_definer() {
    // domain/ has children error, local_client. `domain/mod.rs` re-exports
    // `DomainError` from `error`. A sibling's `use super::DomainError` must
    // resolve to `domain::error` (the definer), not `domain` (the facade).
    let mut idx: HashMap<Vec<String>, NodeId> = HashMap::new();
    idx.insert(vec![], "ROOT".into());
    idx.insert(vec!["domain".into()], "DOMAIN".into());
    idx.insert(vec!["domain".into(), "error".into()], "ERROR".into());
    idx.insert(vec!["domain".into(), "local_client".into()], "LC".into());

    // `pub use error::DomainError;` declared inside the `domain` module.
    let mut rx = ReexportMap::new();
    rx.insert(
        vec!["domain".into()],
        vec![(
            "DomainError".into(),
            vec!["error".into(), "DomainError".into()],
        )],
    );

    // From `domain::local_client`, `use super::DomainError`.
    let r = resolve_use_path(
        &["super".into(), "DomainError".into()],
        &["domain".into(), "local_client".into()],
        &idx,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &rx,
        0,
    );
    assert_eq!(r.as_deref(), Some("ERROR"));

    // Without the re-export table it falls back to the facade module.
    let r0 = resolve_use_path(
        &["super".into(), "DomainError".into()],
        &["domain".into(), "local_client".into()],
        &idx,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &ReexportMap::new(),
        0,
    );
    assert_eq!(r0.as_deref(), Some("DOMAIN"));
}

#[test]
fn resolve_use_path_handles_intra_crate_bare_path() {
    let mut index: HashMap<Vec<String>, NodeId> = HashMap::new();
    index.insert(vec![], "mod:crate".into());
    index.insert(vec!["commands".into()], "mod:commands".into());
    let externs: HashMap<String, NodeId> = HashMap::new();
    let no_deps: HashMap<String, String> = HashMap::new();
    let no_libs: HashMap<String, ForeignLib> = HashMap::new();
    assert_eq!(
        resolve_use_path(
            &["commands".into(), "run".into()],
            &[],
            &index,
            &externs,
            &no_deps,
            &no_libs,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("mod:commands")
    );
    let mut externs2: HashMap<String, NodeId> = HashMap::new();
    externs2.insert("once_cell".into(), "crate:once_cell".into());
    assert_eq!(
        resolve_use_path(
            &["once_cell".into(), "sync".into()],
            &[],
            &index,
            &externs2,
            &no_deps,
            &no_libs,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("crate:once_cell")
    );
}

#[test]
fn resolves_cross_crate_use_to_submodule_file() {
    // The foreign crate's library module index: root + a `node` submodule.
    let mut foreign: HashMap<Vec<String>, NodeId> = HashMap::new();
    foreign.insert(vec![], "mod:api::lib".into());
    foreign.insert(vec!["node".into()], "mod:api::lib::node".into());
    let mut lib_index: HashMap<String, ForeignLib> = HashMap::new();
    lib_index.insert(
        "api 1.0".into(),
        ForeignLib {
            index: foreign,
            reexports: ReexportMap::new(),
        },
    );

    let mut dep_pkg_by_name: HashMap<String, String> = HashMap::new();
    dep_pkg_by_name.insert("api".into(), "api 1.0".into());
    // Fallback crate-root node, used only when the path stops above any submodule.
    let mut externs: HashMap<String, NodeId> = HashMap::new();
    externs.insert("api".into(), "crate:api".into());

    // `use api::node::Node` lands on the `node` submodule (not the crate root).
    assert_eq!(
        resolve_use_path(
            &["api".into(), "node".into(), "Node".into()],
            &[],
            &HashMap::new(),
            &externs,
            &dep_pkg_by_name,
            &lib_index,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("mod:api::lib::node")
    );
    // `use api::TopItem` (no matching submodule) falls back to the crate root.
    assert_eq!(
        resolve_use_path(
            &["api".into(), "TopItem".into()],
            &[],
            &HashMap::new(),
            &externs,
            &dep_pkg_by_name,
            &lib_index,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("mod:api::lib")
    );
}

#[test]
fn resolves_cross_crate_reexport_to_definer() {
    // Foreign crate `sec`: its root re-exports `AccessScope` (defined in the
    // `access_scope` submodule) via `pub use access_scope::AccessScope`.
    let mut foreign: HashMap<Vec<String>, NodeId> = HashMap::new();
    foreign.insert(vec![], "mod:sec::lib".into());
    foreign.insert(
        vec!["access_scope".into()],
        "mod:sec::lib::access_scope".into(),
    );
    let mut rx = ReexportMap::new();
    rx.insert(
        vec![],
        vec![(
            "AccessScope".into(),
            vec!["access_scope".into(), "AccessScope".into()],
        )],
    );
    let mut lib_index: HashMap<String, ForeignLib> = HashMap::new();
    lib_index.insert(
        "sec 1.0".into(),
        ForeignLib {
            index: foreign,
            reexports: rx,
        },
    );
    let mut dep_pkg_by_name: HashMap<String, String> = HashMap::new();
    dep_pkg_by_name.insert("sec".into(), "sec 1.0".into());
    let mut externs: HashMap<String, NodeId> = HashMap::new();
    externs.insert("sec".into(), "crate:sec".into());

    // `use sec::AccessScope` → the defining file, not the facade crate root.
    assert_eq!(
        resolve_use_path(
            &["sec".into(), "AccessScope".into()],
            &[],
            &HashMap::new(),
            &externs,
            &dep_pkg_by_name,
            &lib_index,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("mod:sec::lib::access_scope")
    );
    // A symbol the foreign crate does NOT re-export stays at the crate root.
    assert_eq!(
        resolve_use_path(
            &["sec".into(), "NotReexported".into()],
            &[],
            &HashMap::new(),
            &externs,
            &dep_pkg_by_name,
            &lib_index,
            &ReexportMap::new(),
            0,
        )
        .as_deref(),
        Some("mod:sec::lib")
    );
}

fn use_paths(src: &str) -> Vec<Vec<String>> {
    let f = syn::parse_file(src).unwrap();
    let mut out = Vec::new();
    for item in &f.items {
        if let syn::Item::Use(u) = item {
            collect_use_paths(&u.tree, Vec::new(), &mut out);
        }
    }
    out.into_iter().map(|(p, _)| p).collect()
}

#[test]
fn flattens_simple_use() {
    let paths = use_paths("use foo::bar::Baz;");
    assert_eq!(paths, vec![vec!["foo", "bar", "Baz"]]);
}

#[test]
fn flattens_group() {
    let paths = use_paths("use foo::{bar, baz::Qux};");
    assert_eq!(paths, vec![vec!["foo", "bar"], vec!["foo", "baz", "Qux"],]);
}

#[test]
fn flattens_glob() {
    let paths = use_paths("use foo::bar::*;");
    assert_eq!(paths, vec![vec!["foo", "bar"]]);
}
