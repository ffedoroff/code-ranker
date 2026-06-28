//! The language plugins.
//!
//! Each language lives in its own submodule (`rust`, `python`, `js`,
//! `ts`, `go`); the JavaScript and TypeScript plugins share the
//! grammar-agnostic engine in [`ecmascript`]. The plugin structs are
//! re-exported at the crate root via `lib.rs`.

pub mod c;
pub mod cfamily;
pub mod cpp;
pub mod csharp;
pub mod ecmascript;
pub mod go;
pub mod js;
pub mod md;
pub mod python;
pub mod rust;
pub mod ts;
