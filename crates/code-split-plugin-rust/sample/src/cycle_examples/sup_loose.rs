//! Case 3a — benign `super`: the parent uses the child, the child globs the
//! parent's namespace but uses NONE of its items.

// contains sup_loose → child: file-backed submodule declaration (non-flow).
pub mod child;

// uses sup_loose → child: parent depends on a child item (flow, down).
use self::child::Pip;

pub struct Bough;

pub fn grow() -> Pip {
    Pip
}
