//! Shared config-inheritance layer for the language plugins.
//!
//! Every language ships a `<lang>.toml` that **inherits** the common
//! `defaults.toml` (see [`DEFAULTS`]). [`load`] deep-merges the two into one
//! [`toml::Table`] from which a plugin drives its `levels()` spec overrides,
//! `presets()`, `thresholds()` and the metric-engine node-kind tables — so the
//! per-language Rust stays thin (wiring only) and everything that *can* be data
//! lives in TOML.
//!
//! The merge is generic and language-agnostic, so python / js / ts can adopt the
//! same `defaults.toml` + `<lang>.toml` pattern later without touching this code.
//!
//! This module is a thin facade: the concerns live in submodules so that each
//! plugin's `fan_in` lands on the concern it actually uses, not on one hub file.
//! - [`parse`] — TOML parsing + the `defaults.toml` ⊕ `<lang>.toml` deep-merge.
//! - [`views`] — level-descriptor views (`edge_kinds` / `node_kinds` /
//!   `node_attributes` / `edge_attributes`, `edge_kind_id` / `attr_key`).
//! - [`specs`] — preset catalog, thresholds, `[specs]` description overrides.
//! - [`lookup`] — generic data-list lookups (`units` / `string_list` /
//!   `string_table`).

mod lookup;
mod parse;
mod specs;
mod views;

pub use lookup::{string_list, string_table, units};
pub use parse::{DEFAULTS, load};
// Internal merge helpers, re-exported only for the config unit tests (which
// reach them via `super::*`).
#[cfg(test)]
pub(crate) use parse::{deep_merge, merge_presets};
pub use specs::{
    PresetCfg, SpecOverride, ThresholdCfg, apply_spec_overrides, presets, resolved_presets,
    spec_overrides, thresholds,
};
pub use views::{attr_key, edge_attributes, edge_kind_id, edge_kinds, node_attributes, node_kinds};

#[cfg(test)]
#[path = "../tests/config.rs"]
mod config_tests;
