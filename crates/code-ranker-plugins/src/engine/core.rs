//! Engine core: the [`Dialect`] trait, the shared state types, and the small
//! node helpers — everything the sub-walks key on.
//!
//! This is a LEAF module: the sub-walks (`structural` / `cognitive` / `halstead`
//! / `loc`) and the driver in `mod.rs` all depend on it, and it depends on none
//! of them, so the engine's internal module graph stays acyclic.

use super::roles::Roles;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Node};

/// The per-language hooks the generic engine calls. Everything a language can
/// express as data lives in [`Roles`] (loaded from its `<lang>.toml`); a
/// `Dialect` adds the genuinely-divergent predicates and the grammar.
pub trait Dialect {
    /// The tree-sitter grammar for this dialect.
    fn language(&self) -> &Language;

    /// The resolved role-keyed node-kind sets for this grammar.
    fn roles(&self) -> &Roles;

    // ── structural (cyclomatic / spaces / exits / args / closures) ──────────

    /// Classify a node as a function or closure unit for the structural walk.
    /// `Some(Func)` counts `functions` + `args`; `Some(Closure)` counts
    /// `closures` + `args`; `None` is neither.
    fn classify_unit(&self, node: Node) -> Option<UnitKind>;

    /// Extra `exits` contributed by a node beyond the `exit_kinds` role set
    /// (rust: +1 when a `function_item` declares a return type `-> T`).
    fn extra_exits(&self, _node: Node) -> u32 {
        0
    }

    /// Extra cyclomatic `branches` beyond the `branch_kinds` role set (python:
    /// +1 for a loop-`else`, an `else` keyword whose grandparent is a
    /// `for_statement` / `while_statement`).
    fn extra_branches(&self, _node: Node) -> u32 {
        0
    }

    /// Count the parameters of a function/closure unit. Default: the direct
    /// children of the unit's `parameters` field not in `non_arg_kinds`. The
    /// C-family dialects override this — their parameters nest under a
    /// `function_declarator` rather than sitting on a `parameters` field of the
    /// unit node.
    fn args(&self, node: Node) -> u32 {
        count_args(node, self.roles())
    }

    // ── cognitive ───────────────────────────────────────────────────────────

    /// The divergent cognitive handling for one node: update the running state
    /// and return the child context (nesting/depth/lambda) to recurse with. The
    /// shared driver handles the space save/restore of `boolean_op`.
    fn cog_node(&self, node: Node, ctx: CogCtx, st: &mut CogState) -> CogCtx;

    // ── function-unit collection ──────────────────────────────────────────────

    /// Whether a node is collected as a function-level unit.
    fn is_function_unit(&self, node: Node) -> bool;

    /// The display `kind` string for a function unit (`fn`/`method`/`arrow`/…).
    /// Borrowed from the dialect's config-loaded `[units]` strings.
    fn fn_kind(&self, node: Node) -> &str;

    /// The function unit's name. Default: the `name` field's text. C-family
    /// dialects override this — the name nests under a `function_declarator`.
    fn unit_name(&self, node: Node, src: &[u8]) -> Option<String> {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
            .map(str::to_string)
    }

    // ── Halstead operator / operand classification ───────────────────────────

    /// Classify a node for Halstead, applying any per-language context
    /// exceptions. The default uses only the `operators`/`operands` role sets.
    fn hal_classify(&self, node: Node) -> HalClass {
        let r = self.roles();
        let id = node.kind_id();
        if r.operators.contains(&id) {
            HalClass::Operator
        } else if r.operands.contains(&id) {
            HalClass::Operand
        } else {
            HalClass::Neither
        }
    }

    // ── LOC ───────────────────────────────────────────────────────────────────

    /// Per-language LOC handling for one node. Return `true` if the node was
    /// handled (the shared walk then does nothing for it); `false` to fall
    /// through to the shared default (noop / comment / statement / code-line).
    fn loc_node(&self, _node: Node, _st: &mut LocState) -> bool {
        false
    }

    /// The initial `spaces` count for a file-level [`super::compute`]. Rust/Python
    /// start at 1 (the source_file/module unit is not in any space set, so the
    /// walk won't count it); ECMAScript starts at 0 (`program` IS in
    /// `space_kinds`).
    fn file_initial_spaces(&self) -> u32;
}

/// What kind of unit a node is, for the structural walk's args/closures count.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Func,
    Closure,
}

/// The top-down cognitive context carried through the walk.
#[derive(Clone, Copy, Default)]
pub struct CogCtx {
    pub nesting: u32,
    pub depth: u32,
    pub lambda: u32,
}

/// Cognitive accumulator. `structural` is the running total across all spaces;
/// `boolean_op` tracks the current boolean run (set on the first op, reset at
/// branches / space boundaries) per rca's `eval_based_on_prev`.
#[derive(Default)]
pub struct CogState {
    pub structural: u32,
    pub boolean_op: Option<u16>,
}

impl CogState {
    /// rca's `eval_based_on_prev`: the FIRST operator in a run sets `boolean_op`
    /// and increments; a later DIFFERENT operator increments but leaves
    /// `boolean_op` unchanged; the same operator does not increment.
    pub fn eval_boolean(&mut self, op: u16) {
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

/// The classification of a node for Halstead counting.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HalClass {
    Operator,
    Operand,
    Neither,
}

#[derive(Default)]
pub struct LocState {
    pub ploc: usize, // filled from lines.len() at the end
    pub lines: HashSet<usize>,
    pub only_comment: i64,
    pub code_comment: i64,
    pub comment_line_end: Option<usize>,
    pub lloc: u32,
    pub blank: i64,
}

// ── small shared helpers used by several sub-walks / dialects ─────────────────

/// True if any ancestor of `node` has the single kind id `kind`.
pub fn has_ancestor_id(node: Node, kind: u16) -> bool {
    let mut cur = node;
    while let Some(p) = cur.parent() {
        if p.kind_id() == kind {
            return true;
        }
        cur = p;
    }
    false
}

/// Count the real parameters of a fn/closure: direct children of its
/// `parameters` field not in the `non_arg_kinds` role set.
pub fn count_args(node: Node, roles: &Roles) -> u32 {
    let Some(params) = node.child_by_field_name("parameters") else {
        return 0;
    };
    let mut cursor = params.walk();
    params
        .children(&mut cursor)
        .filter(|c| !roles.non_arg_kinds.contains(&c.kind_id()))
        .count() as u32
}

/// Hashmap-based Halstead accumulators.
pub type OpMap = HashMap<u16, u64>;
pub type OperandMap = HashMap<Vec<u8>, u64>;
