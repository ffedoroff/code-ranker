//! Cycle-semantics demo.
//!
//! Each pair below isolates ONE edge form and shows whether it closes a cycle.
//! Cycle detection (Kosaraju SCC over FLOW edges) counts a loop only when every
//! edge in it is flow — today that is just `uses`. `contains` / `reexports` /
//! `super` are non-flow, so they stay in the JSON but never close a loop.
//!
//! The `reex_*` and `sup_*` pairs are deliberately wired so they WOULD become
//! `mutual` cycles IF `reexports` / `super` were flow — see
//! `principles/rust/what-is-cycle.md`. With the current algorithm they are NOT
//! cycles.

// contains cycle_examples → {reex_hub, reex_spoke, sup_parent, sup_loose} —
// module ownership (non-flow): declaring a submodule never forms a cycle.
pub mod reex_hub;
pub mod reex_spoke;
// `super` comes in two flavours, indistinguishable to the analyzer without name
// resolution: sup_parent (3b) is a REAL back-dependency we deprioritize;
// sup_loose (3a) is benign scope-sugar that would be a FALSE cycle under flow.
pub mod sup_loose;
pub mod sup_parent;
