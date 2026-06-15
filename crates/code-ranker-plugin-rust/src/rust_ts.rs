//! Rust metric engine on `tree-sitter-rust`, a faithful in-tree port of
//! `rust-code-analysis`'s node-kind classification (the algorithm of record).
//! Lives in the Rust plugin; produces a [`code_ranker_graph::FileMetrics`].
//!
//! It counts over the raw tree-sitter node tree with the same node-kind rules rca
//! uses, so it yields the same numbers. Node kinds are resolved **by name** at
//! startup (not hard-coded `u16`s), so a grammar bump that renumbers kinds does
//! not silently break us. Correctness is guarded by the e2e goldens and the
//! layer-3 anchors in the tests module.
//!
//! `dead_code` is allowed because a few computed intermediates (`functions`,
//! η1/η2) are not emitted as metric keys.
#![allow(dead_code)]

use code_ranker_graph::FileMetrics;
use tree_sitter::{Node, Parser};

/// Node-kind ids we key on, resolved by name from the grammar once.
struct Kinds {
    // space-creating nodes (each contributes a McCabe base path of 1)
    function_item: u16,
    impl_item: u16,
    trait_item: u16,
    closure_expression: u16,
    // cyclomatic branch nodes
    if_expression: u16,
    for_expression: u16,
    while_expression: u16,
    loop_expression: u16,
    match_arm: u16,
    try_expression: u16,
    amp_amp: u16,
    pipe_pipe: u16,
    // exits
    return_expression: u16,
    // nargs: punctuation/attribute children of a `parameters` node that don't count
    lparen: u16,
    rparen: u16,
    comma: u16,
    pipe: u16,
    attribute_item: u16,
    // cognitive
    else_clause: u16,
    match_expression: u16,
    binary_expression: u16,
    unary_expression: u16,
    break_expression: u16,
    continue_expression: u16,
    label: u16,
}

impl Kinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        let anon = |n: &str| lang.id_for_node_kind(n, false);
        Kinds {
            function_item: named("function_item"),
            impl_item: named("impl_item"),
            trait_item: named("trait_item"),
            closure_expression: named("closure_expression"),
            if_expression: named("if_expression"),
            for_expression: named("for_expression"),
            while_expression: named("while_expression"),
            loop_expression: named("loop_expression"),
            match_arm: named("match_arm"),
            try_expression: named("try_expression"),
            amp_amp: anon("&&"),
            pipe_pipe: anon("||"),
            return_expression: named("return_expression"),
            lparen: anon("("),
            rparen: anon(")"),
            comma: anon(","),
            pipe: anon("|"),
            attribute_item: named("attribute_item"),
            else_clause: named("else_clause"),
            match_expression: named("match_expression"),
            binary_expression: named("binary_expression"),
            unary_expression: named("unary_expression"),
            break_expression: named("break_expression"),
            continue_expression: named("continue_expression"),
            label: named("label"),
        }
    }

    fn is_non_arg(&self, id: u16) -> bool {
        id == self.lparen
            || id == self.rparen
            || id == self.comma
            || id == self.pipe
            || id == self.attribute_item
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

/// Parse `src` (already test-stripped) with tree-sitter-rust and compute metrics.
pub fn compute(src: &[u8]) -> Option<FileMetrics> {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;
    let tree = parser.parse(src, None)?;
    let k = Kinds::resolve(&lang);

    let mut c = Counts {
        spaces: 1, // the source_file (unit) space
        ..Default::default()
    };
    walk(tree.root_node(), &k, &mut c);

    let mut cog = CogState::default();
    cog_walk(tree.root_node(), 0, 0, 0, &k, &mut cog);

    let loc = compute_loc(tree.root_node(), &lang);
    let h = compute_halstead(tree.root_node(), src, &lang);

    let cyclomatic = (c.spaces + c.branches) as f64;
    let cloc = (loc.only_comment + loc.code_comment) as f64;
    // rca's MI uses the unit SPAN sloc (end − start), not ploc.
    let span_sloc = tree
        .root_node()
        .end_position()
        .row
        .saturating_sub(tree.root_node().start_position().row) as f64;

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
        tloc: 0.0, // set by the caller from strip_cfg_test's removed-line count
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
    let comment_ratio = cloc / sloc;
    171.0 - 5.2 * volume.log2() - 0.23 * cyclomatic - 16.2 * sloc.log2()
        + 50.0 * (comment_ratio * 2.4).sqrt().sin()
}

/// Halstead counts (η₁/η₂/N₁/N₂) and the derived metrics.
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

/// Halstead node kinds, replicating rca's Rust `get_op_type`.
struct HalKinds {
    operators: Vec<u16>, // unconditional operators
    operands: Vec<u16>,
    pipe_pipe: u16,
    slash: u16,
    bang: u16,
    binary_expression: u16,
    inner_doc_comment_marker: u16,
}

impl HalKinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        let anon = |n: &str| lang.id_for_node_kind(n, false);
        HalKinds {
            operators: [
                "(", "{", "[", "=>", "+", "*", "=", ",", "->", "?", "<", ">", "&", "..", "..=",
                "-", "&&", "|", "^", "==", "!=", "<=", ">=", "<<", ">>", "%", "+=", "-=", "*=",
                "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ".", ";", "async", "await", "continue",
                "for", "if", "let", "loop", "match", "return", "unsafe", "while", "move", "fn",
            ]
            .iter()
            .map(|s| anon(s))
            .chain([named("mutable_specifier"), named("primitive_type")])
            .collect(),
            operands: vec![
                named("identifier"),
                named("string_literal"),
                named("raw_string_literal"),
                named("integer_literal"),
                named("float_literal"),
                named("boolean_literal"),
                named("self"),
                named("char_literal"),
                anon("_"),
            ],
            pipe_pipe: anon("||"),
            slash: anon("/"),
            bang: anon("!"),
            binary_expression: named("binary_expression"),
            inner_doc_comment_marker: named("inner_doc_comment_marker"),
        }
    }
}

fn compute_halstead(root: Node, src: &[u8], lang: &tree_sitter::Language) -> Halstead {
    use std::collections::HashMap;
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
    operators: &mut std::collections::HashMap<u16, u64>,
    operands: &mut std::collections::HashMap<Vec<u8>, u64>,
) {
    let id = node.kind_id();
    let is_operator = if id == hk.pipe_pipe || id == hk.slash {
        node.parent()
            .is_some_and(|p| p.kind_id() == hk.binary_expression)
    } else if id == hk.bang {
        node.parent()
            .is_none_or(|p| p.kind_id() != hk.inner_doc_comment_marker)
    } else {
        hk.operators.contains(&id)
    };
    if is_operator {
        *operators.entry(id).or_insert(0) += 1;
    } else if hk.operands.contains(&id) {
        let text = node.utf8_text(src).unwrap_or("").as_bytes().to_vec();
        *operands.entry(text).or_insert(0) += 1;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        hal_walk(child, src, hk, operators, operands);
    }
}

/// LOC node kinds (separate resolver to keep `Kinds` focused).
struct LocKinds {
    noop: Vec<u16>, // string/raw-string/block/source_file + comment markers + `/ // /* */ !`
    comments: [u16; 2], // line_comment, block_comment
    line_comment: u16,
    doc_comment: u16,
    statements: Vec<u16>,
}

impl LocKinds {
    fn resolve(lang: &tree_sitter::Language) -> Self {
        let named = |n: &str| lang.id_for_node_kind(n, true);
        let anon = |n: &str| lang.id_for_node_kind(n, false);
        let line_comment = named("line_comment");
        let block_comment = named("block_comment");
        LocKinds {
            noop: vec![
                named("string_literal"),
                named("raw_string_literal"),
                named("block"),
                named("source_file"),
                named("doc_comment"),
                named("inner_doc_comment_marker"),
                named("outer_doc_comment_marker"),
                anon("/"),
                anon("//"),
                anon("/*"),
                anon("*/"),
                anon("!"),
            ],
            comments: [line_comment, block_comment],
            line_comment,
            doc_comment: named("doc_comment"),
            statements: vec![
                named("expression_statement"),
                named("let_declaration"),
                named("assignment_expression"),
                named("compound_assignment_expr"),
                named("empty_statement"),
            ],
        }
    }
}

#[derive(Default)]
struct LocState {
    ploc: usize, // filled from lines.len() at the end
    lines: std::collections::HashSet<usize>,
    only_comment: i64,
    code_comment: i64,
    comment_line_end: Option<usize>,
    lloc: u32,
    blank: i64,
}

/// Faithful port of rca's Rust `Loc::compute`: preorder over ALL nodes
/// (named + anonymous). Code-bearing tokens insert their start row into `lines`
/// (→ ploc); comments accumulate cloc with rca's same/independent-line logic;
/// statement nodes count lloc. blank = (root span) − ploc − only_comment_lines.
fn compute_loc(root: Node, lang: &tree_sitter::Language) -> LocState {
    let lk = LocKinds::resolve(lang);
    let mut st = LocState::default();
    loc_walk(root, &lk, &mut st);
    st.ploc = st.lines.len();
    // sloc span of the unit (source_file): rca uses end - start for the unit.
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
        // no LOC contribution
    } else if id == lk.comments[0] || id == lk.comments[1] {
        // line_comment with a DocComment child: the doc comment includes the
        // trailing newline, so exclude the last line (rca's adjustment).
        let end = if id == lk.line_comment && has_child_kind(node, lk.doc_comment) {
            end.saturating_sub(1)
        } else {
            end
        };
        add_cloc_lines(st, start, end);
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

fn has_child_kind(node: Node, kind: u16) -> bool {
    if kind == 0 {
        return false;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind_id() == kind)
}

fn add_cloc_lines(st: &mut LocState, start: usize, end: usize) {
    let comment_diff = end - start;
    let after_code = st.lines.contains(&start);
    if after_code && comment_diff == 0 {
        st.code_comment += 1;
    } else if after_code && comment_diff > 0 {
        st.code_comment += 1;
        st.only_comment += comment_diff as i64;
    } else {
        st.only_comment += (comment_diff + 1) as i64;
        st.comment_line_end = Some(end);
    }
}

fn check_comment_ends_on_code_line(st: &mut LocState, start_code_line: usize) {
    if let Some(end) = st.comment_line_end
        && end == start_code_line
        && !st.lines.contains(&start_code_line)
    {
        st.only_comment -= 1;
        st.code_comment += 1;
    }
}

/// Cognitive accumulator. `structural` is the running total across all spaces
/// (cognitive_sum sums per-space, so a single global accumulator is equivalent).
/// `boolean_op` tracks the current boolean run; per rca it is set once (first op)
/// and reset at branches / space boundaries.
#[derive(Default)]
struct CogState {
    structural: u32,
    boolean_op: Option<u16>,
}

impl CogState {
    /// rca's `eval_based_on_prev`: the FIRST operator in a run sets `boolean_op`
    /// and increments; a later DIFFERENT operator increments but leaves
    /// `boolean_op` unchanged; the same operator does not increment.
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

/// Faithful port of rca's Rust cognitive `compute` + the spaces walk: a preorder
/// DFS carrying `(nesting, depth, lambda)` top-down, accumulating `structural`,
/// with `boolean_op` reset at branches and saved/restored across spaces.
fn cog_walk(node: Node, nesting: u32, depth: u32, lambda: u32, k: &Kinds, st: &mut CogState) {
    let id = node.kind_id();
    let (mut cn, mut cd, cl) = (nesting, depth, lambda);

    if id == k.if_expression {
        if !is_else_if(node, k) {
            st.structural += nesting + depth + lambda + 1; // increase_nesting
            cn = nesting + 1;
            st.boolean_op = None;
        }
    } else if id == k.for_expression || id == k.while_expression || id == k.match_expression {
        st.structural += nesting + depth + lambda + 1;
        cn = nesting + 1;
        st.boolean_op = None;
    } else if id == k.else_clause {
        st.structural += 1; // covers plain `else` and `else if`
    } else if id == k.break_expression || id == k.continue_expression {
        if let Some(lbl) = node.child(1)
            && lbl.kind_id() == k.label
        {
            st.structural += 1;
        }
    } else if id == k.unary_expression {
        st.boolean_op = Some(id); // not_operator
    } else if id == k.binary_expression {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let cid = child.kind_id();
            if cid == k.amp_amp || cid == k.pipe_pipe {
                st.eval_boolean(cid);
            }
        }
    } else if id == k.function_item {
        cn = 0;
        if has_ancestor_function_item(node, k) {
            cd = depth + 1;
        }
    } else if id == k.closure_expression {
        // cl handled below (lambda + 1 for children)
    }

    // function_item/impl_item/trait_item/closure_expression are new spaces → a
    // fresh boolean run; save and restore so the parent space resumes its own.
    let is_space = id == k.function_item
        || id == k.impl_item
        || id == k.trait_item
        || id == k.closure_expression;
    let child_lambda = if id == k.closure_expression {
        cl + 1
    } else {
        cl
    };

    let mut cursor = node.walk();
    if is_space {
        let saved = st.boolean_op;
        st.boolean_op = None;
        for child in node.children(&mut cursor) {
            cog_walk(child, cn, cd, child_lambda, k, st);
        }
        st.boolean_op = saved;
    } else {
        for child in node.children(&mut cursor) {
            cog_walk(child, cn, cd, child_lambda, k, st);
        }
    }
}

fn is_else_if(node: Node, k: &Kinds) -> bool {
    node.parent().is_some_and(|p| p.kind_id() == k.else_clause)
}

fn has_ancestor_function_item(node: Node, k: &Kinds) -> bool {
    let mut cur = node;
    while let Some(p) = cur.parent() {
        if p.kind_id() == k.function_item {
            return true;
        }
        cur = p;
    }
    false
}

/// Count the real parameters of a fn/closure: direct children of its `parameters`
/// field that are not punctuation / an attribute (rca's `is_non_arg`).
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

    if id == k.function_item {
        c.spaces += 1;
        c.functions += 1;
        c.args += count_args(node, k);
        // a value-returning exit when the fn declares a return type (`-> T`)
        if node.child_by_field_name("return_type").is_some() {
            c.exits += 1;
        }
    } else if id == k.closure_expression {
        c.spaces += 1;
        c.closures += 1;
        c.args += count_args(node, k);
    } else if id == k.impl_item || id == k.trait_item {
        c.spaces += 1;
    }

    if id == k.if_expression
        || id == k.for_expression
        || id == k.while_expression
        || id == k.loop_expression
        || id == k.match_arm
        || id == k.try_expression
        || id == k.amp_amp
        || id == k.pipe_pipe
    {
        c.branches += 1;
    }
    if id == k.return_expression || id == k.try_expression {
        c.exits += 1;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, k, c);
    }
}
