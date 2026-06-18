//! Structural counters: spaces, cyclomatic branches, exits, args, closures.
//!
//! A preorder DFS that counts the shared role sets (`space_kinds`,
//! `branch_kinds`, `exit_kinds`) and defers function/closure classification
//! (args + the closures/functions split) and any extra exits to the [`Dialect`].

use super::core::{Dialect, UnitKind};
use tree_sitter::Node;

#[derive(Default)]
pub struct Counts {
    pub spaces: u32,
    pub branches: u32,
    pub exits: u32,
    pub args: u32,
    pub closures: u32,
    pub functions: u32,
}

pub fn walk<D: Dialect>(node: Node, d: &D, c: &mut Counts) {
    let r = d.roles();
    let id = node.kind_id();

    if r.space_kinds.contains(&id) {
        c.spaces += 1;
    }
    match d.classify_unit(node) {
        Some(UnitKind::Func) => {
            c.functions += 1;
            c.args += d.args(node);
        }
        Some(UnitKind::Closure) => {
            c.closures += 1;
            c.args += d.args(node);
        }
        None => {}
    }
    if r.branch_kinds.contains(&id) {
        c.branches += 1;
    }
    c.branches += d.extra_branches(node);
    if r.exit_kinds.contains(&id) {
        c.exits += 1;
    }
    c.exits += d.extra_exits(node);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, d, c);
    }
}
