//! `helper` — a second workspace crate. The root sample imports its
//! **submodules** by path (`helper::widget::…`), so the analyzer must resolve
//! those across the crate boundary to the owning submodule file, not collapse
//! them onto this crate root.

pub mod gadget;
pub mod widget;

/// An item defined at the crate root (no submodule). A `use helper::TOP` from
/// another crate has no deeper submodule to match, so it resolves here — to
/// `helper/src/lib.rs`.
pub const TOP: i32 = 0;
