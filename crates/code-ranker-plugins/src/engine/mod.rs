//! Generic tree-sitter metric engine shared by every language plugin.
//!
//! This is the single in-tree port of `rust-code-analysis`'s node-kind
//! classification (the algorithm of record), generalized over a per-language
//! [`Dialect`]. Each language ships a thin `Dialect` that injects its grammar, a
//! resolved [`Roles`] table (the role-keyed node-kind sets, loaded from its
//! `<lang>.toml`), and the few predicates that genuinely differ between
//! languages (Halstead operator context exceptions, exit rules, the cognitive
//! state-machine extras, closure/function classification, and the LOC
//! special-cases). Everything else — the recursion driver, the cyclomatic /
//! cognitive / Halstead / LOC sub-walks, and the [`MetricInputs`] assembly — is
//! shared here, so the per-language Rust stays minimal.
//!
//! Module layout (acyclic): [`core`] is the leaf — the [`Dialect`] trait, the
//! shared state types and the node helpers. The four sub-walks (`structural` /
//! `cognitive` / `halstead` / `loc`) and this driver depend on `core` and on
//! [`roles`]; nothing depends back on this `mod`.
//!
//! The output is byte-identical to the previous per-language walkers: the role
//! sets carry the same kind STRINGS, and each dialect hook is a faithful copy of
//! the branch it replaces.
//!
//! Terminology: tree-sitter produces a *concrete* syntax tree (CST) — every
//! token (punctuation, keywords, operators) is a node. We say "AST" loosely
//! throughout the codebase and docs; the distinction that actually matters here
//! is *syntax nodes, not text*, which holds either way.
#![allow(dead_code)]

use code_ranker_plugin_api::metrics::{FunctionUnit, MetricInputs};

mod cognitive;
mod core;
mod halstead;
pub mod loc;
mod roles;
mod structural;

pub use core::{
    CogCtx, CogState, Dialect, HalClass, LocState, OpMap, OperandMap, UnitKind, count_args,
    has_ancestor_id,
};
pub use roles::{RoleCfg, Roles};

/// Parse `src` with the dialect's grammar and compute the file-level metrics.
pub fn compute<D: Dialect>(src: &[u8], d: &D) -> Option<MetricInputs> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(d.language()).ok()?;
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();
    Some(measure(root, src, d, d.file_initial_spaces()))
}

/// Per-function metric units: run the same counters over each function-like
/// subtree (`spaces` starts at 0 because the structural walk counts the function
/// node itself, +1 = McCabe base path).
pub fn compute_functions<D: Dialect>(src: &[u8], d: &D) -> Vec<FunctionUnit> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(d.language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Vec::new();
    };
    let mut units = Vec::new();
    collect_functions(tree.root_node(), src, d, &mut units);
    units
}

fn collect_functions<D: Dialect>(
    node: tree_sitter::Node,
    src: &[u8],
    d: &D,
    out: &mut Vec<FunctionUnit>,
) {
    if d.is_function_unit(node) {
        let inputs = measure(node, src, d, 0);
        let name = d
            .unit_name(node, src)
            .unwrap_or_else(|| "<anonymous>".to_string());
        out.push(FunctionUnit {
            kind: d.fn_kind(node).to_string(),
            name,
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            inputs,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_functions(child, src, d, out);
    }
}

/// Run all four sub-walks over `root` and assemble a [`MetricInputs`].
fn measure<D: Dialect>(
    root: tree_sitter::Node,
    src: &[u8],
    d: &D,
    initial_spaces: u32,
) -> MetricInputs {
    let mut c = structural::Counts {
        spaces: initial_spaces,
        ..Default::default()
    };
    structural::walk(root, d, &mut c);

    let mut cog = CogState::default();
    cognitive::walk(root, CogCtx::default(), d, &mut cog);

    let loc = loc::compute(root, d);
    let h = halstead::compute(root, src, d);

    let cloc = (loc.only_comment + loc.code_comment) as f64;
    let span_sloc = root
        .end_position()
        .row
        .saturating_sub(root.start_position().row) as f64;

    MetricInputs {
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
        tloc: 0.0, // set by the caller (rust strips cfg(test)); engine returns 0.
        span_sloc,
    }
}
