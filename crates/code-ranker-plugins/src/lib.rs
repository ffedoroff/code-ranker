//! Code Ranker language plugins, merged into one crate.
//!
//! Each language lives in its own module under [`languages`] (`rust`, `python`,
//! `js`, `ts`); the JavaScript and TypeScript plugins share the
//! grammar-agnostic engine in [`languages::ecmascript`]. The four plugin structs
//! are re-exported at the crate root (e.g. [`RustPlugin`]).

pub mod config;
pub mod engine;
pub mod languages;
pub mod walk;

/// Test-only helpers shared across the per-language tests (reachable as
/// `crate::test_support::*` from the `#[path]`-wired test modules).
#[cfg(test)]
mod test_support;

pub use languages::c::CPlugin;
pub use languages::cpp::CppPlugin;
pub use languages::csharp::CsharpPlugin;
pub use languages::go::GoPlugin;
pub use languages::js::JsPlugin;
pub use languages::md::MdPlugin;
pub use languages::python::PythonPlugin;
pub use languages::rust::RustPlugin;
pub use languages::ts::TsPlugin;

// Each plugin self-registers via `inventory::submit!` in its module; consumers get
// the full set through `code_ranker_plugin_api::registry()` and a plugin's merged
// config via `LanguagePlugin::config()`. No name→config map and no central list
// here — adding a language is a self-contained module change.
