//! Case 3b — a parent that uses a child item, while the child glob-pulls the
//! parent AND really uses one of its items. A 2-node loop split across a `uses`
//! edge (down) and a `super` edge (up).
//!
//! This is a GENUINE mutual dependency — sup_parent ⇄ child is, strictly, a real
//! cycle. code-ranker records the upward edge as `super` (non-flow) and so does
//! NOT report it: a deliberate **low-priority** miss (a file-split module looping
//! back on itself), deprioritized vs. obvious cross-module cycles like a ⇄ b —
//! not a claim that the dependency isn't real. See principles/rust/what-is-cycle.md.

// contains sup_parent → child: file-backed submodule declaration (non-flow).
pub mod child;

// uses sup_parent → child: a real dependency on the child's item (named path).
// This is the flow edge going DOWN.
use self::child::Chick;

pub struct Nest;

pub fn brood() -> Chick {
    Chick
}
