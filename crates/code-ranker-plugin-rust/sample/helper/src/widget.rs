//! Submodule `widget`. A cross-crate `use helper::widget::{Widget, make}` must
//! land here (`helper/src/widget.rs`), not on `helper/src/lib.rs`.

pub struct Widget;

pub fn make() -> Widget {
    Widget
}
