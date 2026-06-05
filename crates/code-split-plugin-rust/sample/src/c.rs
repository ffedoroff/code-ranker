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

// Inline unit tests. Every line below is inside `#[cfg(test)]`, so the metrics
// pass strips it FIRST: it is excluded from c.rs's `sloc` / `lloc` / `cloc` /
// `blank` (and HK) and counted as `tloc` instead. The items are referenced by
// their own defining path (`crate::c::…`), i.e. this file, so no cross-file
// edge is added.
#[cfg(test)]
mod tests {
    #[test]
    fn gamma_is_three() {
        assert_eq!(crate::c::gamma(), 3);
    }

    #[test]
    fn offset_is_zero() {
        assert_eq!(crate::c::helpers::offset(), 0);
    }
}
