//! Case A (cont.) — the spoke depends back on the hub via a real code edge.

// uses reex_spoke → reex_hub: a genuine dependency on the hub's item. This edge
// IS flow, but on its own it is one-directional — no cycle until the hub's
// `reexports` edge back to us also counts (see reex_hub.rs).
use crate::cycle_examples::reex_hub::Hub;

pub struct Widget;

pub fn unpack(_h: Hub) -> Widget {
    Widget
}
