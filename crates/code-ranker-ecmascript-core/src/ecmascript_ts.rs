//! ECMAScript metric engine (JavaScript / TypeScript / TSX) on tree-sitter,
//! replicating `rust-code-analysis`'s shared `js_*` node-kind rules. Lives in
//! `code-ranker-ecmascript-core`; produces a [`code_ranker_graph::FileMetrics`].
//! The grammar is passed in by the caller (js → tree-sitter-javascript, ts/tsx →
//! tree-sitter-typescript). Node kinds are resolved by name; duplicate kinds
//! (rca's `Identifier2`/`String2`/… variants) are all collected, since they share
//! a name but differ by id.
#![allow(dead_code)]

use code_ranker_graph::FileMetrics;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Node, Parser};

/// Collect ALL node-kind ids matching any `(name, is_named)` in `wanted`.
fn id_set(lang: &Language, wanted: &[(&str, bool)]) -> HashSet<u16> {
    let mut out = HashSet::new();
    for id in 0..lang.node_kind_count() as u16 {
        if let Some(name) = lang.node_kind_for_id(id) {
            let named = lang.node_kind_is_named(id);
            if wanted.iter().any(|(n, b)| *n == name && *b == named) {
                out.insert(id);
            }
        }
    }
    out
}

struct Kinds {
    // spaces (is_func_space)
    func_space: HashSet<u16>,
    // func / closure classification
    function_declaration: u16,
    method_definition: u16,
    function_expression: u16,
    arrow_function: u16,
    generator_function: u16,
    generator_function_declaration: u16,
    // ancestor/sibling checks for context-aware classification
    func_assign_anc: HashSet<u16>, // VariableDeclarator|AssignmentExpression|LabeledStatement|Pair
    func_stop: HashSet<u16>,       // StatementBlock|ReturnStatement|NewExpression|Arguments
    arrow_assign_anc: HashSet<u16>,
    arrow_stop: HashSet<u16>,
    identifier: HashSet<u16>,
    property_identifier: HashSet<u16>,
    if_statement: HashSet<u16>,
    else_clause: HashSet<u16>,
    /// rca's `is_else_if` rule differs by language: TypeScript checks the parent is
    /// an `else_clause`; JavaScript and TSX check the parent is an `if_statement`.
    else_if_via_else_clause: bool,
    // cyclomatic
    cyc: HashSet<u16>,
    // exits
    return_statement: HashSet<u16>,
    // nargs
    non_arg: HashSet<u16>, // ( ) ,
    // cognitive
    cog_nest: HashSet<u16>, // For|ForIn|While|Do|Switch|Catch|Ternary statements
    kw_else: HashSet<u16>,
    expression_statement: HashSet<u16>,
    unary_expression: HashSet<u16>,
    binary_expression: HashSet<u16>,
    amp_amp: HashSet<u16>,
    pipe_pipe: HashSet<u16>,
}

impl Kinds {
    fn resolve(lang: &Language, else_if_via_else_clause: bool) -> Self {
        let one = |n: &str, b: bool| {
            (0..lang.node_kind_count() as u16)
                .find(|&id| {
                    lang.node_kind_for_id(id) == Some(n) && lang.node_kind_is_named(id) == b
                })
                .unwrap_or(u16::MAX)
        };
        Kinds {
            func_space: id_set(
                lang,
                &[
                    ("program", true),
                    ("function_expression", true),
                    ("function", true), // some grammar versions name it `function`
                    ("class", true),
                    ("generator_function", true),
                    ("function_declaration", true),
                    ("method_definition", true),
                    ("generator_function_declaration", true),
                    ("class_declaration", true),
                    ("arrow_function", true),
                ],
            ),
            function_declaration: one("function_declaration", true),
            method_definition: one("method_definition", true),
            function_expression: one("function_expression", true),
            arrow_function: one("arrow_function", true),
            generator_function: one("generator_function", true),
            generator_function_declaration: one("generator_function_declaration", true),
            func_assign_anc: id_set(
                lang,
                &[
                    ("variable_declarator", true),
                    ("assignment_expression", true),
                    ("labeled_statement", true),
                    ("pair", true),
                ],
            ),
            func_stop: id_set(
                lang,
                &[
                    ("statement_block", true),
                    ("return_statement", true),
                    ("new_expression", true),
                    ("arguments", true),
                ],
            ),
            arrow_assign_anc: id_set(
                lang,
                &[
                    ("variable_declarator", true),
                    ("assignment_expression", true),
                    ("labeled_statement", true),
                ],
            ),
            arrow_stop: id_set(
                lang,
                &[
                    ("statement_block", true),
                    ("return_statement", true),
                    ("new_expression", true),
                    ("call_expression", true),
                ],
            ),
            identifier: id_set(lang, &[("identifier", true)]),
            property_identifier: id_set(lang, &[("property_identifier", true)]),
            if_statement: id_set(lang, &[("if_statement", true)]),
            else_clause: id_set(lang, &[("else_clause", true)]),
            else_if_via_else_clause,
            cyc: id_set(
                lang,
                &[
                    ("if", false),
                    ("for", false),
                    ("while", false),
                    ("case", false),
                    ("catch", false),
                    ("ternary_expression", true),
                    ("&&", false),
                    ("||", false),
                ],
            ),
            return_statement: id_set(lang, &[("return_statement", true)]),
            non_arg: id_set(lang, &[("(", false), (")", false), (",", false)]),
            cog_nest: id_set(
                lang,
                &[
                    ("for_statement", true),
                    ("for_in_statement", true),
                    ("while_statement", true),
                    ("do_statement", true),
                    ("switch_statement", true),
                    ("catch_clause", true),
                    ("ternary_expression", true),
                ],
            ),
            kw_else: id_set(lang, &[("else", false)]),
            expression_statement: id_set(lang, &[("expression_statement", true)]),
            unary_expression: id_set(lang, &[("unary_expression", true)]),
            binary_expression: id_set(lang, &[("binary_expression", true)]),
            amp_amp: id_set(lang, &[("&&", false)]),
            pipe_pipe: id_set(lang, &[("||", false)]),
        }
    }
}

/// `else_if_via_else_clause`: true for TypeScript, false for JavaScript and TSX
/// (matches rca's per-language `is_else_if`).
pub fn compute(src: &[u8], lang: &Language, else_if_via_else_clause: bool) -> Option<FileMetrics> {
    let mut parser = Parser::new();
    parser.set_language(lang).ok()?;
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();
    let k = Kinds::resolve(lang, else_if_via_else_clause);

    // `program` (the unit) is in `func_space`, so the walk counts it — start at 0.
    let mut c = Counts::default();
    walk(root, &k, &mut c);

    let mut cog = CogState::default();
    cog_walk(root, 0, 0, 0, &k, &mut cog);

    let loc = compute_loc(root, lang);
    let h = compute_halstead(root, src, lang);

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
        tloc: 0.0, // ECMAScript has no inline-test stripping
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

// ── structural ────────────────────────────────────────────────────────────

#[derive(Default)]
struct Counts {
    spaces: u32,
    branches: u32,
    exits: u32,
    args: u32,
    closures: u32,
    functions: u32,
}

/// rca `count_specific_ancestors`: walk parents; stop at a `stop` node; count
/// `check` nodes that aren't else-ifs.
fn count_ancestors(node: Node, check: &HashSet<u16>, stop: &HashSet<u16>, k: &Kinds) -> usize {
    let mut count = 0;
    let mut cur = node;
    while let Some(p) = cur.parent() {
        if stop.contains(&p.kind_id()) {
            break;
        }
        if check.contains(&p.kind_id()) && !is_else_if(p, k) {
            count += 1;
        }
        cur = p;
    }
    count
}

fn is_else_if(node: Node, k: &Kinds) -> bool {
    if !k.if_statement.contains(&node.kind_id()) {
        return false;
    }
    let want = if k.else_if_via_else_clause {
        &k.else_clause
    } else {
        &k.if_statement
    };
    node.parent().is_some_and(|p| want.contains(&p.kind_id()))
}

fn is_child(node: Node, set: &HashSet<u16>) -> bool {
    let mut cur = node.walk();
    node.children(&mut cur).any(|c| set.contains(&c.kind_id()))
}

fn has_sibling(node: Node, set: &HashSet<u16>) -> bool {
    node.parent().is_some_and(|p| {
        let mut cur = p.walk();
        p.children(&mut cur).any(|c| set.contains(&c.kind_id()))
    })
}

fn check_if_func(node: Node, k: &Kinds) -> bool {
    count_ancestors(node, &k.func_assign_anc, &k.func_stop, k) > 0 || is_child(node, &k.identifier)
}
fn check_if_arrow_func(node: Node, k: &Kinds) -> bool {
    count_ancestors(node, &k.arrow_assign_anc, &k.arrow_stop, k) > 0
        || has_sibling(node, &k.property_identifier)
}

fn is_func(node: Node, k: &Kinds) -> bool {
    let id = node.kind_id();
    if id == k.function_declaration || id == k.method_definition {
        true
    } else if id == k.function_expression {
        check_if_func(node, k)
    } else if id == k.arrow_function {
        check_if_arrow_func(node, k)
    } else {
        false
    }
}
fn is_closure(node: Node, k: &Kinds) -> bool {
    let id = node.kind_id();
    if id == k.generator_function || id == k.generator_function_declaration {
        true
    } else if id == k.function_expression {
        !check_if_func(node, k)
    } else if id == k.arrow_function {
        !check_if_arrow_func(node, k)
    } else {
        false
    }
}

fn count_args(node: Node, k: &Kinds) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut cur = params.walk();
    params
        .children(&mut cur)
        .filter(|c| !k.non_arg.contains(&c.kind_id()))
        .count() as u32
}

fn walk(node: Node, k: &Kinds, c: &mut Counts) {
    let id = node.kind_id();
    if k.func_space.contains(&id) {
        c.spaces += 1;
    }
    if is_func(node, k) {
        c.functions += 1;
        c.args += count_args(node, k);
    } else if is_closure(node, k) {
        c.closures += 1;
        c.args += count_args(node, k);
    }
    if k.cyc.contains(&id) {
        c.branches += 1;
    }
    if k.return_statement.contains(&id) {
        c.exits += 1;
    }
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        walk(child, k, c);
    }
}

// ── cognitive ─────────────────────────────────────────────────────────────

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

    if k.if_statement.contains(&id) {
        if !is_else_if(node, k) {
            st.structural += nesting + depth + lambda + 1;
            cn = nesting + 1;
            st.boolean_op = None;
        }
    } else if k.cog_nest.contains(&id) {
        st.structural += nesting + depth + lambda + 1;
        cn = nesting + 1;
        st.boolean_op = None;
    } else if k.kw_else.contains(&id) {
        st.structural += 1;
    } else if k.expression_statement.contains(&id) {
        st.boolean_op = None;
    } else if k.unary_expression.contains(&id) {
        st.boolean_op = Some(id);
    } else if k.binary_expression.contains(&id) {
        let mut cur = node.walk();
        for child in node.children(&mut cur) {
            let cid = child.kind_id();
            if k.amp_amp.contains(&cid) || k.pipe_pipe.contains(&cid) {
                st.eval_boolean(cid);
            }
        }
    } else if id == k.function_declaration {
        cn = 0;
        cl = 0;
        if has_ancestor(node, k.function_declaration) {
            cd = depth + 1;
        }
    } else if id == k.arrow_function {
        cl = lambda + 1;
    }

    let is_space = k.func_space.contains(&id);
    let mut cur = node.walk();
    if is_space {
        let saved = st.boolean_op;
        st.boolean_op = None;
        for child in node.children(&mut cur) {
            cog_walk(child, cn, cd, cl, k, st);
        }
        st.boolean_op = saved;
    } else {
        for child in node.children(&mut cur) {
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
    noop: HashSet<u16>,    // string / " / program
    comment: HashSet<u16>, // comment
    statements: HashSet<u16>,
}
impl LocKinds {
    fn resolve(lang: &Language) -> Self {
        LocKinds {
            noop: id_set(lang, &[("string", true), ("\"", false), ("program", true)]),
            comment: id_set(lang, &[("comment", true)]),
            statements: id_set(
                lang,
                &[
                    ("expression_statement", true),
                    ("export_statement", true),
                    ("import_statement", true),
                    ("statement_block", true),
                    ("if_statement", true),
                    ("switch_statement", true),
                    ("for_statement", true),
                    ("for_in_statement", true),
                    ("while_statement", true),
                    ("do_statement", true),
                    ("try_statement", true),
                    ("with_statement", true),
                    ("break_statement", true),
                    ("continue_statement", true),
                    ("debugger_statement", true),
                    ("return_statement", true),
                    ("throw_statement", true),
                    ("empty_statement", true),
                    ("statement_identifier", true),
                ],
            ),
        }
    }
}

#[derive(Default)]
struct LocState {
    ploc: usize,
    lines: HashSet<usize>,
    only_comment: i64,
    code_comment: i64,
    comment_line_end: Option<usize>,
    lloc: u32,
    blank: i64,
}

fn compute_loc(root: Node, lang: &Language) -> LocState {
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
    } else if lk.comment.contains(&id) {
        add_cloc_lines(st, start, end);
    } else if lk.statements.contains(&id) {
        st.lloc += 1;
    } else {
        check_comment_ends_on_code_line(st, start);
        st.lines.insert(start);
    }
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
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
    operators: HashSet<u16>,
    operands: HashSet<u16>,
}
impl HalKinds {
    fn resolve(lang: &Language) -> Self {
        let op_named = ["ternary_expression", "member_expression"];
        // Operators: mostly anon tokens + a few keyword-ish named.
        let op_anon = [
            "export", "import", "extends", ".", "from", "(", ",", "as", "*", ">>", ">>>", ":",
            "return", "delete", "throw", "break", "continue", "if", "else", "switch", "case",
            "default", "async", "for", "in", "of", "while", "try", "catch", "finally", "with", "=",
            "@", "&&", "||", "+", "-", "--", "++", "/", "%", "**", "|", "&", "<<", "~", "<", "<=",
            "==", "!=", ">=", ">", "+=", "!", "!==", "===", "-=", "*=", "/=", "%=", "**=", ">>=",
            ">>>=", "<<=", "&=", "^", "^=", "|=", "yield", "[", "{", "await", "?", "??", "new",
            "let", "var", "const", "function", ";",
        ];
        let mut operators = HashSet::new();
        for n in op_anon {
            operators.extend(id_set(lang, &[(n, false)]));
        }
        // Named operator kinds (rca's FunctionExpression, and the `import`
        // expression node `Import2`, distinct from the anon `import` keyword).
        operators.extend(id_set(lang, &[("function_expression", true)]));
        operators.extend(id_set(lang, &[("import", true)]));
        let _ = op_named;

        let operands = {
            // NAMED-only operands: value/literal nodes. Resolving these as anon too
            // would wrongly count TS type keywords (`number`/`string`/`void`/… are
            // anon tokens inside `predefined_type`), which rca's named operand kinds
            // do not include.
            let named_operands: &[&str] = &[
                "identifier",
                "nested_identifier",
                "member_expression",
                "property_identifier",
                "string",
                "number",
                "true",
                "false",
                "null",
                "void",
                "this",
                "super",
                "undefined",
            ];
            // Keyword operands that are ANON tokens in the JS grammar but counted as
            // operands by rca.
            let anon_operands: &[&str] = &["set", "get", "typeof", "instanceof"];
            let mut s = HashSet::new();
            for n in named_operands {
                s.extend(id_set(lang, &[(n, true)]));
            }
            for n in anon_operands {
                s.extend(id_set(lang, &[(n, true)]));
                s.extend(id_set(lang, &[(n, false)]));
            }
            s
        };
        HalKinds {
            operators,
            operands,
        }
    }
}

fn compute_halstead(root: Node, src: &[u8], lang: &Language) -> Halstead {
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
    }
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        hal_walk(child, src, hk, operators, operands);
    }
}
