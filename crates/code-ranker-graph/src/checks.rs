//! Custom config-defined checks (`[rules.checks.<id>]`) — a **config-only linter
//! primitive**.
//!
//! A [`CheckDef`] pairs a CEL boolean `when` predicate with a diagnostic
//! `message`. The predicate is evaluated **per node** over everything the node
//! carries: its numeric / boolean / string attributes (`tloc`, `unsafe`,
//! `cyclomatic`, …) **plus** derived path strings (`path`, `name`, `stem`,
//! `ext`, `dir`). String predicates use CEL's own stdlib (`contains`,
//! `startsWith`, `endsWith`, `matches` regex, `size`, `double`, …); on top of it
//! we register only the graph-aware functions (`depends_on` / `depended_on_by` /
//! `file_exists`). When the predicate is `true`, the check fires and produces a
//! [`CheckHit`] the CLI turns into a violation.
//!
//! This is what lets a project express a custom linter — e.g. "no inline tests
//! in a production file" (`tloc > 0 && !path.endsWith("_tests.rs")`) — entirely
//! in `code-ranker.toml`, with no Rust change. It complements
//! `[rules.thresholds.file]` (which only does `metric > limit`) with an arbitrary
//! boolean expression and a path-aware, string-aware context.

use crate::level_graph::LevelGraph;
use crate::nodepath::{node_path, split_path};
use cel::{Context, Program, Value};
use code_ranker_plugin_api::{attrs::AttrValue, node::EXTERNAL, node::Node};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// The default concern-group label for a check that doesn't set one.
const DEFAULT_GROUP: &str = "LNT";

/// One custom check from `[rules.checks.<id>]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckDef {
    /// CEL boolean predicate over the node's values. `true` → a violation.
    pub when: String,
    /// Diagnostic message. `{key}` placeholders are filled from the node's
    /// values at evaluation time (any attribute, or a derived path field
    /// `path`/`name`/`stem`/`ext`/`dir`). An unknown `{key}` is left verbatim.
    pub message: String,
    /// Concern-group label shown / grouped in diagnostics (free-form, e.g.
    /// `"TST"`). Defaults to [`DEFAULT_GROUP`].
    #[serde(default)]
    pub group: Option<String>,
    /// Optional diagnostic copy — the `why` / `fix` lines in `check` output.
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub fix: Option<String>,
    /// Optional title (SARIF `shortDescription`). Defaults to the check id.
    #[serde(default)]
    pub title: Option<String>,
}

/// A compiled check: its id, the parsed predicate program, its definition, and
/// which graph collections the predicate references (so eval binds only those).
pub struct CompiledCheck {
    pub id: String,
    pub def: CheckDef,
    program: Program,
    uses: Uses,
}

/// Which graph-derived list variables a predicate mentions. Binding the project
/// file list is O(files), so it is bound only when actually referenced.
#[derive(Default, Clone, Copy)]
struct Uses {
    deps: bool,
    rdeps: bool,
    files: bool,
    siblings: bool,
}

/// A `when` predicate that failed to compile (reported up-front, so the gate
/// fails loudly instead of silently skipping a misspelled check).
#[derive(Debug, Clone)]
pub struct CheckCompileError {
    pub id: String,
    pub message: String,
}

impl std::fmt::Display for CheckCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "check `{}`: invalid `when` predicate: {}",
            self.id, self.message
        )
    }
}

impl std::error::Error for CheckCompileError {}

/// A fired check on a node — the data a violation is built from.
#[derive(Debug, Clone)]
pub struct CheckHit {
    pub id: String,
    pub message: String,
    pub group: String,
    pub why: Option<String>,
    pub fix: Option<String>,
    pub title: Option<String>,
}

/// Compile one check's `when` predicate. Named helpers from `[rules.defs]` are
/// expanded into the predicate first (see [`expand_defs`]), so a check can reuse
/// a shared vocabulary (`is_domain`, `is_test_file`, …).
pub fn compile(
    id: &str,
    def: &CheckDef,
    defs: &BTreeMap<String, String>,
) -> Result<CompiledCheck, CheckCompileError> {
    let when = expand_defs(id, &def.when, defs)?;
    let program = Program::compile(&when).map_err(|e| CheckCompileError {
        id: id.to_string(),
        message: e.to_string(),
    })?;
    let uses = Uses {
        deps: references(&when, "deps"),
        rdeps: references(&when, "rdeps"),
        files: references(&when, "files"),
        siblings: references(&when, "siblings"),
    };
    Ok(CompiledCheck {
        id: id.to_string(),
        def: def.clone(),
        program,
        uses,
    })
}

impl CompiledCheck {
    /// Evaluate the predicate over `node`, with `graph` giving access to the
    /// fully-built level (edges + the file set) for dependency / collection
    /// predicates. Returns a [`CheckHit`] when it fires (`when` evaluates to
    /// `true`). A predicate that errors or yields a non-boolean value does
    /// **not** fire — a check never panics on a node.
    pub fn eval(&self, node: &Node, graph: &GraphView) -> Option<CheckHit> {
        // `Context::default()` already provides the CEL string stdlib
        // (`contains` / `startsWith` / `endsWith` / `matches` regex / `size` /
        // `double` / …). On top we add the same math host functions the metric
        // engine uses (`pow` / `log2` / `sqrt` / …) and the graph-aware functions,
        // so a predicate can do real arithmetic over node values.
        let mut ctx = Context::default();
        crate::registry::register_math(&mut ctx);
        // `agg(metric, reducer, population)` over the whole project, so a predicate
        // can use a relative threshold (this node vs the project distribution).
        // Memoized across nodes (see `GraphView::register_agg`).
        graph.register_agg(&mut ctx);
        register_graph_fns(&mut ctx, graph, &node.id);
        bind_node(&mut ctx, node);
        self.bind_collections(&mut ctx, node, graph);
        match self.program.execute(&ctx) {
            Ok(Value::Bool(true)) => Some(CheckHit {
                id: self.id.clone(),
                message: render_message(&self.def.message, node),
                group: self
                    .def
                    .group
                    .clone()
                    .unwrap_or_else(|| DEFAULT_GROUP.to_string()),
                // `{key}` placeholders are interpolated in the copy too, so a
                // per-file fix reads "move into `handler_tests.rs`", not `{stem}`.
                why: self.def.why.as_deref().map(|s| render_message(s, node)),
                fix: self.def.fix.as_deref().map(|s| render_message(s, node)),
                title: self.def.title.clone(),
            }),
            _ => None,
        }
    }

    /// Bind the graph-derived list variables the predicate actually references:
    /// `deps` / `rdeps` (out / in dependency neighbours of this node, by label),
    /// `files` (every project file path), `siblings` (files in the same folder).
    /// Each is a CEL list, usable with the comprehension macros (`.exists`,
    /// `.all`, `.filter`, `.size()`).
    fn bind_collections(&self, ctx: &mut Context, node: &Node, graph: &GraphView) {
        if self.uses.deps {
            let _ = ctx.add_variable("deps", graph.deps(&node.id));
        }
        if self.uses.rdeps {
            let _ = ctx.add_variable("rdeps", graph.rdeps(&node.id));
        }
        if self.uses.files {
            let _ = ctx.add_variable("files", graph.files_vec());
        }
        if self.uses.siblings {
            let _ = ctx.add_variable("siblings", graph.siblings(&node_path(node)));
        }
    }
}

/// Bind a node's values into the CEL context: every attribute under its own key
/// (numeric / boolean / string), plus the derived path fields.
fn bind_node(ctx: &mut Context, node: &Node) {
    for (key, value) in node.attrs.iter() {
        match value {
            AttrValue::Int(i) => {
                let _ = ctx.add_variable(key.as_str(), *i);
            }
            AttrValue::Float(f) => {
                let _ = ctx.add_variable(key.as_str(), *f);
            }
            AttrValue::Bool(b) => {
                let _ = ctx.add_variable(key.as_str(), *b);
            }
            AttrValue::Str(s) => {
                let _ = ctx.add_variable(key.as_str(), s.clone());
            }
        }
    }
    // Derived path fields. `path` may already be bound from the attr loop above;
    // re-binding it here is harmless (same value) and covers nodes that carry the
    // path only in their id.
    let path = node_path(node);
    let parts = split_path(&path);
    let _ = ctx.add_variable("path", path);
    let _ = ctx.add_variable("name", parts.name);
    let _ = ctx.add_variable("stem", parts.stem);
    let _ = ctx.add_variable("ext", parts.ext);
    let _ = ctx.add_variable("dir", parts.dir);
}

/// Register the graph-aware predicate functions for `node_id`, bound to the
/// fully-built level: `depends_on(s)` / `depended_on_by(s)` (does this node have
/// an out- / in-dependency whose label contains `s` — e.g. `"ext:sqlx"` or
/// `"/infrastructure/"`), and `file_exists(p)` (is `p` a file in the project).
fn register_graph_fns(ctx: &mut Context, graph: &GraphView, node_id: &str) {
    let out = graph.deps(node_id);
    ctx.add_function("depends_on", move |s: Arc<String>| -> bool {
        out.iter().any(|d| d.contains(s.as_str()))
    });
    let inc = graph.rdeps(node_id);
    ctx.add_function("depended_on_by", move |s: Arc<String>| -> bool {
        inc.iter().any(|d| d.contains(s.as_str()))
    });
    let files = graph.files_set_arc();
    ctx.add_function("file_exists", move |p: Arc<String>| -> bool {
        files.contains(p.as_str())
    });
}

/// A read-only view of the fully-built level, prepared once per `check` run and
/// shared across every node's predicate. Holds the dependency adjacency (by
/// node id → neighbour *labels*) and the project's file set / per-folder index.
/// A node's **label** is its repo-relative `path` attribute when present, else
/// its id (so an external crate stays `ext:<name>`).
#[derive(Default)]
pub struct GraphView {
    out: HashMap<String, Vec<String>>,
    inc: HashMap<String, Vec<String>>,
    files: Arc<Vec<String>>,
    files_set: Arc<HashSet<String>>,
    by_dir: HashMap<String, Vec<String>>,
    /// Value populations over all internal nodes, so a predicate can compare a
    /// node against the project distribution via `agg(metric, reducer, pop)` —
    /// e.g. a relative threshold `cyclomatic.double() > agg('cyclomatic','p90','not_empty')`.
    pops: Arc<crate::registry::Populations>,
    /// Memoized `agg(key, reducer, population)` results. The value is identical
    /// for every node in a run (the population is the whole project), so each
    /// distinct call is reduced (sorted) once, not once per file.
    agg_cache: AggCache,
}

/// Cache keyed by `(metric, reducer, population)` → the reduced scalar.
type AggCache = Arc<std::sync::Mutex<HashMap<(String, String, String), f64>>>;

impl GraphView {
    /// Build the view from a fully-enriched level (nodes carry their `path`
    /// attribute and the edges are final).
    pub fn build(level: &LevelGraph) -> Self {
        let mut label: HashMap<String, String> = HashMap::new();
        let mut files: Vec<String> = Vec::new();
        let mut by_dir: HashMap<String, Vec<String>> = HashMap::new();
        let mut rows: Vec<BTreeMap<String, f64>> = Vec::new();
        let mut metric_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for n in &level.nodes {
            let l = label_of(n);
            label.insert(n.id.clone(), l.clone());
            if n.kind != EXTERNAL {
                files.push(l.clone());
                by_dir.entry(split_path(&l).dir).or_default().push(l);
                let row = numeric_attrs(n);
                metric_keys.extend(row.keys().cloned());
                rows.push(row);
            }
        }
        sort_dedup(&mut files);
        for v in by_dir.values_mut() {
            sort_dedup(v);
        }
        let files_set: HashSet<String> = files.iter().cloned().collect();

        // Aggregate populations over internal nodes, using each metric's declared
        // `omit_at` floor (from the level specs) so `not_empty` matches the metric
        // engine's semantics.
        let keys: Vec<String> = metric_keys.into_iter().collect();
        let omit_at: BTreeMap<String, f64> = keys
            .iter()
            .map(|k| {
                let floor = level
                    .node_attributes
                    .get(k)
                    .map(|s| s.omit_at)
                    .unwrap_or(0.0);
                (k.clone(), floor)
            })
            .collect();
        let pops = crate::registry::Populations::build(&rows, &keys, &omit_at);

        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let mut inc: HashMap<String, Vec<String>> = HashMap::new();
        let resolve = |id: &str| label.get(id).cloned().unwrap_or_else(|| id.to_string());
        for e in &level.edges {
            out.entry(e.source.clone())
                .or_default()
                .push(resolve(&e.target));
            inc.entry(e.target.clone())
                .or_default()
                .push(resolve(&e.source));
        }
        for v in out.values_mut() {
            sort_dedup(v);
        }
        for v in inc.values_mut() {
            sort_dedup(v);
        }

        GraphView {
            out,
            inc,
            files: Arc::new(files),
            files_set: Arc::new(files_set),
            by_dir,
            pops: Arc::new(pops),
            agg_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Register the memoizing `agg(key, reducer, population)` host function on
    /// `ctx`, sharing this view's populations + cache.
    fn register_agg(&self, ctx: &mut Context) {
        let pops = self.pops.clone();
        let cache = self.agg_cache.clone();
        ctx.add_function(
            "agg",
            move |key: Arc<String>, reducer: Arc<String>, population: Arc<String>| -> f64 {
                let k = (
                    key.as_str().to_string(),
                    reducer.as_str().to_string(),
                    population.as_str().to_string(),
                );
                if let Some(v) = cache.lock().unwrap().get(&k) {
                    return *v;
                }
                let v = pops.reduce_for(&key, &reducer, &population);
                cache.lock().unwrap().insert(k, v);
                v
            },
        );
    }

    fn deps(&self, id: &str) -> Vec<String> {
        self.out.get(id).cloned().unwrap_or_default()
    }

    fn rdeps(&self, id: &str) -> Vec<String> {
        self.inc.get(id).cloned().unwrap_or_default()
    }

    fn files_vec(&self) -> Vec<String> {
        (*self.files).clone()
    }

    fn files_set_arc(&self) -> Arc<HashSet<String>> {
        self.files_set.clone()
    }

    /// Files in the same folder as `path`, excluding `path` itself.
    fn siblings(&self, path: &str) -> Vec<String> {
        let dir = split_path(path).dir;
        self.by_dir
            .get(&dir)
            .map(|v| v.iter().filter(|f| f.as_str() != path).cloned().collect())
            .unwrap_or_default()
    }
}

/// A node's numeric attributes as a name→f64 map (for the aggregate populations).
fn numeric_attrs(node: &Node) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    for (k, v) in node.attrs.iter() {
        match v {
            AttrValue::Int(i) => {
                m.insert(k.clone(), *i as f64);
            }
            AttrValue::Float(f) => {
                m.insert(k.clone(), *f);
            }
            _ => {}
        }
    }
    m
}

fn sort_dedup(v: &mut Vec<String>) {
    v.sort();
    v.dedup();
}

/// A node's label for dependency matching. An **external** crate keeps its
/// `ext:<name>` id (its `path` attribute is the crate's cargo-registry location,
/// useless for a `depends_on("ext:sqlx")` predicate). An internal file uses the
/// same repo-relative string [`node_path`] resolves, so the adjacency / folder
/// index and a node's own `path`/`dir` always agree.
fn label_of(node: &Node) -> String {
    if node.kind == EXTERNAL || node.id.starts_with("ext:") {
        return node.id.clone();
    }
    node_path(node)
}

/// Expand `[rules.defs]` named helpers into `expr` by whole-word substitution,
/// to a fixpoint (a helper may reference earlier helpers). Each helper body is
/// wrapped in parentheses so it composes with surrounding operators. A helper
/// set that never settles (a reference cycle) is a compile error rather than an
/// infinite loop.
fn expand_defs(
    id: &str,
    expr: &str,
    defs: &BTreeMap<String, String>,
) -> Result<String, CheckCompileError> {
    let mut out = expr.to_string();
    // A non-cyclic set settles within `defs.len()` passes; one extra pass detects
    // a cycle (the (defs.len()+1)-th pass would still be changing the string).
    for _ in 0..=defs.len() {
        let mut changed = false;
        for (name, body) in defs {
            if references(&out, name) {
                out = replace_word(&out, name, &format!("({body})"));
                changed = true;
            }
        }
        if !changed {
            return Ok(out);
        }
    }
    Err(CheckCompileError {
        id: id.to_string(),
        message: "`[rules.defs]` helpers reference each other in a cycle".to_string(),
    })
}

/// Whole-word membership: does `haystack` reference identifier `word` (not as a
/// substring of a larger identifier)? Mirrors the metric registry's scan.
fn references(haystack: &str, word: &str) -> bool {
    word_positions(haystack, word).next().is_some()
}

/// Replace every whole-word occurrence of `word` in `s` with `repl` (UTF-8 safe;
/// only ASCII-identifier boundaries are considered, so string-literal contents
/// are copied through unchanged).
fn replace_word(s: &str, word: &str, repl: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for start in word_positions(s, word) {
        out.push_str(&s[last..start]);
        out.push_str(repl);
        last = start + word.len();
    }
    out.push_str(&s[last..]);
    out
}

/// Byte offsets of every whole-word occurrence of `word` in `s`.
fn word_positions<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = s.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut from = 0;
    std::iter::from_fn(move || {
        while let Some(rel) = s[from..].find(word) {
            let start = from + rel;
            let end = start + word.len();
            from = start + 1;
            let before_ok = start == 0 || !is_word(bytes[start - 1]);
            let after_ok = end == bytes.len() || !is_word(bytes[end]);
            if before_ok && after_ok {
                return Some(start);
            }
        }
        None
    })
}

/// Fill `{key}` placeholders in a message from the node's values. `{` / `}` are
/// ASCII, so byte offsets from `find` stay on char boundaries. An unmatched `{`
/// or an unknown key is left verbatim.
fn render_message(template: &str, node: &Node) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                match lookup_value(node, key) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolve a `{key}` for message interpolation — a derived path field or any
/// node attribute, formatted as a human string.
fn lookup_value(node: &Node, key: &str) -> Option<String> {
    match key {
        "path" => Some(node_path(node)),
        "name" | "stem" | "ext" | "dir" => {
            let parts = split_path(&node_path(node));
            Some(match key {
                "name" => parts.name,
                "stem" => parts.stem,
                "ext" => parts.ext,
                _ => parts.dir,
            })
        }
        _ => node.attrs.get(key).map(format_attr),
    }
}

/// Human form of an attribute value: integers and whole floats print without a
/// decimal point; fractional floats keep two places.
fn format_attr(value: &AttrValue) -> String {
    match value {
        AttrValue::Int(i) => i.to_string(),
        AttrValue::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
        AttrValue::Float(f) => format!("{f:.2}"),
        AttrValue::Bool(b) => b.to_string(),
        AttrValue::Str(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
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
                message: "mi={mi} whole={whole} flag={flag} ext=[{ext}] dir=[{dir}] lang={lang} dangling={".into(),
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
}
