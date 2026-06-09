//! Case A — a re-export hub the spoke also depends on. A 2-node loop split
//! across a `reexports` edge (down) and a `uses` edge (up).

// reexports reex_hub → reex_spoke: re-publish the spoke's type as our own API.
// Non-flow today, so it does NOT close the loop. Flip `reexports` to flow and
// reex_hub ⇄ reex_spoke becomes a `mutual` cycle (the `uses` edge in reex_spoke
// is the return side).
pub use crate::cycle_examples::reex_spoke::Widget;

/// The item reex_spoke imports back — the return side of the would-be loop.
pub struct Hub;
