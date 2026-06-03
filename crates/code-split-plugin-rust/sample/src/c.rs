//! Module `c` — a leaf target for `a`'s grouped/glob/path imports. It has an
//! inline submodule to show that `self::`-style child paths resolve too.

pub fn gamma() -> i32 {
    3
}

// Inline (brace) module — DETECTED. Collapses into c.rs's File node; a `use`
// targeting it resolves to this same file.
pub mod helpers {
    pub fn offset() -> i32 {
        0
    }
}
