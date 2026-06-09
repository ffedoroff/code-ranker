//! Submodule `gadget`. Target of a second cross-crate, submodule-precise edge
//! (`use helper::gadget::spin` → `helper/src/gadget.rs`).

pub fn spin() -> i32 {
    1
}
