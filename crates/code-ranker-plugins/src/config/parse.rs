//! Config parsing + the `defaults.toml` ⊕ `[base] ⊕ <lang>.toml` inheritance
//! chain. The generic table merge itself lives in
//! [`code_ranker_plugin_api::toml_merge`] (shared with the CLI); this module only
//! reads the embedded plugin config files and folds them through it.

use code_ranker_plugin_api::toml_merge::deep_merge;
use toml::Table;

/// The common base every language inherits.
pub const DEFAULTS: &str = include_str!("../defaults.toml");

/// Parse `defaults.toml` and a language's `<lang>.toml` source and deep-merge
/// them (language overrides base). Panics on malformed TOML — both inputs are
/// `include_str!`'d compile-time constants, so a parse failure is a build-time
/// authoring bug, not a runtime condition.
pub fn load(lang_toml: &str) -> Table {
    load_chain(&[lang_toml])
}

/// Deep-merge an inheritance chain onto `defaults.toml`: `DEFAULTS ⊕ layers[0] ⊕
/// layers[1] ⊕ …`, each layer overriding the accumulated result (see module docs
/// for the per-key rules). A derived language with a base language passes
/// `&[base_lang_toml, lang_toml]` (e.g. js/ts inherit `ecmascript/config.toml`,
/// c/cpp inherit `cfamily/config.toml`); a standalone language passes its single
/// `&[lang_toml]`. Panics on malformed TOML — every layer is an `include_str!`'d
/// compile-time constant, so a parse failure is a build-time authoring bug.
pub fn load_chain(layers: &[&str]) -> Table {
    let mut acc: Table = DEFAULTS.parse().expect("defaults.toml parses");
    for layer in layers {
        let overlay: Table = layer.parse().expect("config layer parses");
        acc = deep_merge(acc, overlay);
    }
    acc
}
