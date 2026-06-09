//! Case 3b (cont.) — the child glob-pulls its parent's namespace AND uses a
//! parent item (`Nest`), so it genuinely depends on the parent.

// super child → sup_parent: GLOB `use super::*` reaches UP the module tree. The
// child really uses `Nest` below, so this IS a real back-dependency — sup_parent
// ⇄ child is, strictly, a cycle. But the edge is recorded as `super` (non-flow),
// so the cycle is NOT reported: a deliberate low-priority miss, deprioritized vs.
// obvious cross-module cycles. (Had the child written the NAMED form
// `use super::Nest`, it would be a `uses` edge and the loop would already count —
// cf. b.rs. Contrast sup_loose, where the child uses no parent item at all.)
use super::*;

pub struct Chick;

pub fn settle(_n: Nest) {}
