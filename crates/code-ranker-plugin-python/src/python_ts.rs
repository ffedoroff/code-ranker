//! Python metric engine on `tree-sitter-python`, a faithful port of
//! `rust-code-analysis`'s node-kind classification (the algorithm of record).
//! Lives in the Python plugin; produces a [`code_ranker_graph::FileMetrics`].
//! Node kinds are resolved by name. Correctness is guarded by the e2e Python
//! golden and the layer-1/2/3 tests in `lib.rs`.
#![allow(dead_code)]

use code_ranker_graph::FileMetrics;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

pub fn compute(src: &[u8]) -> Option<FileMetrics> {
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

    let cyclomatic = (c.spaces + c.branches) as f64;
    let cloc = (loc.only_comment + loc.code_comment) as f64;
    let span_sloc = root
        .end_position()
        .row
        .saturating_sub(root.start_position().row) as f64;

    Some(FileMetrics {
        cyclomatic,
        cognitive: cog.structural as f64,
        exits: c.exits as f64,
        args: c.args as f64,
        closures: c.closures as f64,
        sloc: loc.ploc as f64,
        lloc: loc.lloc as f64,
        cloc,
        blank: loc.blank as f64,
        tloc: 0.0, // Python has no inline-test stripping
        length: h.length,
        vocabulary: h.vocabulary,
        volume: h.volume,
        effort: h.effort,
        time: h.time,
        bugs: h.bugs,
        mi: mi_original(h.volume, cyclomatic, span_sloc),
        mi_sei: mi_sei(h.volume, cyclomatic, span_sloc, cloc),
    })
}

fn mi_original(volume: f64, cyclomatic: f64, sloc: f64) -> f64 {
    171.0 - 5.2 * volume.ln() - 0.23 * cyclomatic - 16.2 * sloc.ln()
}
fn mi_sei(volume: f64, cyclomatic: f64, sloc: f64, cloc: f64) -> f64 {
    let cr = cloc / sloc;
    171.0 - 5.2 * volume.log2() - 0.23 * cyclomatic - 16.2 * sloc.log2()
        + 50.0 * (cr * 2.4).sqrt().sin()
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
    length: f64,
    vocabulary: f64,
    volume: f64,
    effort: f64,
    time: f64,
    bugs: f64,
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

    let eta1 = operators.len() as f64;
    let eta2 = operands.len() as f64;
    let n1: u64 = operators.values().sum();
    let n2: u64 = operands.values().sum();
    let length = (n1 + n2) as f64;
    let vocabulary = eta1 + eta2;
    let volume = if vocabulary > 0.0 {
        length * vocabulary.log2()
    } else {
        0.0
    };
    let (effort, time, bugs) = if eta2 > 0.0 {
        let difficulty = (eta1 / 2.0) * (n2 as f64 / eta2);
        let effort = difficulty * volume;
        (effort, effort / 18.0, effort.powf(2.0 / 3.0) / 3000.0)
    } else {
        (0.0, 0.0, 0.0)
    };
    Halstead {
        eta1,
        eta2,
        length,
        vocabulary,
        volume,
        effort,
        time,
        bugs,
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
