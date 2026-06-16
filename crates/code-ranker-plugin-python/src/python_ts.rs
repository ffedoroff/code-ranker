//! Python metric engine on `tree-sitter-python`, a faithful port of
//! `rust-code-analysis`'s node-kind classification (the algorithm of record).
//! Lives in the Python plugin; produces a [`code_ranker_graph::MetricInputs`].
//! Node kinds are resolved by name. Correctness is guarded by the e2e Python
//! golden and the layer-1/2/3 tests in `lib.rs`.
#![allow(dead_code)]

use code_ranker_graph::{FunctionUnit, MetricInputs};
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

pub fn compute(src: &[u8]) -> Option<MetricInputs> {
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();

    let k = Kinds::resolve(&lang);
    let mut c = Counts {
        spaces: 1,
        ..Default::default()
    };
    walk(root, &k, &mut c);

    let mut cog = CogState::default();
    cog_walk(root, 0, 0, 0, &k, &mut cog);

    let loc = compute_loc(root, &lang);
    let h = compute_halstead(root, src, &lang);

    let cloc = (loc.only_comment + loc.code_comment) as f64;
    let span_sloc = root
        .end_position()
        .row
        .saturating_sub(root.start_position().row) as f64;

    // tier-1 counts; tier-2 is derived downstream by the registry engine.
    Some(MetricInputs {
        eta1: h.eta1,
        eta2: h.eta2,
        n1: h.n1,
        n2: h.n2,
        spaces: c.spaces as f64,
        branches: c.branches as f64,
        cognitive: cog.structural as f64,
        exits: c.exits as f64,
        args: c.args as f64,
        closures: c.closures as f64,
        sloc: loc.ploc as f64,
        lloc: loc.lloc as f64,
        cloc,
        blank: loc.blank as f64,
        tloc: 0.0, // Python has no inline-test stripping
        span_sloc,
    })
}

/// Per-function metric units (function-level metrics): run the same tier-1
/// counters over each `function_definition` subtree, then the shared tier-2
/// derivation. The file-level [`compute`] is untouched, so default output is
/// unchanged; this only runs when the `functions` level is requested.
///
/// A unit's `spaces` starts at 0 because [`walk`] counts the `function_definition`
/// node itself (+1), giving the McCabe base path of 1. Metrics include nested
/// closures/functions (like the file includes everything); each nested function
/// also gets its own unit.
pub fn compute_functions(src: &[u8]) -> Vec<FunctionUnit> {
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Vec::new();
    };
    let k = Kinds::resolve(&lang);
    let mut units = Vec::new();
    collect_functions(tree.root_node(), &k, src, &lang, &mut units);
    units
}

fn collect_functions(
    node: Node,
    k: &Kinds,
    src: &[u8],
    lang: &tree_sitter::Language,
    out: &mut Vec<FunctionUnit>,
) {
    if node.kind_id() == k.function_definition {
        out.push(unit_for(node, k, src, lang));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(child, k, src, lang, out);
    }
}

fn unit_for(fnode: Node, k: &Kinds, src: &[u8], lang: &tree_sitter::Language) -> FunctionUnit {
    let mut c = Counts::default(); // spaces:0 — walk(fnode) counts fnode itself
    walk(fnode, k, &mut c);
    let mut cog = CogState::default();
    cog_walk(fnode, 0, 0, 0, k, &mut cog);
    let loc = compute_loc(fnode, lang);
    let h = compute_halstead(fnode, src, lang);
    let cloc = (loc.only_comment + loc.code_comment) as f64;
    let span_sloc = fnode
        .end_position()
        .row
        .saturating_sub(fnode.start_position().row) as f64;
    let inputs = MetricInputs {
        eta1: h.eta1,
        eta2: h.eta2,
        n1: h.n1,
        n2: h.n2,
        spaces: c.spaces as f64,
        branches: c.branches as f64,
        cognitive: cog.structural as f64,
        exits: c.exits as f64,
        args: c.args as f64,
        closures: c.closures as f64,
        sloc: loc.ploc as f64,
        lloc: loc.lloc as f64,
        cloc,
        blank: loc.blank as f64,
        tloc: 0.0,
        span_sloc,
    };
    let name = fnode
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .unwrap_or("<anonymous>")
        .to_string();
    FunctionUnit {
        kind: fn_kind(fnode, k).to_string(),
        name,
        start_line: fnode.start_position().row as u32 + 1,
        end_line: fnode.end_position().row as u32 + 1,
        inputs,
    }
}

/// `method` when the nearest enclosing scope is a class, else `function`.
fn fn_kind(node: Node, k: &Kinds) -> &'static str {
    let mut p = node.parent();
    while let Some(n) = p {
        if n.kind_id() == k.class_definition {
            return "method";
        }
        if n.kind_id() == k.function_definition {
            return "function";
        }
        p = n.parent();
    }
    "function"
}

// ── structural (cyclomatic / exits / nargs / nom) ───────────────────────────

struct Kinds {
    function_definition: u16,
    class_definition: u16,
    lambda: u16,
    return_statement: u16,
    // cyclomatic branch tokens (anonymous keywords/operators)
    cyc: Vec<u16>,
    kw_else: u16,
    else_clause: u16,
    for_statement: u16,
    while_statement: u16,
    // nargs punctuation
    lparen: u16,
    rparen: u16,
    comma: u16,
    // cognitive
    if_statement: u16,
    while_statement2: u16,
    conditional_expression: u16,
    elif_clause: u16,
    finally_clause: u16,
    except_clause: u16,
    expression_statement: u16,
    expression_list: u16,
    tuple: u16,
    not_operator: u16,
    boolean_operator: u16,
    kw_and: u16,
    kw_or: u16,
}

impl Kinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        let anon = |n: &str| lang.id_for_node_kind(n, false);
        Kinds {
            function_definition: named("function_definition"),
            class_definition: named("class_definition"),
            lambda: named("lambda"),
            return_statement: named("return_statement"),
            cyc: [
                "if", "elif", "for", "while", "except", "with", "assert", "and", "or",
            ]
            .iter()
            .map(|s| anon(s))
            .collect(),
            kw_else: anon("else"),
            else_clause: named("else_clause"),
            for_statement: named("for_statement"),
            while_statement: named("while_statement"),
            lparen: anon("("),
            rparen: anon(")"),
            comma: anon(","),
            if_statement: named("if_statement"),
            while_statement2: named("while_statement"),
            conditional_expression: named("conditional_expression"),
            elif_clause: named("elif_clause"),
            finally_clause: named("finally_clause"),
            except_clause: named("except_clause"),
            expression_statement: named("expression_statement"),
            expression_list: named("expression_list"),
            tuple: named("tuple"),
            not_operator: named("not_operator"),
            boolean_operator: named("boolean_operator"),
            kw_and: anon("and"),
            kw_or: anon("or"),
        }
    }
    fn is_non_arg(&self, id: u16) -> bool {
        id == self.lparen || id == self.rparen || id == self.comma
    }
}

#[derive(Default)]
struct Counts {
    spaces: u32,
    branches: u32,
    exits: u32,
    args: u32,
    closures: u32,
    functions: u32,
}

fn count_args(node: Node, k: &Kinds) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut cursor = params.walk();
    params
        .children(&mut cursor)
        .filter(|c| !k.is_non_arg(c.kind_id()))
        .count() as u32
}

fn walk(node: Node, k: &Kinds, c: &mut Counts) {
    let id = node.kind_id();
    if id == k.function_definition {
        c.spaces += 1;
        c.functions += 1;
        c.args += count_args(node, k);
    } else if id == k.lambda {
        c.closures += 1;
        c.args += count_args(node, k);
    } else if id == k.class_definition {
        c.spaces += 1;
    }
    if k.cyc.contains(&id) {
        c.branches += 1;
    }
    if id == k.kw_else {
        // else attached to a for/while loop counts (not an if's else).
        if let Some(clause) = node.parent()
            && clause.kind_id() == k.else_clause
            && let Some(stmt) = clause.parent()
            && (stmt.kind_id() == k.for_statement || stmt.kind_id() == k.while_statement)
        {
            c.branches += 1;
        }
    }
    if id == k.return_statement {
        c.exits += 1;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, k, c);
    }
}

// ── cognitive ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct CogState {
    structural: u32,
    boolean_op: Option<u16>,
}
impl CogState {
    fn eval_boolean(&mut self, op: u16) {
        match self.boolean_op {
            Some(prev) => {
                if prev != op {
                    self.structural += 1;
                }
            }
            None => {
                self.boolean_op = Some(op);
                self.structural += 1;
            }
        }
    }
}

fn cog_walk(node: Node, nesting: u32, depth: u32, lambda: u32, k: &Kinds, st: &mut CogState) {
    let id = node.kind_id();
    let (mut cn, mut cd, mut cl) = (nesting, depth, lambda);

    if id == k.if_statement
        || id == k.for_statement
        || id == k.while_statement2
        || id == k.conditional_expression
    {
        st.structural += nesting + depth + lambda + 1;
        cn = nesting + 1;
        st.boolean_op = None;
    } else if id == k.elif_clause {
        st.structural += 1;
        st.boolean_op = None;
    } else if id == k.else_clause || id == k.finally_clause {
        st.structural += 1;
    } else if id == k.except_clause {
        cn = nesting + 1;
        st.structural += cn + depth + lambda + 1; // rca: nesting+=1; increment (uses new nesting)
    } else if id == k.expression_statement || id == k.expression_list || id == k.tuple {
        st.boolean_op = None;
    } else if id == k.not_operator {
        st.boolean_op = Some(id);
    } else if id == k.boolean_operator {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let cid = child.kind_id();
            if cid == k.kw_and || cid == k.kw_or {
                st.eval_boolean(cid);
            }
        }
    } else if id == k.lambda {
        cl = lambda + 1;
    } else if id == k.function_definition && has_ancestor(node, k.function_definition) {
        cd = depth + 1;
    }

    let is_space = id == k.function_definition || id == k.class_definition || id == k.lambda;
    let mut cursor = node.walk();
    if is_space {
        let saved = st.boolean_op;
        st.boolean_op = None;
        for child in node.children(&mut cursor) {
            cog_walk(child, cn, cd, cl, k, st);
        }
        st.boolean_op = saved;
    } else {
        for child in node.children(&mut cursor) {
            cog_walk(child, cn, cd, cl, k, st);
        }
    }
}

fn has_ancestor(node: Node, kind: u16) -> bool {
    let mut cur = node;
    while let Some(p) = cur.parent() {
        if p.kind_id() == kind {
            return true;
        }
        cur = p;
    }
    false
}

// ── LOC ─────────────────────────────────────────────────────────────────────

struct LocKinds {
    noop: Vec<u16>,
    comment: u16,
    string: u16,
    expression_statement: u16,
    statements: Vec<u16>,
}
impl LocKinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        LocKinds {
            noop: vec![
                named("string_start"),
                named("string_end"),
                named("string_content"),
                named("block"),
                named("module"),
            ],
            comment: named("comment"),
            string: named("string"),
            expression_statement: named("expression_statement"),
            statements: [
                "statement",
                "simple_statements",
                "import_statement",
                "future_import_statement",
                "import_from_statement",
                "print_statement",
                "assert_statement",
                "return_statement",
                "delete_statement",
                "raise_statement",
                "pass_statement",
                "break_statement",
                "continue_statement",
                "if_statement",
                "for_statement",
                "while_statement",
                "try_statement",
                "with_statement",
                "global_statement",
                "nonlocal_statement",
                "exec_statement",
                "expression_statement",
            ]
            .iter()
            .map(|s| named(s))
            .collect(),
        }
    }
}

#[derive(Default)]
struct LocState {
    ploc: usize,
    lines: std::collections::HashSet<usize>,
    only_comment: i64,
    code_comment: i64,
    comment_line_end: Option<usize>,
    lloc: u32,
    blank: i64,
}

fn compute_loc(root: Node, lang: &tree_sitter::Language) -> LocState {
    let lk = LocKinds::resolve(lang);
    let mut st = LocState::default();
    loc_walk(root, &lk, &mut st);
    st.ploc = st.lines.len();
    let span = root
        .end_position()
        .row
        .saturating_sub(root.start_position().row) as i64;
    st.blank = span - st.ploc as i64 - st.only_comment;
    st
}

fn loc_walk(node: Node, lk: &LocKinds, st: &mut LocState) {
    let id = node.kind_id();
    let start = node.start_position().row;
    let end = node.end_position().row;

    if lk.noop.contains(&id) {
        // nothing
    } else if id == lk.comment {
        add_cloc_lines(st, start, end);
    } else if id == lk.string {
        // A bare docstring (sole child of an expression_statement) is a comment;
        // otherwise a string spanning past its parent's start line is code.
        if let Some(parent) = node.parent() {
            if parent.kind_id() == lk.expression_statement {
                add_cloc_lines(st, start, end);
            } else if parent.start_position().row != start {
                check_comment_ends_on_code_line(st, start);
                st.lines.insert(start);
            }
        }
    } else if lk.statements.contains(&id) {
        st.lloc += 1;
    } else {
        check_comment_ends_on_code_line(st, start);
        st.lines.insert(start);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        loc_walk(child, lk, st);
    }
}

fn add_cloc_lines(st: &mut LocState, start: usize, end: usize) {
    let diff = end - start;
    let after_code = st.lines.contains(&start);
    if after_code && diff == 0 {
        st.code_comment += 1;
    } else if after_code && diff > 0 {
        st.code_comment += 1;
        st.only_comment += diff as i64;
    } else {
        st.only_comment += (diff + 1) as i64;
        st.comment_line_end = Some(end);
    }
}
fn check_comment_ends_on_code_line(st: &mut LocState, start: usize) {
    if let Some(end) = st.comment_line_end
        && end == start
        && !st.lines.contains(&start)
    {
        st.only_comment -= 1;
        st.code_comment += 1;
    }
}

// ── Halstead ──────────────────────────────────────────────────────────────

struct Halstead {
    eta1: f64,
    eta2: f64,
    n1: f64,
    n2: f64,
}

struct HalKinds {
    operators: Vec<u16>,
    operands: Vec<u16>, // identifier/integer/float/true/false/none
    string: u16,
    expression_statement: u16,
}
impl HalKinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        let anon = |n: &str| lang.id_for_node_kind(n, false);
        HalKinds {
            operators: [
                "import", ".", "from", ",", "as", "*", ">>", "assert", ":=", "return", "def",
                "del", "raise", "pass", "break", "continue", "if", "elif", "else", "async", "for",
                "in", "while", "try", "except", "finally", "with", "->", "=", "global", "exec",
                "@", "not", "and", "or", "+", "-", "/", "%", "//", "**", "|", "&", "^", "<<", "~",
                "<", "<=", "==", "!=", ">=", ">", "<>", "is", "+=", "-=", "*=", "/=", "@=", "//=",
                "%=", "**=", ">>=", "<<=", "&=", "^=", "|=", "yield", "await", "print",
            ]
            .iter()
            .map(|s| anon(s))
            .collect(),
            operands: vec![
                named("identifier"),
                named("integer"),
                named("float"),
                named("true"),
                named("false"),
                named("none"),
            ],
            string: named("string"),
            expression_statement: named("expression_statement"),
        }
    }
}

fn compute_halstead(root: Node, src: &[u8], lang: &tree_sitter::Language) -> Halstead {
    let hk = HalKinds::resolve(lang);
    let mut operators: HashMap<u16, u64> = HashMap::new();
    let mut operands: HashMap<Vec<u8>, u64> = HashMap::new();
    hal_walk(root, src, &hk, &mut operators, &mut operands);

    let n1: u64 = operators.values().sum();
    let n2: u64 = operands.values().sum();
    Halstead {
        eta1: operators.len() as f64,
        eta2: operands.len() as f64,
        n1: n1 as f64,
        n2: n2 as f64,
    }
}

fn hal_walk(
    node: Node,
    src: &[u8],
    hk: &HalKinds,
    operators: &mut HashMap<u16, u64>,
    operands: &mut HashMap<Vec<u8>, u64>,
) {
    let id = node.kind_id();
    if hk.operators.contains(&id) {
        *operators.entry(id).or_insert(0) += 1;
    } else if hk.operands.contains(&id) {
        *operands
            .entry(node.utf8_text(src).unwrap_or("").as_bytes().to_vec())
            .or_insert(0) += 1;
    } else if id == hk.string {
        // operand unless it is a bare docstring (sole child of expression_statement)
        let is_docstring = node
            .parent()
            .is_some_and(|p| p.kind_id() == hk.expression_statement && p.child_count() == 1);
        if !is_docstring {
            *operands
                .entry(node.utf8_text(src).unwrap_or("").as_bytes().to_vec())
                .or_insert(0) += 1;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        hal_walk(child, src, hk, operators, operands);
    }
}

#[cfg(test)]
mod fn_tests {
    use super::*;

    /// `compute_functions` finds top-level functions and class methods and counts
    /// branches (covers collect_functions / unit_for / fn_kind).
    #[test]
    fn compute_functions_covers_function_and_method() {
        let src = b"def f(x):\n    if x:\n        return 1\n    return 0\n\nclass C:\n    def m(self, y):\n        return y\n";
        let units = compute_functions(src);
        assert!(
            units.iter().any(|u| u.name == "f" && u.kind == "function"),
            "function f"
        );
        assert!(
            units.iter().any(|u| u.name == "m" && u.kind == "method"),
            "method m"
        );
        let f = units.iter().find(|u| u.name == "f").unwrap();
        assert!(f.inputs.branches >= 1.0, "f has an `if` branch");
    }

    #[test]
    fn compute_functions_empty_on_no_functions() {
        assert!(compute_functions(b"x = 1\n").is_empty());
    }
}
