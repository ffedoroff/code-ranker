//! Cross-crate dependencies into the `helper` workspace member.
//!
//! With submodule-precise cross-crate resolution, a `use helper::<sub>::Item`
//! resolves to the **submodule file** in `helper`, producing distinct edges:
//!   cross.rs → helper/src/widget.rs
//!   cross.rs → helper/src/gadget.rs
//! A path that stops at a crate-root item falls back to the crate root:
//!   cross.rs → helper/src/lib.rs   (for `helper::TOP`)

// Submodule-precise: edge to helper/src/widget.rs (NOT helper's lib.rs).
use helper::widget::{Widget, make};
// Second submodule: edge to helper/src/gadget.rs.
use helper::gadget::spin;
// Crate-root item, no matching submodule → falls back to helper/src/lib.rs.
use helper::TOP;

pub fn use_helper() -> i32 {
    let _w: Widget = make();
    spin() + TOP
}
