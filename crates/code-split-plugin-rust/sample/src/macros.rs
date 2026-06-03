//! Macro definitions. To `syn` these are `Item::Macro`, which the analyzer
//! ignores entirely — neither the definitions here nor their invocations
//! elsewhere create graph nodes or edges, and any dependency referenced inside
//! a macro body is a blind spot.

/// Expands to a function item (used at item position in lib.rs). NOT detected.
macro_rules! make_answer {
    () => {
        pub fn answer() -> i32 {
            42
        }
    };
}

/// Body contains `use crate::c::gamma` — but because macros are never expanded,
/// this dependency is INVISIBLE to the analyzer. NOT detected.
macro_rules! pull_in_c {
    () => {{
        use crate::c::gamma;
        gamma()
    }};
}
