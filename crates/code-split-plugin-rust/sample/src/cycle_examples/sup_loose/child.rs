//! Case 3a (cont.) — the child globs the parent for convenience but uses NO
//! parent item: a genuine scope-pull with no real back-dependency.

// super child → sup_loose: glob `use super::*` (non-flow). Unlike Case 3b, the
// child references no parent item, so there is no real dependency upward.
// Making `super` flow would report sup_loose ⇄ child as a cycle FALSELY (a false
// positive). The analyzer cannot tell 3a from 3b without resolving which glob'd
// names are actually used — which is why `super` stays non-flow.
use super::*;

pub struct Pip;
