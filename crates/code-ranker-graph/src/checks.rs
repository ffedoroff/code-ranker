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
#[path = "checks_test.rs"]
mod tests;
