use super::*;
use code_ranker_plugin_api::attrs::AttrValue;

fn node(id: &str, attrs: &[(&str, AttrValue)]) -> Node {
    Node {
        id: id.into(),
        kind: "file".into(),
        name: id.into(),
        parent: None,
        attrs: attrs
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect(),
    }
}

fn def(when: &str) -> CheckDef {
    CheckDef {
        when: when.into(),
        message: "hit".into(),
        group: None,
        why: None,
        fix: None,
        title: None,
    }
}

fn no_defs() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn compiled(when: &str) -> CompiledCheck {
    compile("t", &def(when), &no_defs()).expect("compiles")
}

#[test]
fn numeric_predicate_fires_over_threshold() {
    let n = node("a.rs", &[("tloc", AttrValue::Int(120))]);
    assert!(
        compiled("tloc > 100")
            .eval(&n, &GraphView::default())
            .is_some()
    );
    assert!(
        compiled("tloc > 200")
            .eval(&n, &GraphView::default())
            .is_none()
    );
}

#[test]
fn path_strings_and_string_stdlib_are_available() {
    // path comes from the `path` attr; stem/ext/dir/name are derived.
    let n = node(
        "x",
        &[
            ("path", AttrValue::Str("crates/a/src/handler.rs".into())),
            ("tloc", AttrValue::Int(5)),
        ],
    );
    // Inline tests in a production (non-_tests) file.
    assert!(
        compiled(r#"tloc > 0 && !path.endsWith("_tests.rs")"#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
    // A sibling _tests.rs file is exempt.
    let t = node(
        "y",
        &[
            (
                "path",
                AttrValue::Str("crates/a/src/handler_tests.rs".into()),
            ),
            ("tloc", AttrValue::Int(5)),
        ],
    );
    assert!(
        compiled(r#"tloc > 0 && !path.endsWith("_tests.rs")"#)
            .eval(&t, &GraphView::default())
            .is_none()
    );
}

#[test]
fn derived_path_fields_resolve() {
    let n = node(
        "x",
        &[("path", AttrValue::Str("crates/a/src/handler.rs".into()))],
    );
    assert!(
        compiled(r#"stem == "handler""#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
    assert!(
        compiled(r#"ext == "rs""#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
    assert!(
        compiled(r#"name == "handler.rs""#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
    assert!(
        compiled(r#"dir == "crates/a/src""#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
}

#[test]
fn path_falls_back_to_id_without_target_prefix() {
    let n = node("{target}/src/main.rs", &[("tloc", AttrValue::Int(3))]);
    assert!(
        compiled(r#"path == "src/main.rs""#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
}

#[test]
fn matches_uses_regex_and_tolerates_bad_pattern() {
    let n = node(
        "x",
        &[("path", AttrValue::Str("src/api/rest/handler.rs".into()))],
    );
    assert!(
        compiled(r#"matches(path, "api/.*\\.rs$")"#)
            .eval(&n, &GraphView::default())
            .is_some()
    );
    // A malformed pattern never panics — it just doesn't match.
    assert!(
        compiled(r#"matches(path, "(")"#)
            .eval(&n, &GraphView::default())
            .is_none()
    );
}

#[test]
fn agg_enables_relative_thresholds() {
    // Three files with cyclomatic 1 / 5 / 100 — only the outlier is above the
    // project's own p90, a threshold no fixed number could express portably.
    let n = |id: &str, c: i64| node(id, &[("cyclomatic", AttrValue::Int(c))]);
    let level = LevelGraph {
        nodes: vec![n("a.rs", 1), n("b.rs", 5), n("c.rs", 100)],
        ..Default::default()
    };
    let view = GraphView::build(&level);
    let check = compiled("cyclomatic.double() > agg('cyclomatic', 'p90', 'not_empty')");
    assert!(
        check.eval(&level.nodes[2], &view).is_some(),
        "outlier fires"
    );
    assert!(
        check.eval(&level.nodes[0], &view).is_none(),
        "low file does not"
    );
    // A 'max' aggregate is the project max — nothing strictly exceeds it.
    assert!(
        compiled("cyclomatic.double() > agg('cyclomatic', 'max', 'not_empty')")
            .eval(&level.nodes[2], &view)
            .is_none()
    );
}

#[test]
fn math_host_functions_are_available_in_predicates() {
    let n = node("x", &[("hk", AttrValue::Int(64))]);
    let g = GraphView::default();
    // `pow`, `sqrt`, `log2` etc. — the same math the metric engine has.
    assert!(compiled("sqrt(hk.double()) == 8.0").eval(&n, &g).is_some());
    assert!(compiled("log2(hk.double()) == 6.0").eval(&n, &g).is_some());
    assert!(compiled("pow(2.0, 3.0) == 8.0").eval(&n, &g).is_some());
}

#[test]
fn message_interpolates_attrs_and_path_fields() {
    let n = node(
        "x",
        &[
            ("path", AttrValue::Str("src/handler.rs".into())),
            ("tloc", AttrValue::Int(42)),
        ],
    );
    let check = compile(
        "de",
        &CheckDef {
            when: "tloc > 0".into(),
            message: "{name}: {tloc} inline test lines; {unknown} stays".into(),
            group: Some("TST".into()),
            why: Some("why".into()),
            fix: Some("fix".into()),
            title: None,
        },
        &no_defs(),
    )
    .unwrap();
    let hit = check.eval(&n, &GraphView::default()).expect("fires");
    assert_eq!(
        hit.message,
        "handler.rs: 42 inline test lines; {unknown} stays"
    );
    assert_eq!(hit.group, "TST");
    assert_eq!(hit.why.as_deref(), Some("why"));
}

#[test]
fn why_and_fix_copy_is_interpolated() {
    let n = node(
        "x",
        &[
            ("path", AttrValue::Str("src/handler.rs".into())),
            ("tloc", AttrValue::Int(5)),
        ],
    );
    let check = compile(
        "de",
        &CheckDef {
            when: "tloc > 0".into(),
            message: "m".into(),
            group: None,
            why: None,
            fix: Some("move into `{stem}_tests.rs`".into()),
            title: None,
        },
        &no_defs(),
    )
    .unwrap();
    let hit = check.eval(&n, &GraphView::default()).expect("fires");
    assert_eq!(hit.fix.as_deref(), Some("move into `handler_tests.rs`"));
}

#[test]
fn double_cast_enables_float_proportion_predicate() {
    // tloc/sloc = 60/100 = 0.6 — fires above 0.5, not above 0.8. The
    // `.double()` casts make `/` a float division (bare int `/` would
    // truncate 60/100 to 0).
    let n = node(
        "a.rs",
        &[
            ("loc", AttrValue::Int(180)),
            ("sloc", AttrValue::Int(100)),
            ("tloc", AttrValue::Int(60)),
        ],
    );
    assert!(
        compiled("loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.5")
            .eval(&n, &GraphView::default())
            .is_some()
    );
    assert!(
        compiled("loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.8")
            .eval(&n, &GraphView::default())
            .is_none()
    );
    // The same proportion on a file under 100 lines never fires.
    let small = node(
        "b.rs",
        &[
            ("loc", AttrValue::Int(40)),
            ("sloc", AttrValue::Int(20)),
            ("tloc", AttrValue::Int(18)),
        ],
    );
    assert!(
        compiled("loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.5")
            .eval(&small, &GraphView::default())
            .is_none()
    );
    // A file with no production source never fires: the `sloc > 0` guard is
    // false, and 0/0 would be NaN (not > 0.5) regardless.
    let no_src = node(
        "c.rs",
        &[
            ("loc", AttrValue::Int(150)),
            ("sloc", AttrValue::Int(0)),
            ("tloc", AttrValue::Int(5)),
        ],
    );
    assert!(
        compiled("loc > 100 && sloc > 0 && tloc.double() / sloc.double() > 0.5")
            .eval(&no_src, &GraphView::default())
            .is_none()
    );
}

#[test]
fn non_boolean_or_error_predicate_does_not_fire() {
    let n = node("a.rs", &[("tloc", AttrValue::Int(5))]);
    // A numeric result is not a boolean → no hit (and no panic).
    assert!(
        compiled("tloc + 1")
            .eval(&n, &GraphView::default())
            .is_none()
    );
    // Referencing an absent variable errors → no hit.
    assert!(
        compiled("missing_attr > 0")
            .eval(&n, &GraphView::default())
            .is_none()
    );
}

// ── Graph-aware predicates (edges + collections) ────────────────────────

use crate::level_graph::LevelGraph;
use code_ranker_plugin_api::edge::Edge;

fn file_node(id: &str, path: &str) -> Node {
    node(id, &[("path", AttrValue::Str(path.into()))])
}

fn edge(source: &str, target: &str) -> Edge {
    Edge {
        source: source.into(),
        target: target.into(),
        kind: "uses".into(),
        line: None,
        attrs: Default::default(),
    }
}

/// A small level: a domain file that depends on an infra file and on the
/// external crate `sqlx`, plus an unrelated api file.
fn sample_graph() -> (LevelGraph, GraphView) {
    let mut ext = node("ext:sqlx", &[]);
    ext.kind = EXTERNAL.into();
    let level = LevelGraph {
        nodes: vec![
            file_node("{t}/domain/order.rs", "src/domain/order.rs"),
            file_node("{t}/infra/db.rs", "src/infra/db.rs"),
            file_node("{t}/api/rest/order.rs", "src/api/rest/order.rs"),
            ext,
        ],
        edges: vec![
            edge("{t}/domain/order.rs", "{t}/infra/db.rs"),
            edge("{t}/domain/order.rs", "ext:sqlx"),
        ],
        ..Default::default()
    };
    let view = GraphView::build(&level);
    (level, view)
}

#[test]
fn depends_on_and_deps_list_see_edges() {
    let (level, view) = sample_graph();
    let domain = &level.nodes[0];
    // `depends_on` substring helper over out-neighbour labels.
    assert!(
        compiled(r#"depends_on("ext:sqlx")"#)
            .eval(domain, &view)
            .is_some()
    );
    assert!(
        compiled(r#"depends_on("/infra/")"#)
            .eval(domain, &view)
            .is_some()
    );
    assert!(
        compiled(r#"depends_on("/nope/")"#)
            .eval(domain, &view)
            .is_none()
    );
    // `deps` list + comprehension macro: a domain file must not reach infra.
    assert!(
        compiled(r#"contains(path,"/domain/") && deps.exists(d, contains(d,"/infra/"))"#)
            .eval(domain, &view)
            .is_some()
    );
    // The api file has no out-edges → no violation.
    let api = &level.nodes[2];
    assert!(
        compiled(r#"deps.exists(d, contains(d,"/infra/"))"#)
            .eval(api, &view)
            .is_none()
    );
}

#[test]
fn depended_on_by_and_rdeps_see_reverse_edges() {
    let (level, view) = sample_graph();
    let infra = &level.nodes[1];
    assert!(
        compiled(r#"depended_on_by("/domain/")"#)
            .eval(infra, &view)
            .is_some()
    );
    assert!(compiled("rdeps.size() >= 1").eval(infra, &view).is_some());
    // External node aside, the api file is depended on by nobody here.
    let api = &level.nodes[2];
    assert!(compiled("rdeps.size() == 0").eval(api, &view).is_some());
}

#[test]
fn files_and_siblings_collections_and_file_exists() {
    let (level, view) = sample_graph();
    let domain = &level.nodes[0]; // src/domain/order.rs
    // `files` excludes the external crate (3 real files).
    assert!(compiled("files.size() == 3").eval(domain, &view).is_some());
    // `file_exists` over the project file set.
    assert!(
        compiled(r#"file_exists("src/infra/db.rs")"#)
            .eval(domain, &view)
            .is_some()
    );
    assert!(
        compiled(r#"file_exists("src/nope.rs")"#)
            .eval(domain, &view)
            .is_none()
    );
    // `siblings` = same folder, excluding self. order.rs is alone in domain/.
    assert!(
        compiled("siblings.size() == 0")
            .eval(domain, &view)
            .is_some()
    );
}

#[test]
fn collections_consistent_when_nodes_carry_only_target_prefixed_ids() {
    // Real snapshot nodes may carry no `path` attr — only a `{target}/…` id.
    // The folder index and a node's own `dir` must still agree (regression:
    // `label_of` and `node_path` both strip `{target}/`).
    let n1 = node("{target}/src/config/a.rs", &[("loc", AttrValue::Int(500))]);
    let n2 = node("{target}/src/config/b.rs", &[("loc", AttrValue::Int(50))]);
    let level = LevelGraph {
        nodes: vec![n1.clone(), n2],
        ..Default::default()
    };
    let view = GraphView::build(&level);
    // a.rs has a sibling (b.rs) → NOT alone, despite the {target}/ prefix.
    assert!(
        compiled("loc > 400 && siblings.size() == 0")
            .eval(&n1, &view)
            .is_none()
    );
    assert!(compiled("siblings.size() == 1").eval(&n1, &view).is_some());
    // `files` carries the stripped paths.
    assert!(
        compiled(r#"file_exists("src/config/b.rs")"#)
            .eval(&n1, &view)
            .is_some()
    );
}

#[test]
fn defs_are_expanded_into_predicates() {
    let mut defs = BTreeMap::new();
    defs.insert(
        "is_domain".to_string(),
        r#"contains(path, "/domain/")"#.to_string(),
    );
    defs.insert(
        "reaches_infra".to_string(),
        r#"deps.exists(d, contains(d, "/infra/"))"#.to_string(),
    );
    let (level, view) = sample_graph();
    let check = compile("layer", &def("is_domain && reaches_infra"), &defs).unwrap();
    assert!(check.eval(&level.nodes[0], &view).is_some()); // domain → infra
    assert!(check.eval(&level.nodes[2], &view).is_none()); // api, no infra dep
}

#[test]
fn defs_reference_chain_resolves() {
    let mut defs = BTreeMap::new();
    defs.insert("a".to_string(), "tloc > 0".to_string());
    defs.insert("b".to_string(), "a && loc > 100".to_string()); // b uses a
    let n = node(
        "x.rs",
        &[("tloc", AttrValue::Int(5)), ("loc", AttrValue::Int(150))],
    );
    let check = compile("c", &def("b"), &defs).unwrap();
    assert!(check.eval(&n, &GraphView::default()).is_some());
}

#[test]
fn cyclic_defs_are_a_compile_error() {
    let mut defs = BTreeMap::new();
    defs.insert("a".to_string(), "b".to_string());
    defs.insert("b".to_string(), "a".to_string());
    assert!(compile("c", &def("a"), &defs).is_err());
}

#[test]
fn float_bool_attrs_path_fns_and_message_formatting() {
    let n = node(
        "x",
        &[
            ("path", AttrValue::Str("README".into())), // no extension
            ("mi", AttrValue::Float(72.5)),
            ("whole", AttrValue::Float(3.0)),
            ("flag", AttrValue::Bool(true)),
            ("n", AttrValue::Int(7)),
            ("lang", AttrValue::Str("rust".into())),
        ],
    );
    let g = GraphView::default();
    // Float + Bool attributes bind and compare in a predicate.
    assert!(compiled("mi > 70.0 && flag").eval(&n, &g).is_some());
    // Native CEL string methods (startsWith / contains).
    assert!(
        compiled(r#"path.startsWith("READ") && path.contains("ME")"#)
            .eval(&n, &g)
            .is_some()
    );
    // A file with no `.` → empty ext, stem == name.
    assert!(
        compiled(r#"ext == "" && stem == "README""#)
            .eval(&n, &g)
            .is_some()
    );
    // Message formatting: fractional float (2dp), whole float (no point), bool,
    // and an unmatched `{` left verbatim.
    let check = compile(
        "m",
        &CheckDef {
            when: "n > 0".into(),
            message:
                "mi={mi} whole={whole} flag={flag} ext=[{ext}] dir=[{dir}] lang={lang} dangling={"
                    .into(),
            group: None,
            why: None,
            fix: None,
            title: None,
        },
        &no_defs(),
    )
    .unwrap();
    let hit = check.eval(&n, &g).unwrap();
    assert_eq!(
        hit.message,
        "mi=72.50 whole=3 flag=true ext=[] dir=[] lang=rust dangling={"
    );
}

#[test]
fn bad_predicate_fails_to_compile() {
    let err = compile(
        "bad",
        &CheckDef {
            when: "tloc >".into(),
            message: "m".into(),
            group: None,
            why: None,
            fix: None,
            title: None,
        },
        &no_defs(),
    );
    assert!(err.is_err());
}
